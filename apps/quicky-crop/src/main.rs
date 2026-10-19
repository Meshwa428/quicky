//! quicky-crop: cut an image down to a box, saved as a copy.
//!
//!     quicky-crop FILE
//!
//! Drag a box (or its edges), lock it to a common shape, or type an exact
//! size, then Apply. The original is never touched; the result lands next to
//! it as `name-cropped.ext`.

use gtk::{gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{
    Canvas, Frame, Palette, Timeline,
    canvas::{self, Mode},
    media,
    timeline::Marks,
};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.crop";

/// Tangerine's shape presets: label and width / height (`None` is free).
const SHAPES: [(&str, Option<f64>); 6] = [
    ("Free", None),
    ("1:1", Some(1.0)),
    ("16:9", Some(16.0 / 9.0)),
    ("9:16", Some(9.0 / 16.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("3:4", Some(3.0 / 4.0)),
];

fn main() -> glib::ExitCode {
    let file = std::env::args().nth(1).map(PathBuf::from);
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        Palette::load().install();
        match &file {
            Some(path) => build(app, path.clone()),
            None => choose_file(app),
        }
    });
    app.run_with_args::<&str>(&[])
}

fn choose_file(app: &gtk::Application) {
    let filter = gtk::FileFilter::new();
    filter.add_mime_type("image/*");
    filter.add_mime_type("video/*");
    filter.set_name(Some("Images and video"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder().title("Crop").filters(&filters).build();
    let app = app.clone();
    // Keep the app alive while the dialog has no parent window.
    let hold = app.hold();
    dialog.open(None::<&gtk::Window>, None::<&gio::Cancellable>, move |res| {
        drop(hold);
        if let Some(path) = res.ok().and_then(|f| f.path()) {
            build(&app, path);
        }
    });
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    // Video crops on a frame of the clip; a scrubber picks which one.
    let video = (media::kind(&path) == media::Kind::Video).then(|| media::probe(&path)).filter(|m| m.has_video);
    let image = match &video {
        Some(m) => media::grab_frame(&path, m.duration.min(1.0) / 2.0, None).and_then(|png| {
            let p = Pixbuf::from_file(&png).ok();
            let _ = std::fs::remove_file(png);
            p
        }),
        None => canvas::load_image(&path),
    };
    let Some(image) = image else {
        media::notify("Can't crop", &format!("{name} isn't an image or video Quicky can open"));
        return;
    };
    let (iw, ih) = (image.width(), image.height());

    let title = if video.is_some() { "Crop Video" } else { "Crop Image" };
    let frame = Frame::new(app, title, &name, 640, "Apply");
    let reset = frame.reset_button();
    let body = &frame.body;

    let canvas = Canvas::new(image, palette.clone(), Mode::Single);
    body.append(canvas.widget());

    if let Some(m) = &video {
        body.append(&scrubber(&path, m, &canvas, &palette));
    }

    let (shapes, buttons) = Frame::choices(&SHAPES.map(|(label, _)| label));
    body.append(&shapes);

    // Exact size in pixels.
    let width = gtk::Entry::builder().width_chars(6).max_width_chars(6).input_purpose(gtk::InputPurpose::Digits).build();
    let height = gtk::Entry::builder().width_chars(6).max_width_chars(6).input_purpose(gtk::InputPurpose::Digits).build();
    let size = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let w_label = gtk::Label::new(Some("W"));
    let h_label = gtk::Label::new(Some("H"));
    let px = gtk::Label::new(Some("px"));
    for l in [&w_label, &h_label, &px] {
        l.add_css_class("dim");
    }
    size.append(&w_label);
    size.append(&width);
    size.append(&h_label);
    size.append(&height);
    size.append(&px);
    size.set_halign(gtk::Align::Center);
    body.append(&size);

    // ---------------------------------------------------------------- wiring

    // Size fields follow the box; Apply is pointless while it covers it all.
    let show_size = {
        let (canvas, width, height, apply) = (canvas.clone(), width.clone(), height.clone(), frame.primary.clone());
        move || {
            let Some(b) = canvas.boxes().first().copied() else { return };
            let (_, _, w, h) = b.pixels(iw, ih);
            if !width.has_focus() {
                width.set_text(&w.to_string());
            }
            if !height.has_focus() {
                height.set_text(&h.to_string());
            }
            apply.set_sensitive(w < iw || h < ih);
        }
    };
    show_size();
    {
        let show_size = show_size.clone();
        canvas.on_change(move || show_size());
    }

    for (i, b) in buttons.iter().enumerate() {
        let canvas = canvas.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                canvas.set_aspect(SHAPES[i].1);
            }
        });
    }
    // Typing a size frees the shape, since the typed numbers set it.
    for entry in [&width, &height] {
        let (canvas, width, height, buttons) = (canvas.clone(), width.clone(), height.clone(), buttons.clone());
        entry.connect_activate(move |_| {
            let (Ok(w), Ok(h)) = (width.text().trim().parse::<f64>(), height.text().trim().parse::<f64>()) else { return };
            buttons[0].set_active(true);
            canvas.set_size(w, h);
        });
    }
    {
        let (canvas, buttons) = (canvas.clone(), buttons.clone());
        reset.connect_clicked(move |_| {
            buttons[0].set_active(true);
            canvas.reset();
        });
    }

    let running = Rc::new(Cell::new(false));
    let run = {
        let (frame, canvas, running) = (frame.clone(), canvas.clone(), running.clone());
        let path = path.clone();
        Rc::new(move || {
            if !frame.primary.is_sensitive() || running.replace(true) {
                return;
            }
            let Some(b) = canvas.boxes().first().copied() else { return };
            let geometry = b.pixels(iw, ih);
            let out = media::output_path(&path, "-cropped", output_ext(&path));
            frame.busy(true, "Cropping…");
            let (frame, running) = (frame.clone(), running.clone());
            let (src, is_video) = (path.clone(), video.is_some());
            glib::spawn_future_local(async move {
                let dst = out.clone();
                let ok = gio::spawn_blocking(move || if is_video { crop_video(&src, &dst, geometry) } else { crop(&src, &dst, geometry) })
                    .await
                    .unwrap_or(false);
                if ok {
                    let shown = out.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    media::notify("Cropped", &shown);
                    frame.window.close();
                } else {
                    frame.busy(false, "Crop failed");
                    media::notify("Crop failed", "ImageMagick couldn't write the cropped copy");
                    running.set(false);
                }
            });
        })
    };
    {
        let run = run.clone();
        frame.primary.connect_clicked(move |_| run());
    }

    // Enter applies, except while typing a size. Esc closes (Frame).
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let window = frame.window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk::gdk::Key;
            let typing = GtkWindowExt::focus(&window).is_some_and(|w| w.is::<gtk::Text>());
            if matches!(key, Key::Return | Key::KP_Enter) && !typing {
                run();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    frame.window.add_controller(keys);

    frame.window.present();
}

/// Vector images come out as PNG: cropping rasterizes them anyway.
fn output_ext(src: &Path) -> Option<&'static str> {
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    (ext == "svg").then_some("png")
}

