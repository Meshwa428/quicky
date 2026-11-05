//! quicky-visualize: turn audio into a shareable MP4.
//!
//!     quicky-visualize FILE
//!
//! Waveform: rounded bars in the theme's accent on black, moving with the
//! sound; quiet stretches show as dots. Still image: a picture you choose,
//! held for the length of the audio. Pick landscape, square, or portrait,
//! press ▶ to preview, then Create Visualization. The result lands next to
//! the original as `name-visual.mp4`.

use gtk::{cairo, gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{
    Frame, Palette, Player, app, canvas,
    draw::{self, Rgb},
    media,
};
use std::{
    cell::RefCell,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.visualize";

/// Loudness samples per second of audio.
const RATE: f64 = 60.0;
/// How much time each bar stands for, in seconds.
const STEP: f64 = 0.06;
const FPS: u32 = 30;

const SHAPES: [(&str, (i32, i32)); 3] = [("Landscape · 16:9", (1280, 720)), ("Square · 1:1", (1080, 1080)), ("Portrait · 9:16", (1080, 1920))];

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Audio Visualizer", &["audio/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if !m.has_audio || m.duration <= 0.0 {
        media::notify("Can't visualize", &format!("{name} has no sound"));
        return;
    }
    let player = match Player::new() {
        Ok(p) => Rc::new(p),
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };
    player.load(path.to_str().unwrap_or_default());
    let accent = draw::hex(&palette.accent);

    let frame = Frame::new(app, "Audio Visualizer", &name, 560, "Create Visualization");
    let body = &frame.body;

    let envelope: Rc<RefCell<Vec<f32>>> = Rc::default();
    let still: Rc<RefCell<Option<(PathBuf, Pixbuf)>>> = Rc::default();
    let shape = Rc::new(std::cell::Cell::new(SHAPES[0].1));
    let use_still = Rc::new(std::cell::Cell::new(false));

    let preview = gtk::DrawingArea::builder().vexpand(true).hexpand(true).build();
    {
        let (envelope, still, shape, use_still, player) = (envelope.clone(), still.clone(), shape.clone(), use_still.clone(), player.clone());
        preview.set_draw_func(move |_, cr, w, h| {
            let (sw, sh) = shape.get();
            let k = (w as f64 / sw as f64).min(h as f64 / sh as f64);
            let (pw, ph) = (sw as f64 * k, sh as f64 * k);
            cr.translate((w as f64 - pw) / 2.0, (h as f64 - ph) / 2.0);
            // Rounded like the preview card, not the output.
            draw::rounded(cr, 0.0, 0.0, pw, ph, 14.0);
            cr.clip();
            let t = player.get_f64("time-pos").unwrap_or(0.0);
            match (use_still.get(), still.borrow().as_ref()) {
                (true, Some((_, img))) => {
                    cr.set_source_rgb(0.0, 0.0, 0.0);
                    let _ = cr.paint();
                    draw::contain(cr, img, 0.0, 0.0, pw, ph);
                }
                _ => bars(cr, pw, ph, &envelope.borrow(), t, accent),
            }
        });
    }
    body.append(&preview);

    let play = gtk::Button::builder().icon_name("media-playback-start-symbolic").tooltip_text("Preview  Space").halign(gtk::Align::Start).build();
    play.add_css_class("flat");
    body.append(&play);

    let style = gtk::DropDown::from_strings(&["Waveform", "Still image"]);
    body.append(&Frame::labelled("Style", &style));
    let shapes = gtk::DropDown::from_strings(&SHAPES.map(|(l, _)| l));
    body.append(&Frame::labelled("Shape", &shapes));
    let choose = gtk::Button::builder().label("Choose image…").halign(gtk::Align::End).visible(false).build();
    body.append(&choose);

    // ---------------------------------------------------------------- wiring

    // The loudness envelope is read once, in the background.
    {
        let (envelope, preview, src) = (envelope.clone(), preview.clone(), path.clone());
        glib::spawn_future_local(async move {
            let env = gio::spawn_blocking(move || loudness(&src)).await.unwrap_or_default();
            *envelope.borrow_mut() = env;
            preview.queue_draw();
        });
    }
    // Animate while playing.
    {
        let (player, preview, play) = (player.clone(), preview.clone(), play.clone());
        glib::timeout_add_local(std::time::Duration::from_millis(33), move || {
            player.drain_events();
            if !player.paused() {
                preview.queue_draw();
            }
            quicky_ui::frame::show_playing(&play, !player.paused());
            glib::ControlFlow::Continue
        });
    }
    {
        let player = player.clone();
        play.connect_clicked(move |_| player.toggle_pause());
    }
    {
        let (shape, preview) = (shape.clone(), preview.clone());
        shapes.connect_selected_notify(move |d| {
            shape.set(SHAPES[d.selected() as usize].1);
            preview.queue_draw();
        });
    }
    let pick_image = {
        let (still, preview, window, apply) = (still.clone(), preview.clone(), frame.window.clone(), frame.primary.clone());
        move || {
            let filter = gtk::FileFilter::new();
            filter.add_mime_type("image/*");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder().title("Still image").filters(&filters).build();
            let (still, preview, apply) = (still.clone(), preview.clone(), apply.clone());
            dialog.open(Some(&window), None::<&gio::Cancellable>, move |res| {
                let Some(p) = res.ok().and_then(|f| f.path()) else { return };
                if let Some(img) = canvas::load_image(&p) {
                    *still.borrow_mut() = Some((p, img));
                    apply.set_sensitive(true);
                    preview.queue_draw();
                }
            });
        }
    };
    {
        let pick_image = pick_image.clone();
        choose.connect_clicked(move |_| pick_image());
    }
    {
        let (use_still, still, choose, preview, apply) = (use_still.clone(), still.clone(), choose.clone(), preview.clone(), frame.primary.clone());
        style.connect_selected_notify(move |d| {
            let on = d.selected() == 1;
            use_still.set(on);
            choose.set_visible(on);
            apply.set_sensitive(!on || still.borrow().is_some());
            if on && still.borrow().is_none() {
                pick_image();
            }
            preview.queue_draw();
        });
    }

    {
        let (frame, player, envelope, still, shape, use_still) = (frame.clone(), player.clone(), envelope.clone(), still.clone(), shape.clone(), use_still.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let out = media::output_path(&path, "-visual", Some("mp4"));
            let (src, size, dur) = (path.clone(), shape.get(), m.duration);
            match (use_still.get(), still.borrow().clone()) {
                (true, Some((image, _))) => {
                    app::finish(&frame, "Creating…", "Visualization created", "Visualization failed", out, move |dst| still_video(&src, &image, dst, size));
                }
                _ => {
                    let env = envelope.borrow().clone();
                    app::finish(&frame, "Creating… (rendering frames)", "Visualization created", "Visualization failed", out, move |dst| {
                        waveform_video(&src, dst, &env, size, dur, accent)
                    });
                }
            }
        });
    }

    let keys = gtk::EventControllerKey::new();
    {
        let player = player.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::space {
                player.toggle_pause();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    frame.window.add_controller(keys);
    frame.window.present();
}

/// Loudness over time, `RATE` values a second, scaled so the loudest is 1.
/// Blocking.
fn loudness(src: &Path) -> Vec<f32> {
    let sr = 8000.0;
    let Ok(mut child) = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(src)
        .args(["-ac", "1", "-ar", "8000", "-f", "s16le", "-"])
        .stdout(Stdio::piped())
        .spawn()
    else {
        return Vec::new();
    };
    let mut raw = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_end(&mut raw);
    }
    let _ = child.wait();
    let chunk = (sr / RATE) as usize * 2;
    let mut env: Vec<f32> = raw
        .chunks(chunk)
        .map(|c| {
            let n = (c.len() / 2).max(1) as f32;
            let sum: f32 = c.chunks(2).map(|b| (i16::from_le_bytes([b[0], *b.get(1).unwrap_or(&0)]) as f32 / 32768.0).powi(2)).sum();
            (sum / n).sqrt()
        })
        .collect();
    let peak = env.iter().cloned().fold(0.0f32, f32::max).max(1e-6);
    for v in &mut env {
        // A slight curve so quieter passages still move.
        *v = (*v / peak).powf(0.85);
    }
    env
}

/// One frame of the waveform at time `t`: bars for the moments leading up
/// to it, newest on the right; near-silence shows as a dot.
fn bars(cr: &cairo::Context, w: f64, h: f64, env: &[f32], t: f64, color: Rgb) {
    cr.set_source_rgb(0.0, 0.0, 0.0);
    let _ = cr.paint();
    let count = ((w / h) * 22.0).round().clamp(14.0, 48.0) as usize;
    let margin = w * 0.05;
    let pitch = (w - 2.0 * margin) / count as f64;
    let bar = pitch * 0.42;
    let level = |time: f64| -> f64 {
        if time < 0.0 || env.is_empty() {
            return 0.0;
        }
        // The loudest moment inside this bar's slice of time.
        let a = (time * RATE) as usize;
        let b = ((time + STEP) * RATE) as usize;
        env.get(a..b.min(env.len())).and_then(|s| s.iter().cloned().reduce(f32::max)).unwrap_or(0.0) as f64
    };
    draw::set(cr, color, 1.0);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_width(bar);
    for i in 0..count {
        let time = t - (count - 1 - i) as f64 * STEP;
        let x = margin + pitch * (i as f64 + 0.5);
        let v = level(time);
        let half = v * h * 0.36;
        if half < bar {
            cr.arc(x, h / 2.0, bar / 2.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        } else {
            cr.move_to(x, h / 2.0 - half);
            cr.line_to(x, h / 2.0 + half);
            let _ = cr.stroke();
        }
    }
}

/// Renders every frame with `bars` and pipes them to ffmpeg with the
/// sound. Blocking.
fn waveform_video(src: &Path, dst: &Path, env: &[f32], (w, h): (i32, i32), duration: f64, color: Rgb) -> bool {
    let Ok(mut child) = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgra", "-s", &format!("{w}x{h}"), "-r", &FPS.to_string(), "-i", "-", "-i"])
        .arg(src)
        .args(["-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart"])
        .arg(dst)
        .stdin(Stdio::piped())
        .spawn()
    else {
        return false;
    };
    let Some(mut stdin) = child.stdin.take() else { return false };
    let Ok(mut surface) = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h) else { return false };
    let frames = (duration * FPS as f64).ceil() as u32;
    for f in 0..frames {
        {
            let Ok(cr) = cairo::Context::new(&surface) else { return false };
            bars(&cr, w as f64, h as f64, env, f as f64 / FPS as f64, color);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let Ok(data) = surface.data() else { return false };
        // Rows may be padded; send exactly w*4 bytes of each.
        let ok = data.chunks(stride).take(h as usize).all(|row| stdin.write_all(&row[..w as usize * 4]).is_ok());
        if !ok {
            return false;
        }
    }
    drop(stdin);
    child.wait().is_ok_and(|s| s.success())
}

/// The picture, fitted on black, held for the length of the audio.
fn still_video(src: &Path, image: &Path, dst: &Path, (w, h): (i32, i32)) -> bool {
    let fit = format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,format=yuv420p");
    media::run(
        Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-loop", "1", "-framerate", "2", "-i"])
            .arg(image)
            .arg("-i")
            .arg(src)
            .args(["-vf", &fit, "-c:v", "libx264", "-tune", "stillimage", "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart"])
            .arg(dst),
    )
}
