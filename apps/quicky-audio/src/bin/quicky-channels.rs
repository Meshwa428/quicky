//! quicky-channels: turn stereo into mono or mono into stereo, with a
//! volume for each side, saved as a copy.
//!
//!     quicky-channels FILE
//!
//! Each side's waveform has its own ▶. Pick Mono or Stereo, balance the
//! sides, ▶ Play Output to hear the result, then Apply. The result lands
//! next to the original as `name-mono.ext` or `name-stereo.ext`.

use gtk::{gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{Frame, Palette, Player, app, media};
use std::{
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.channels";

/// Waveform height for each side.
const WAVE: i32 = 54;

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Convert Audio Channels", &["audio/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if !m.has_audio {
        media::notify("Can't convert channels", &format!("{name} has no sound"));
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
    let stereo_in = channel_count(&path) > 1;

    let frame = Frame::new(app, "Convert Audio Channels", &name, 470, "Apply");
    let body = &frame.body;

    // Left and right, each with its waveform and a ▶ that plays that side.
    let mut side_plays = Vec::new();
    for (side, label) in [(0usize, "L"), (1, "R")] {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let tag = gtk::Label::builder().label(label).width_chars(2).build();
        tag.add_css_class("dim");
        let wave = gtk::Picture::builder().can_shrink(true).content_fit(gtk::ContentFit::Fill).height_request(WAVE).hexpand(true).build();
        let play = gtk::Button::builder().label("▶").tooltip_text(format!("Play the {} side", if side == 0 { "left" } else { "right" })).build();
        play.add_css_class("flat");
        row.append(&tag);
        row.append(&wave);
        row.append(&play);
        body.append(&row);
        side_plays.push(play);

        // A mono file has the same sound on both sides.
        let channel = if stereo_in { side } else { 0 };
        let (src, color) = (path.clone(), palette.accent.clone());
        glib::spawn_future_local(async move {
            let png = media::temp("side", "png");
            let out = png.clone();
            let ok = gio::spawn_blocking(move || media::waveform_of(&src, &color, Some(channel), &out)).await.unwrap_or(false);
            if ok && let Ok(p) = Pixbuf::from_file(&png) {
                wave.set_paintable(Some(&gtk::gdk::Texture::for_pixbuf(&p)));
            }
            let _ = std::fs::remove_file(png);
        });
    }

    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let (outputs, output_buttons) = Frame::choices(&["Mono", "Stereo"]);
    body.append(&Frame::labelled("Output", &outputs));
    // Stereo files usually become mono, and the other way round.
    output_buttons[if stereo_in { 0 } else { 1 }].set_active(true);
    let pct = |v: f64| format!("{v:.0}%");
    let (left_row, left) = Frame::slider("Left volume", (0.0, 200.0), 1.0, 100.0, pct);
    let (right_row, right) = Frame::slider("Right volume", (0.0, 200.0), 1.0, 100.0, pct);
    body.append(&left_row);
    body.append(&right_row);
    let play_output = gtk::Button::builder().label("▶ Play Output").halign(gtk::Align::Start).build();
    body.append(&play_output);

    // ---------------------------------------------------------------- wiring

    let mono = {
        let b = output_buttons[0].clone();
        move || b.is_active()
    };
    let filter = {
        let (mono, left, right) = (mono.clone(), left.clone(), right.clone());
        move || pan(mono(), stereo_in, left.value() / 100.0, right.value() / 100.0)
    };
    let listen = {
        let player = player.clone();
        move |af: String| {
            player.set("af", &format!("lavfi=[{af}]"));
            player.seek(0.0);
            player.set("pause", "no");
        }
    };
    for (side, b) in side_plays.iter().enumerate() {
        let listen = listen.clone();
        b.connect_clicked(move |_| {
            let c = if stereo_in { side } else { 0 };
            listen(format!("pan=stereo|c0=c{c}|c1=c{c}"));
        });
    }
    {
        let (listen, filter) = (listen.clone(), filter.clone());
        play_output.connect_clicked(move |_| listen(filter()));
    }

    {
        let (frame, player) = (frame.clone(), player.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let to_mono = mono();
            let out = media::output_path(&path, if to_mono { "-mono" } else { "-stereo" }, None);
            let (src, af) = (path.clone(), filter());
            app::finish(&frame, "Converting…", "Channels converted", "Convert failed", out, move |dst| {
                media::run(Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&src).args(["-af", &af]).arg(dst))
            });
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
    {
        let player = player.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            player.drain_events();
            glib::ControlFlow::Continue
        });
    }
    frame.window.present();
}

/// The `pan` filter for the chosen output; `l` and `r` are each side's
/// volume (1 = as is).
fn pan(mono: bool, stereo_in: bool, l: f64, r: f64) -> String {
    let (cl, cr) = if stereo_in { ("c0", "c1") } else { ("c0", "c0") };
    if mono {
        format!("pan=mono|c0={:.3}*{cl}+{:.3}*{cr}", l / 2.0, r / 2.0)
    } else {
        format!("pan=stereo|c0={l:.3}*{cl}|c1={r:.3}*{cr}")
    }
}

fn channel_count(path: &Path) -> u32 {
    Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=channels", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(2)
}
