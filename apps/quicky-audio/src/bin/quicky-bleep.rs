//! quicky-bleep: beep over parts of a recording, saved as a copy.
//!
//!     quicky-bleep FILE
//!
//! Drag across the Bleep layer above the waveform to add a section; drag a
//! section or its edges to adjust it, Delete removes the selected one.
//! Compare Original and With Bleeps, then save. The result lands next to
//! the original as `name-bleeped.ext`.

use gtk::{gio, glib, prelude::*};
use quicky_ui::{Deck, Frame, Palette, app, media, timeline::Marks};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.bleep";

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Bleep Audio", &["audio/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if !m.has_audio || m.duration <= 0.0 {
        media::notify("Can't bleep", &format!("{name} has no sound"));
        return;
    }
    let deck = match Deck::new(&path, &m, &palette, Marks::Regions, 130) {
        Ok(d) => d,
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };

    let frame = Frame::new(app, "Bleep Audio", &name, 400, "Save Bleeped Copy");
    let reset = frame.reset_button();
    let body = &frame.body;
    let (caption, count) = Frame::caption("Drag across the Bleep layer · Del removes");
    body.append(&caption);
    body.append(deck.timeline.widget());
    body.append(&deck.transport);
    let (previews, preview_buttons) = Frame::choices(&["Original", "▶ With Bleeps"]);
    body.append(&Frame::labelled("Preview", &previews));

    let regions: Rc<RefCell<Vec<(f64, f64)>>> = Rc::default();
    {
        let (timeline, regions, count, apply, original) = (deck.timeline.clone(), regions.clone(), count.clone(), frame.primary.clone(), preview_buttons[0].clone());
        deck.timeline.on_regions(move || {
            *regions.borrow_mut() = timeline.regions();
            let n = regions.borrow().len();
            count.set_text(&format!("Bleeps {n}"));
            apply.set_sensitive(n > 0);
            // A changed bleep makes any rendered preview stale.
            original.set_active(true);
        });
    }
    frame.primary.set_sensitive(false);
    count.set_text("Bleeps 0");

    // With Bleeps renders a quick preview copy and plays that instead.
    let preview_file = media::temp("bleep", "wav");
    {
        let (player, path, deck) = (deck.player.clone(), path.clone(), deck.clone());
        preview_buttons[0].connect_toggled(move |b| {
            if b.is_active() {
                let t = deck.now();
                player.load(path.to_str().unwrap_or_default());
                player.command(&["seek", &format!("{t:.3}"), "absolute"]);
            }
        });
    }
    {
        let (player, path, regions, deck, preview_file, status) = (deck.player.clone(), path.clone(), regions.clone(), deck.clone(), preview_file.clone(), frame.status.clone());
        preview_buttons[1].connect_toggled(move |b| {
            if !b.is_active() {
                return;
            }
            let (src, dst, r) = (path.clone(), preview_file.clone(), regions.borrow().clone());
            let (player, deck, status) = (player.clone(), deck.clone(), status.clone());
            status.set_text("Preparing preview…");
            glib::spawn_future_local(async move {
                let out = dst.clone();
                let ok = gio::spawn_blocking(move || bleep(&src, &out, &r)).await.unwrap_or(false);
                status.set_text("");
                if ok {
                    let t = deck.now();
                    player.load(dst.to_str().unwrap_or_default());
                    player.command(&["seek", &format!("{t:.3}"), "absolute"]);
                    player.set("pause", "no");
                }
            });
        });
    }
    {
        let (timeline, regions) = (deck.timeline.clone(), regions.clone());
        reset.connect_clicked(move |_| {
            timeline.clear_regions();
            regions.borrow_mut().clear();
        });
    }

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let timeline = deck.timeline.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk::gdk::Key;
            if matches!(key, Key::Delete | Key::BackSpace | Key::KP_Delete) && timeline.remove_selected() {
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    frame.window.add_controller(keys);

    {
        let (frame, regions, player) = (frame.clone(), regions.clone(), deck.player.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let out = media::output_path(&path, "-bleeped", None);
            let (src, r) = (path.clone(), regions.borrow().clone());
            app::finish(&frame, "Bleeping…", "Bleeped", "Bleep failed", out, move |dst| bleep(&src, dst, &r));
        });
    }
    {
        let preview_file = preview_file.clone();
        frame.window.connect_destroy(move |_| {
            let _ = std::fs::remove_file(&preview_file);
        });
    }

    deck.attach(&frame.window);
    frame.window.present();
}

/// Silences each section and lays a 1 kHz tone over it.
fn bleep(src: &Path, dst: &Path, regions: &[(f64, f64)]) -> bool {
    let inside = if regions.is_empty() {
        "0".to_string()
    } else {
        regions.iter().map(|(a, b)| format!("between(t,{a:.3},{b:.3})")).collect::<Vec<_>>().join("+")
    };
    let graph = format!(
        "[0:a]volume=0:enable='{inside}'[muted];\
         sine=frequency=1000:sample_rate=48000,volume='if({inside},0.35,0)':eval=frame[tone];\
         [muted][tone]amix=inputs=2:duration=first:normalize=0[a]"
    );
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-i"]).arg(src);
    cmd.args(["-filter_complex", &graph, "-map", "[a]"]);
    media::run(cmd.arg(dst))
}