/// A strip to pick the frame to crop on, with frame stepping. Frames are
/// grabbed in the background; a newer request wins over a slower one.
fn scrubber(path: &Path, m: &media::Media, canvas: &Canvas, palette: &Palette) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let strip = Timeline::with_marks(m.duration, palette.clone(), 44, Marks::Points);
    column.append(strip.widget());
    let back = gtk::Button::builder().label("◁|").tooltip_text("Previous frame").build();
    let fwd = gtk::Button::builder().label("|▷").tooltip_text("Next frame").build();
    let clock = gtk::Label::builder().hexpand(true).xalign(1.0).build();
    clock.add_css_class("dim");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    for b in [&back, &fwd] {
        b.add_css_class("flat");
        row.append(b);
    }
    row.append(&clock);
    column.append(&row);

    let total = media::fmt_time(m.duration);
    let at = Rc::new(Cell::new(m.duration.min(1.0) / 2.0));
    let wanted = Rc::new(Cell::new(0u32));
    let show = {
        let (path, canvas, strip, clock, at, wanted) = (path.to_path_buf(), canvas.clone(), strip.clone(), clock.clone(), at.clone(), wanted.clone());
        let duration = m.duration;
        Rc::new(move |t: f64| {
            let t = t.clamp(0.0, (duration - 0.04).max(0.0));
            at.set(t);
            strip.set_playhead(t);
            clock.set_text(&format!("{} / {total}", media::fmt_time(t)));
            let ticket = wanted.get() + 1;
            wanted.set(ticket);
            let (path, canvas, wanted) = (path.clone(), canvas.clone(), wanted.clone());
            glib::spawn_future_local(async move {
                let png = gio::spawn_blocking(move || media::grab_frame(&path, t, None)).await.ok().flatten();
                if let Some(png) = png {
                    if wanted.get() == ticket
                        && let Ok(p) = Pixbuf::from_file(&png)
                    {
                        canvas.set_image(p);
                    }
                    let _ = std::fs::remove_file(png);
                }
            });
        })
    };
    show(at.get());
    {
        let show = show.clone();
        strip.on_seek(move |t| show(t));
    }
    for (b, step) in [(&back, -1.0 / 30.0), (&fwd, 1.0 / 30.0)] {
        let (show, at) = (show.clone(), at.clone());
        b.connect_clicked(move |_| show(at.get() + step));
    }
    {
        let (strip, path, media, color) = (strip.clone(), path.to_path_buf(), m.clone(), palette.accent.clone());
        let aspect = (quicky_ui::frame::WIDTH - 32) as f64 / 44.0;
        glib::spawn_future_local(async move {
            let png = gio::spawn_blocking(move || media::backdrop(&path, &media, &color, aspect)).await.ok().flatten();
            if let Some(png) = png {
                strip.set_backdrop(Pixbuf::from_file(&png).ok());
                let _ = std::fs::remove_file(png);
            }
        });
    }
    column
}

/// Crops every frame; the sound is copied as it is. Sizes are kept even,
/// which H.264 needs.
fn crop_video(src: &Path, dst: &Path, (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    let (w, h) = (w / 2 * 2, h / 2 * 2);
    Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(src)
        .args(["-vf", &format!("crop={w}:{h}:{x}:{y}"), "-c:v", "libx264", "-crf", "18", "-preset", "medium", "-c:a", "copy"])
        .arg(dst)
        .status()
        .is_ok_and(|s| s.success())
}

fn crop(src: &Path, dst: &Path, (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    Command::new("magick")
        .arg(src)
        .args(["-auto-orient", "-crop", &format!("{w}x{h}+{x}+{y}"), "+repage"])
        .arg(dst)
        .status()
        .is_ok_and(|s| s.success())
}
