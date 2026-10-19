//! quicky-trim: keep one section of a video or audio file, saved as a copy.
//!
//!     quicky-trim FILE
//!
//! Drag the ends of the strip, check the cut with the transport, then
//! Apply. Click Start or End to type an exact timecode. The original is
//! never touched; the result lands next to it as `name-trimmed.ext`.

use gtk::{gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{Frame, Palette, Player, Timeline, frame::WIDTH, media};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.trim";

/// Strip heights: a filmstrip under a video, a taller waveform for audio.
const STRIP_VIDEO: i32 = 64;
const STRIP_AUDIO: i32 = 110;

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
    filter.add_mime_type("video/*");
    filter.add_mime_type("audio/*");
    filter.set_name(Some("Video and audio"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder().title("Trim").filters(&filters).build();
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
    let media = media::probe(&path);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    if media.duration <= 0.0 {
        media::notify("Can't trim", &format!("{name} has no duration to trim"));
        return;
    }
    let video = media.has_video;

    let player = match Player::new() {
        Ok(p) => Rc::new(p),
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };
    player.load(path.to_str().unwrap_or_default());

    let frame = Frame::new(
        app,
        if video { "Trim Video" } else { "Trim Audio" },
        &name,
        if video { 640 } else { 330 },
        "Apply",
    );
    let body = &frame.body;

    if video {
        let screen = gtk::Frame::new(None);
        screen.add_css_class("video");
        screen.set_child(Some(player.widget()));
        screen.set_vexpand(true);
        body.append(&screen);
    }

    let (caption, selected) = Frame::caption("Drag the ends to trim");
    body.append(&caption);
    let timeline = Timeline::new(media.duration, palette.clone(), if video { STRIP_VIDEO } else { STRIP_AUDIO });
    body.append(timeline.widget());

    // Start / End read as plain text; clicking one opens a timecode entry.
    let start = time_button();
    let end = time_button();
    let ends = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let gap = gtk::Box::builder().hexpand(true).build();
    ends.append(&start.0);
    ends.append(&gap);
    ends.append(&end.0);
    body.append(&ends);

    // Transport.
    let back = gtk::Button::builder().label("◁|").tooltip_text("Previous frame  ,").build();
    let play = gtk::Button::builder().label("▶").tooltip_text("Play / pause  Space").build();
    let fwd = gtk::Button::builder().label("|▷").tooltip_text("Next frame  .").build();
    let position = gtk::Label::builder().hexpand(true).xalign(1.0).build();
    position.add_css_class("dim");
    let transport = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    for b in [&back, &play, &fwd] {
        b.add_css_class("flat");
        transport.append(b);
    }
    transport.append(&position);
    body.append(&transport);

    // Audio has no picture to watch, so it gets the two options Tangerine
    // shows instead.
    let silent = gtk::CheckButton::with_label("Trim silent ends automatically");
    let play_range = gtk::Button::with_label("▶ Play selected range");
    if !video {
        back.set_visible(false);
        fwd.set_visible(false);
        body.append(&silent);
        play_range.set_halign(gtk::Align::Start);
        body.append(&play_range);
    }

    // ---------------------------------------------------------------- wiring

    let show_range = {
        let (start, end, selected) = (start.0.clone(), end.0.clone(), selected.clone());
        move |s: f64, e: f64| {
            start.set_label(&format!("Start {}", media::fmt_time(s)));
            end.set_label(&format!("End {}", media::fmt_time(e)));
            selected.set_text(&format!("{} selected", media::fmt_time(e - s)));
        }
    };
    show_range(0.0, media.duration);

    // Dragging an end previews the frame under it.
    {
        let (player, show_range) = (player.clone(), show_range.clone());
        timeline.on_range(move |s, e, moving_start| {
            show_range(s, e);
            player.set_loop(None);
            player.set("pause", "yes");
            player.seek(if moving_start { s } else { e });
        });
    }
    {
        let player = player.clone();
        timeline.on_seek(move |t| player.seek(t));
    }

    // A typed timecode moves its end of the range.
    for ((button, entry, pop), is_start) in [(&start, true), (&end, false)] {
        {
            let (entry, pop, timeline) = (entry.clone(), pop.clone(), timeline.clone());
            button.connect_clicked(move |_| {
                let (s, e) = timeline.range();
                entry.set_text(&media::fmt_time(if is_start { s } else { e }));
                pop.popup();
                entry.grab_focus();
            });
        }
        let (timeline, player, show_range, pop) = (timeline.clone(), player.clone(), show_range.clone(), pop.clone());
        entry.connect_activate(move |entry| {
            let (s, e) = timeline.range();
            if let Some(t) = media::parse_time(&entry.text()) {
                if is_start { timeline.set_range(t, e) } else { timeline.set_range(s, t) }
                let (s, e) = timeline.range();
                show_range(s, e);
                player.seek(if is_start { s } else { e });
            }
            pop.popdown();
        });
    }

    {
        let player = player.clone();
        back.connect_clicked(move |_| player.command(&["frame-back-step"]));
    }
    {
        let player = player.clone();
        fwd.connect_clicked(move |_| player.command(&["frame-step"]));
    }
    {
        let player = player.clone();
        play.connect_clicked(move |_| {
            player.set_loop(None);
            player.toggle_pause();
        });
    }
    let loop_range = {
        let (player, timeline) = (player.clone(), timeline.clone());
        move || {
            let (s, e) = timeline.range();
            player.set_loop(Some((s, e)));
            player.seek(s);
            player.set("pause", "no");
        }
    };
    {
        let loop_range = loop_range.clone();
        play_range.connect_clicked(move |_| loop_range());
    }

    // Poll position: keeps the playhead, clock, and play button current.
    {
        let (player, timeline, position, play) = (player.clone(), timeline.clone(), position.clone(), play.clone());
        let total = media::fmt_time(media.duration);
        glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            player.drain_events();
            if let Some(t) = player.get_f64("time-pos") {
                timeline.set_playhead(t);
                position.set_text(&format!("{} / {total}", media::fmt_time(t)));
            }
            play.set_label(if player.paused() { "▶" } else { "⏸\u{fe0e}" });
            glib::ControlFlow::Continue
        });
    }

    // The strip backdrop renders in the background; the UI is usable first.
    {
        let (timeline, path, media) = (timeline.clone(), path.clone(), media.clone());
        let color = palette.accent.clone();
        let aspect = (WIDTH - 32) as f64 / if video { STRIP_VIDEO } else { STRIP_AUDIO } as f64;
        glib::spawn_future_local(async move {
            let png = gio::spawn_blocking(move || media::backdrop(&path, &media, &color, aspect)).await.ok().flatten();
            if let Some(png) = png {
                timeline.set_backdrop(Pixbuf::from_file(&png).ok());
                let _ = std::fs::remove_file(png);
            }
        });
    }

    let running = Rc::new(Cell::new(false));
    let run = {
        let (frame, timeline, silent, player, running) =
            (frame.clone(), timeline.clone(), silent.clone(), player.clone(), running.clone());
        let path = path.clone();
        Rc::new(move || {
            if running.replace(true) {
                return;
            }
            player.set("pause", "yes");
            let (s, e) = timeline.range();
            let out = media::output_path(&path, "-trimmed", None);
            frame.busy(true, "Trimming…");
            let (frame, running) = (frame.clone(), running.clone());
            let (src, silent) = (path.clone(), silent.is_active());
            glib::spawn_future_local(async move {
                let dst = out.clone();
                let ok = gio::spawn_blocking(move || trim(&src, &dst, s, e, video, silent)).await.unwrap_or(false);
                if ok {
                    let shown = out.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    media::notify("Trimmed", &shown);
                    frame.window.close();
                } else {
                    frame.busy(false, "Trim failed");
                    media::notify("Trim failed", "ffmpeg couldn't write the trimmed copy");
                    running.set(false);
                }
            });
        })
    };
    {
        let run = run.clone();
        frame.primary.connect_clicked(move |_| run());
    }

    // Keys: Space play/pause, , . frame step, I / O set start/end at the
    // playhead, L loops the selection, Enter applies. Esc closes (Frame).
    let keys = gtk::EventControllerKey::new();
    // Capture phase: the window sees keys before a focused button turns
    // Enter or Space into a click.
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (player, timeline, window, show_range) = (player.clone(), timeline.clone(), frame.window.clone(), show_range.clone());
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk::gdk::Key;
            if GtkWindowExt::focus(&window).is_some_and(|w| w.is::<gtk::Text>()) {
                return glib::Propagation::Proceed;
            }
            let here = || player.get_f64("time-pos").unwrap_or(0.0);
            match key {
                Key::space => {
                    player.set_loop(None);
                    player.toggle_pause();
                }
                Key::comma => player.command(&["frame-back-step"]),
                Key::period => player.command(&["frame-step"]),
                Key::i | Key::I | Key::o | Key::O => {
                    let (s, e) = timeline.range();
                    if matches!(key, Key::i | Key::I) { timeline.set_range(here(), e) } else { timeline.set_range(s, here()) }
                    let (s, e) = timeline.range();
                    show_range(s, e);
                }
                Key::l | Key::L => loop_range(),
                Key::Return | Key::KP_Enter => run(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
    }
    frame.window.add_controller(keys);

    // Drop the player with the window so mpv shuts down cleanly.
    {
        let player = std::cell::RefCell::new(Some(player));
        frame.window.connect_close_request(move |_| {
            player.borrow_mut().take();
            glib::Propagation::Proceed
        });
    }

    frame.window.present();
    // Start on the transport, so Space / I / O drive playback. Deferred:
    // focus set before the window maps is overridden by GTK.
    glib::idle_add_local_once(move || {
        play.grab_focus();
    });
}

/// A flat button showing a timecode, with a popover entry to type one.
fn time_button() -> (gtk::Button, gtk::Entry, gtk::Popover) {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    let entry = gtk::Entry::builder().width_chars(10).build();
    let pop = gtk::Popover::builder().child(&entry).build();
    pop.set_parent(&button);
    // A popover is an extra child GTK won't unparent for us.
    {
        let pop = pop.clone();
        button.connect_destroy(move |_| pop.unparent());
    }
    (button, entry, pop)
}

/// An exact cut, re-encoded from the cut point. Audio can also shed the
/// silence at either end of the kept range.
fn trim(src: &Path, dst: &Path, start: f64, end: f64, video: bool, silent_ends: bool) -> bool {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-ss", &format!("{start:.3}"), "-to", &format!("{end:.3}"), "-i"]).arg(src);
    let ext = dst.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if video {
        if ext == "webm" {
            cmd.args(["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-c:a", "libopus"]);
        } else {
            cmd.args(["-c:v", "libx264", "-crf", "18", "-preset", "medium", "-c:a", "aac", "-b:a", "192k"]);
        }
    } else if silent_ends {
        // silenceremove only trims the start, so reverse, trim, reverse.
        let cut = "silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.05";
        cmd.args(["-af", &format!("{cut},areverse,{cut},areverse")]);
    }
    cmd.arg(dst);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}
