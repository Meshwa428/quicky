//! quicky-normalize: even out how loud a recording is, saved as a copy.
//!
//!     quicky-normalize FILE
//!
//! Listen, adjust the loudness target if you like (the defaults suit most
//! speech and music), then Normalize Audio. The result lands next to the
//! original as `name-normalized.ext`.

use gtk::{glib, prelude::*};
use quicky_ui::{Deck, Frame, Palette, app, media, timeline::Marks};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.normalize";

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Normalize Volume", &["audio/*", "video/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if !m.has_audio {
        media::notify("Can't normalize", &format!("{name} has no sound"));
        return;
    }
    let deck = match Deck::new(&path, &m, &palette, Marks::Points, 96) {
        Ok(d) => d,
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };

    let frame = Frame::new(app, "Normalize Volume", &name, 420, "Normalize Audio");
    let body = &frame.body;
    body.append(deck.timeline.widget());
    body.append(&deck.transport);

    let spin = |lo: f64, hi: f64, step: f64, value: f64| {
        let s = gtk::SpinButton::with_range(lo, hi, step);
        s.set_digits(1);
        s.set_value(value);
        s
    };
    let target = spin(-36.0, -5.0, 0.5, -16.0);
    let range = spin(1.0, 20.0, 0.5, 11.0);
    let peak = spin(-9.0, 0.0, 0.1, -1.5);
    body.append(&Frame::labelled("Target loudness (LUFS)", &target));
    body.append(&Frame::labelled("Loudness range (LU)", &range));
    body.append(&Frame::labelled("True peak limit (dBTP)", &peak));
    let help = gtk::Label::builder()
        .label("−16 LUFS is a balanced default for spoken audio and music. The true peak limit helps prevent clipping after encoding.")
        .xalign(0.0)
        .wrap(true)
        .build();
    help.add_css_class("dim");
    body.append(&help);

    {
        let (frame, player) = (frame.clone(), deck.player.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let (i, lra, tp) = (target.value(), range.value(), peak.value());
            let out = media::output_path(&path, "-normalized", None);
            let src = path.clone();
            app::finish(&frame, "Normalizing…", "Normalized", "Normalize failed", out, move |dst| normalize(&src, dst, i, lra, tp, m.has_video));
        });
    }

    deck.attach(&frame.window);
    frame.window.present();
}

/// EBU R128 loudness normalization; any video is copied untouched.
fn normalize(src: &Path, dst: &Path, i: f64, lra: f64, tp: f64, video: bool) -> bool {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-i"]).arg(src);
    cmd.args(["-af", &format!("loudnorm=I={i}:LRA={lra}:TP={tp}"), "-ar", "48000"]);
    if video {
        cmd.args(["-c:v", "copy", "-c:a", "aac", "-b:a", "192k"]);
    }
    media::run(cmd.arg(dst))
}
