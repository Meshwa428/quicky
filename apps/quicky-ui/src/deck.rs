//! A video deck: the picture, a timeline strip under it, and the transport
//! (◁| ▶ |▷ and the clock). The video tools that work at the playhead
//! (snapshots, split, speed, redact) build on it.

use crate::{
    Player, Timeline,
    frame::WIDTH,
    media::{self, Media},
    theme::Palette,
    timeline::Marks,
};
use gtk::{gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use std::{path::Path, rc::Rc};

#[derive(Clone)]
pub struct Deck {
    pub player: Rc<Player>,
    pub timeline: Timeline,
    /// The picture, framed.
    pub screen: gtk::Frame,
    /// ◁| ▶ |▷ and the clock; tools may append buttons to it.
    pub transport: gtk::Box,
    pub play: gtk::Button,
}

impl Deck {
    /// `strip` is the timeline height; `marks` what it edits.
    pub fn new(path: &Path, media: &Media, palette: &Palette, marks: Marks, strip: i32) -> anyhow::Result<Deck> {
        let player = Rc::new(Player::new()?);
        player.load(path.to_str().unwrap_or_default());

        let screen = gtk::Frame::new(None);
        screen.add_css_class("video");
        screen.set_child(Some(player.widget()));
        screen.set_vexpand(true);

        let timeline = Timeline::with_marks(media.duration, palette.clone(), strip, marks);

        let back = gtk::Button::builder().icon_name("media-seek-backward-symbolic").tooltip_text("Previous frame  ,").build();
        let play = gtk::Button::builder().icon_name("media-playback-start-symbolic").tooltip_text("Play / pause  Space").build();
        let fwd = gtk::Button::builder().icon_name("media-seek-forward-symbolic").tooltip_text("Next frame  .").build();
        let clock = gtk::Label::builder().hexpand(true).xalign(1.0).build();
        clock.add_css_class("dim");
        let transport = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for b in [&back, &play, &fwd] {
            b.add_css_class("flat");
            transport.append(b);
        }
        transport.append(&clock);

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
            play.connect_clicked(move |_| player.toggle_pause());
        }
        {
            let player = player.clone();
            timeline.on_seek(move |t| player.seek(t));
        }

        // Keep the playhead, clock, and play button current.
        {
            let (player, timeline, clock, play) = (player.clone(), timeline.clone(), clock.clone(), play.clone());
            let total = media::fmt_time(media.duration);
            glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
                player.drain_events();
                if let Some(t) = player.get_f64("time-pos") {
                    timeline.set_playhead(t);
                    clock.set_text(&format!("{} / {total}", media::fmt_time(t)));
                }
                crate::frame::show_playing(&play, !player.paused());
                glib::ControlFlow::Continue
            });
        }

        // The strip's filmstrip or waveform renders in the background.
        {
            let (timeline, path, media, color) = (timeline.clone(), path.to_path_buf(), media.clone(), palette.accent.clone());
            let aspect = (WIDTH - 32) as f64 / strip as f64;
            glib::spawn_future_local(async move {
                let png = gio::spawn_blocking(move || media::backdrop(&path, &media, &color, aspect)).await.ok().flatten();
                if let Some(png) = png {
                    timeline.set_backdrop(Pixbuf::from_file(&png).ok());
                    let _ = std::fs::remove_file(png);
                }
            });
        }

        Ok(Deck { player, timeline, screen, transport, play })
    }

    /// The playhead, in seconds.
    pub fn now(&self) -> f64 {
        self.player.get_f64("time-pos").unwrap_or(0.0)
    }

    /// Space plays, `,` `.` step frames; mpv shuts down with the window.
    pub fn attach(&self, window: &gtk::ApplicationWindow) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let (player, window) = (self.player.clone(), window.clone());
            keys.connect_key_pressed(move |_, key, _, _| {
                use gtk::gdk::Key;
                if GtkWindowExt::focus(&window).is_some_and(|w| w.is::<gtk::Text>()) {
                    return glib::Propagation::Proceed;
                }
                match key {
                    Key::space => player.toggle_pause(),
                    Key::comma => player.command(&["frame-back-step"]),
                    Key::period => player.command(&["frame-step"]),
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            });
        }
        window.add_controller(keys);

        let player = std::cell::RefCell::new(Some(self.player.clone()));
        window.connect_close_request(move |_| {
            if let Some(p) = player.borrow_mut().take() {
                p.set("pause", "yes");
            }
            glib::Propagation::Proceed
        });
        // Start on play, so Space drives playback.
        let play = self.play.clone();
        glib::idle_add_local_once(move || {
            play.grab_focus();
        });
    }
}
