//! quicky-split: cut a video into clips at chosen points.
//!
//!     quicky-split FILE
//!
//! Add Split puts a cut at the playhead; Equal clips spaces N clips evenly.
//! Click a cut on the strip's list to remove it. The clips land next to the
//! original as `name-part1.ext`, `name-part2.ext`, ...

use gtk::{glib, prelude::*};
use quicky_ui::{Deck, Frame, Palette, app, media, timeline::Marks};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.split";

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Split Video", &["video/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if m.duration <= 0.0 {
        media::notify("Can't split", &format!("{name} has no duration"));
        return;
    }
    let deck = match Deck::new(&path, &m, &palette, Marks::Points, 56) {
        Ok(d) => d,
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };

    let frame = Frame::new(app, "Split Video", &name, 640, "Split Video");
    let reset = frame.reset_button();
    reset.set_label("Clear");
    let body = &frame.body;
    body.append(&deck.screen);
    let (caption, count) = Frame::caption("Split points");
    body.append(&caption);
    body.append(deck.timeline.widget());
    let add = gtk::Button::with_label("Add Split");
    deck.transport.append(&add);
    body.append(&deck.transport);

    let clips = gtk::SpinButton::with_range(2.0, 50.0, 1.0);
    clips.set_value(2.0);
    let set = gtk::Button::with_label("Set");
    let equal = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    equal.append(&gtk::Label::builder().label("Equal clips").xalign(0.0).hexpand(true).build());
    equal.append(&clips);
    equal.append(&set);
    body.append(&equal);

    let points: Rc<RefCell<Vec<f64>>> = Rc::default();
    let refresh: Rc<dyn Fn()> = {
        let (points, count, timeline, apply) = (points.clone(), count.clone(), deck.timeline.clone(), frame.primary.clone());
        Rc::new(move || {
            let mut p = points.borrow_mut();
            p.sort_by(f64::total_cmp);
            p.dedup_by(|a, b| (*a - *b).abs() < 0.05);
            count.set_text(&match p.len() {
                0 => "No split points".to_string(),
                1 => "1 split point · 2 clips".to_string(),
                n => format!("{n} split points · {} clips", n + 1),
            });
            timeline.set_markers(p.clone());
            apply.set_sensitive(!p.is_empty());
        })
    };
    refresh();

    {
        let (deck, points, refresh) = (deck.clone(), points.clone(), refresh.clone());
        add.connect_clicked(move |_| {
            let t = deck.now();
            if t > 0.05 && t < m.duration - 0.05 {
                points.borrow_mut().push(t);
                refresh();
            }
        });
    }
    {
        let (points, refresh, clips) = (points.clone(), refresh.clone(), clips.clone());
        set.connect_clicked(move |_| {
            let n = clips.value() as usize;
            *points.borrow_mut() = (1..n).map(|i| m.duration * i as f64 / n as f64).collect();
            refresh();
        });
    }
    {
        let (points, refresh) = (points.clone(), refresh.clone());
        reset.connect_clicked(move |_| {
            points.borrow_mut().clear();
            refresh();
        });
    }

    {
        let (frame, points, player) = (frame.clone(), points.clone(), deck.player.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let mut cuts = vec![0.0];
            cuts.extend(points.borrow().iter().copied());
            cuts.push(m.duration);
            let first = media::output_path(&path, "-part1", None);
            let src = path.clone();
            app::finish(&frame, "Splitting… (re-encoding)", "Split", "Split failed", first, move |_| {
                cuts.windows(2).enumerate().all(|(i, w)| cut(&src, &media::output_path(&src, &format!("-part{}", i + 1), None), w[0], w[1]))
            });
        });
    }

    deck.attach(&frame.window);
    frame.window.present();
}

/// One clip, cut exactly (re-encoded from the cut point).
fn cut(src: &Path, dst: &Path, start: f64, end: f64) -> bool {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-ss", &format!("{start:.3}"), "-to", &format!("{end:.3}"), "-i"]).arg(src);
    let webm = dst.extension().is_some_and(|e| e.eq_ignore_ascii_case("webm"));
    if webm {
        cmd.args(["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-c:a", "libopus"]);
    } else {
        cmd.args(["-c:v", "libx264", "-crf", "18", "-preset", "medium", "-c:a", "aac", "-b:a", "192k"]);
    }
    media::run(cmd.arg(dst))
}
