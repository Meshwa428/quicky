//! An mpv player drawn into a `gtk::Picture`.
//!
//! mpv decodes (hardware-accelerated where it can) and renders each frame
//! into a CPU buffer through its software render API; the buffer becomes a
//! GDK texture. That avoids sharing an OpenGL context with GTK, and at
//! preview sizes the copy is cheap. mpv plays every format ffmpeg does.

use gtk::{gdk, glib, prelude::*};
use libmpv2_sys as mpv;
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_char, c_int, c_void},
    ptr,
    rc::Rc,
};

/// Longest edge a frame is ever rendered at; normally the on-screen size of
/// the picture is smaller and wins, which keeps software rendering cheap.
const MAX_EDGE: f64 = 1920.0;

pub struct Player {
    inner: Rc<Inner>,
}

struct Inner {
    handle: *mut mpv::mpv_handle,
    render: Cell<*mut mpv::mpv_render_context>,
    picture: gtk::Picture,
    buffer: RefCell<Vec<u8>>,
    // Owned here so the pointer handed to mpv's callback outlives it.
    _wakeup: Box<async_channel::Sender<()>>,
}

// Called on an mpv thread: no mpv calls allowed here, just wake the UI.
unsafe extern "C" fn on_update(ctx: *mut c_void) {
    let tx = unsafe { &*(ctx as *const async_channel::Sender<()>) };
    let _ = tx.try_send(());
}

impl Player {
    pub fn new() -> anyhow::Result<Self> {
        // mpv refuses to start unless numbers parse the C way, and GTK has
        // just switched the process to the user's locale.
        unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };

        let handle = unsafe { mpv::mpv_create() };
        anyhow::ensure!(!handle.is_null(), "mpv_create failed");
        for (k, v) in [
            ("config", "no"),
            ("terminal", "no"),
            ("vo", "libmpv"),
            ("hwdec", "auto-safe"),
            ("keep-open", "always"),
            ("idle", "yes"),
            ("pause", "yes"),
            ("osc", "no"),
            ("input-default-bindings", "no"),
            ("audio-display", "no"),
            ("hr-seek", "yes"),
        ] {
            set_option(handle, k, v);
        }
        anyhow::ensure!(unsafe { mpv::mpv_initialize(handle) } >= 0, "mpv_initialize failed");

        let (tx, rx) = async_channel::bounded::<()>(1);
        let wakeup = Box::new(tx);

        let mut render = ptr::null_mut();
        let mut params = [
            param(mpv::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE, c"sw".as_ptr() as *mut c_void),
            param(mpv::mpv_render_param_type_MPV_RENDER_PARAM_INVALID, ptr::null_mut()),
        ];
        let rc = unsafe { mpv::mpv_render_context_create(&mut render, handle, params.as_mut_ptr()) };
        if rc < 0 {
            unsafe { mpv::mpv_terminate_destroy(handle) };
            anyhow::bail!("mpv software renderer unavailable ({rc})");
        }
        unsafe {
            mpv::mpv_render_context_set_update_callback(
                render,
                Some(on_update),
                &*wakeup as *const _ as *mut c_void,
            )
        };

        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .hexpand(true)
            .vexpand(true)
            .build();

        let inner = Rc::new(Inner {
            handle,
            render: Cell::new(render),
            picture,
            buffer: RefCell::new(Vec::new()),
            _wakeup: wakeup,
        });

        let weak = Rc::downgrade(&inner);
        glib::spawn_future_local(async move {
            while rx.recv().await.is_ok() {
                let Some(inner) = weak.upgrade() else { break };
                inner.draw();
            }
        });

