//! quicky-speed: slow a video down or speed it up, saved as a copy.
//!
//!     quicky-speed FILE
//!
//! Slide to a speed (0.25× to 4×) and play to preview it; the sound keeps
//! its pitch. The result lands next to the original as `name-2x.ext`.

use gtk::{glib, prelude::*};
use quicky_ui::{Deck, Frame, Palette, app, media, timeline::Marks};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.speed";

/// Tangerine's stops.
const SPEEDS: [f64; 6] = [0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Change Speed", &["video/*"], build)
}

fn label(s: f64) -> String {
    format!("{}×", s)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if m.duration <= 0.0 {
        media::notify("Can't change speed", &format!("{name} has no duration"));
        return;
    }
    let deck = match Deck::new(&path, &m, &palette, Marks::Points, 40) {
        Ok(d) => d,
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };

    let frame = Frame::new(app, "Change Video Speed", &name, 600, "Apply");
    let body = &frame.body;
    body.append(&deck.screen);
    body.append(deck.timeline.widget());
    body.append(&deck.transport);

    // The slider moves between the stops, not continuously.
    let (caption, readout) = Frame::caption("Speed");
    body.append(&caption);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, (SPEEDS.len() - 1) as f64, 1.0);
    scale.set_draw_value(false);
    scale.set_round_digits(0);
    for (i, s) in SPEEDS.iter().enumerate() {
        scale.add_mark(i as f64, gtk::PositionType::Bottom, Some(&label(*s)));
    }
    scale.set_value(2.0);
    body.append(&scale);
    let length = gtk::Label::builder().xalign(0.0).build();
    length.add_css_class("dim");
    body.append(&length);

    let speed = {
        let scale = scale.clone();
        move || SPEEDS[(scale.value().round() as usize).min(SPEEDS.len() - 1)]
    };
    let show = {
        let (readout, length, player, apply, speed) = (readout.clone(), length.clone(), deck.player.clone(), frame.primary.clone(), speed.clone());
        move || {
            let s = speed();
            readout.set_text(&label(s));
            length.set_text(&format!("New length {}", media::fmt_time(m.duration / s)));
            player.set("speed", &format!("{s}"));
            apply.set_sensitive(s != 1.0);
        }
    };
    show();
    {
        let show = show.clone();
        scale.connect_value_changed(move |s| {
            // Snap to the nearest stop.
            let r = s.value().round();
            if (s.value() - r).abs() > 1e-9 {
                s.set_value(r);
                return;
            }
            show();
        });
    }

    {
        let (frame, player) = (frame.clone(), deck.player.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let s = speed();
            let suffix = format!("-{}x", format!("{s}").replace('.', "_"));
            let out = media::output_path(&path, &suffix, None);
            let src = path.clone();
            app::finish(&frame, "Changing speed…", "Speed changed", "Speed change failed", out, move |dst| retime(&src, dst, s, m.has_audio));
        });
    }

    deck.attach(&frame.window);
    frame.window.present();
}

/// `atempo` takes 0.5..2 at a time, so bigger changes chain several.
fn atempo(mut s: f64) -> String {
    let mut parts = Vec::new();
    while s > 2.0 {
        parts.push("atempo=2".to_string());
        s /= 2.0;
    }
    while s < 0.5 {
        parts.push("atempo=0.5".to_string());
        s /= 0.5;
    }
    parts.push(format!("atempo={s}"));
    parts.join(",")
}

fn retime(src: &Path, dst: &Path, s: f64, audio: bool) -> bool {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-i"]).arg(src);
    cmd.args(["-filter:v", &format!("setpts=PTS/{s}"), "-c:v", "libx264", "-crf", "18", "-preset", "medium"]);
    if audio {
        cmd.args(["-filter:a", &atempo(s), "-c:a", "aac", "-b:a", "192k"]);
    }
    media::run(cmd.arg(dst))
}