        Ok(Player { inner })
    }

    pub fn widget(&self) -> &gtk::Picture {
        &self.inner.picture
    }

    pub fn load(&self, path: &str) {
        self.command(&["loadfile", path, "replace"]);
    }

    pub fn command(&self, args: &[&str]) {
        let owned: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap_or_default()).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|s| s.as_ptr()).collect();
        ptrs.push(ptr::null());
        unsafe { mpv::mpv_command(self.inner.handle, ptrs.as_mut_ptr()) };
    }

    pub fn set(&self, name: &str, value: &str) {
        let (n, v) = (CString::new(name).unwrap(), CString::new(value).unwrap());
        unsafe { mpv::mpv_set_property_string(self.inner.handle, n.as_ptr(), v.as_ptr()) };
    }

    pub fn get(&self, name: &str) -> Option<String> {
        get_string(self.inner.handle, name)
    }

    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.get(name)?.parse().ok()
    }

    pub fn paused(&self) -> bool {
        self.get("pause").as_deref() != Some("no")
    }

    pub fn toggle_pause(&self) {
        self.command(&["cycle", "pause"]);
    }

    pub fn seek(&self, seconds: f64) {
        self.command(&["seek", &format!("{seconds:.3}"), "absolute+exact"]);
    }

    /// Empties mpv's event queue; call this regularly (the UI's poll timer).
    pub fn drain_events(&self) {
        loop {
            let event = unsafe { mpv::mpv_wait_event(self.inner.handle, 0.0) };
            if event.is_null() || unsafe { (*event).event_id } == mpv::mpv_event_id_MPV_EVENT_NONE {
                break;
            }
        }
    }

    /// Loops playback between `a` and `b` (seconds); `None` turns looping off.
    pub fn set_loop(&self, range: Option<(f64, f64)>) {
        match range {
            Some((a, b)) => {
                self.set("ab-loop-a", &format!("{a:.3}"));
                self.set("ab-loop-b", &format!("{b:.3}"));
            }
            None => {
                self.set("ab-loop-a", "no");
                self.set("ab-loop-b", "no");
            }
        }
    }
}

impl Inner {
    fn draw(&self) {
        let render = self.render.get();
        if render.is_null() {
            return;
        }
        let flags = unsafe { mpv::mpv_render_context_update(render) };
        if flags & mpv::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64 == 0 {
            return;
        }

        let (dw, dh) = (
            get_string(self.handle, "dwidth").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
            get_string(self.handle, "dheight").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
        );
        if dw <= 0 || dh <= 0 {
            return;
        }
        // Render at the size the picture actually occupies on screen.
        let factor = self.picture.scale_factor().max(1) as f64;
        let (pw, ph) = (self.picture.width() as f64 * factor, self.picture.height() as f64 * factor);
        let fit = if pw > 1.0 && ph > 1.0 { (pw / dw as f64).min(ph / dh as f64) } else { 1.0 };
        let scale = fit.min(MAX_EDGE / dw.max(dh) as f64).min(1.0);
        let (w, h) = (((dw as f64 * scale) as c_int).max(2), ((dh as f64 * scale) as c_int).max(2));
        // mpv prefers 64-byte aligned rows.
        let mut stride: usize = ((w as usize * 4) + 63) & !63;

        let mut buffer = self.buffer.borrow_mut();
        buffer.resize(stride * h as usize, 0);
        let mut size = [w, h];
        let mut params = [
            param(mpv::mpv_render_param_type_MPV_RENDER_PARAM_SW_SIZE, size.as_mut_ptr() as *mut c_void),
            param(mpv::mpv_render_param_type_MPV_RENDER_PARAM_SW_FORMAT, c"rgb0".as_ptr() as *mut c_void),
            param(mpv::mpv_render_param_type_MPV_RENDER_PARAM_SW_STRIDE, &mut stride as *mut usize as *mut c_void),
            param(mpv::mpv_render_param_type_MPV_RENDER_PARAM_SW_POINTER, buffer.as_mut_ptr() as *mut c_void),
            param(mpv::mpv_render_param_type_MPV_RENDER_PARAM_INVALID, ptr::null_mut()),
        ];
        if unsafe { mpv::mpv_render_context_render(render, params.as_mut_ptr()) } < 0 {
            return;
        }

        let bytes = glib::Bytes::from(&buffer[..]);
        let texture = gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::R8g8b8x8, &bytes, stride);
        self.picture.set_paintable(Some(&texture));
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Free the renderer first so mpv stops calling back into us.
        let render = self.render.replace(ptr::null_mut());
        unsafe {
            if !render.is_null() {
                mpv::mpv_render_context_free(render);
            }
            mpv::mpv_terminate_destroy(self.handle);
        }
    }
}

fn param(kind: mpv::mpv_render_param_type, data: *mut c_void) -> mpv::mpv_render_param {
    mpv::mpv_render_param { type_: kind, data }
}

fn set_option(handle: *mut mpv::mpv_handle, name: &str, value: &str) {
    let (n, v) = (CString::new(name).unwrap(), CString::new(value).unwrap());
    unsafe { mpv::mpv_set_option_string(handle, n.as_ptr(), v.as_ptr()) };
}

fn get_string(handle: *mut mpv::mpv_handle, name: &str) -> Option<String> {
    let n = CString::new(name).ok()?;
    let raw = unsafe { mpv::mpv_get_property_string(handle, n.as_ptr()) };
    if raw.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
    unsafe { mpv::mpv_free(raw as *mut c_void) };
    Some(value)
}
