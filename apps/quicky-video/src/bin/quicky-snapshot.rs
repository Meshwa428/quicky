//! quicky-snapshot: save chosen frames of a video as PNGs.
//!
//!     quicky-snapshot FILE
//!
//! Scrub or step to a frame and Add Frame; repeat for as many as you like,
//! then save them all. Each lands next to the video as
//! `name-frame-0m01.50s.png`.

use gtk::{gdk, gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{Deck, Frame, Palette, app, media, timeline::Marks};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.snapshot";

/// Tile thumbnail height.
const TILE: i32 = 54;

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Video Snapshots", &["video/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if !m.has_video || m.duration <= 0.0 {
        media::notify("Can't take snapshots", &format!("{name} has no video"));
        return;
    }
    let deck = match Deck::new(&path, &m, &palette, Marks::Points, 40) {
        Ok(d) => d,
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };

    let frame = Frame::new(app, "Video Snapshots", &name, 680, "Save Frames");
    let body = &frame.body;
    body.append(&deck.screen);
    body.append(deck.timeline.widget());
    let add = gtk::Button::with_label("Add Frame");
    deck.transport.append(&add);
    body.append(&deck.transport);

    let (caption, count) = Frame::caption("Frames to save");
    body.append(&caption);
    let tiles = gtk::FlowBox::builder().selection_mode(gtk::SelectionMode::None).max_children_per_line(4).min_children_per_line(4).row_spacing(6).column_spacing(6).build();
    let scroller = gtk::ScrolledWindow::builder().child(&tiles).min_content_height(TILE + 12).max_content_height(2 * TILE + 24).propagate_natural_height(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    body.append(&scroller);

    let times: Rc<RefCell<Vec<f64>>> = Rc::default();

    // Count, markers on the strip, and the button follow the list.
    let refresh: Rc<dyn Fn()> = {
        let (times, count, timeline, apply) = (times.clone(), count.clone(), deck.timeline.clone(), frame.primary.clone());
        Rc::new(move || {
            let t = times.borrow();
            count.set_text(&format!("{} selected", t.len()));
            timeline.set_markers(t.clone());
            apply.set_sensitive(!t.is_empty());
            apply.set_label(&match t.len() {
                1 => "Save 1 Frame".to_string(),
                n => format!("Save {n} Frames"),
            });
        })
    };
    refresh();

    {
        let (deck, times, tiles, refresh, path) = (deck.clone(), times.clone(), tiles.clone(), refresh.clone(), path.clone());
        add.connect_clicked(move |_| {
            let t = deck.now();
            if times.borrow().iter().any(|x| (x - t).abs() < 0.01) {
                return;
            }
            times.borrow_mut().push(t);
            refresh();

            // A tile: the frame, its time, and × to drop it.
            let picture = gtk::Picture::builder().can_shrink(true).content_fit(gtk::ContentFit::Cover).height_request(TILE).width_request(TILE * 16 / 9).build();
            let badge = gtk::Label::builder().label(media::fmt_time(t)).halign(gtk::Align::Start).valign(gtk::Align::End).build();
            badge.add_css_class("badge");
            let remove = gtk::Button::builder().label("×").halign(gtk::Align::End).valign(gtk::Align::Start).build();
            remove.add_css_class("compact");
            let overlay = gtk::Overlay::builder().child(&picture).build();
            overlay.add_overlay(&badge);
            overlay.add_overlay(&remove);
            overlay.add_css_class("tile");
            tiles.append(&overlay);
            {
                let (times, tiles, refresh, overlay) = (times.clone(), tiles.clone(), refresh.clone(), overlay.clone());
                remove.connect_clicked(move |_| {
                    times.borrow_mut().retain(|x| (x - t).abs() >= 0.01);
                    if let Some(child) = overlay.parent() {
                        tiles.remove(&child);
                    }
                    refresh();
                });
            }
            let src = path.clone();
            glib::spawn_future_local(async move {
                let png = gio::spawn_blocking(move || media::grab_frame(&src, t, Some(TILE * 2))).await.ok().flatten();
                if let Some(png) = png {
                    if let Ok(p) = Pixbuf::from_file(&png) {
                        picture.set_paintable(Some(&gdk::Texture::for_pixbuf(&p)));
                    }
                    let _ = std::fs::remove_file(png);
                }
            });
        });
    }

    {
        let (frame, times, player) = (frame.clone(), times.clone(), deck.player.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let mut ts = times.borrow().clone();
            ts.sort_by(f64::total_cmp);
            let n = ts.len();
            // Name the first file; the notification says how many.
            let first = frame_path(&path, ts[0]);
            let src = path.clone();
            let title = if n == 1 { "Saved 1 frame" } else { "Saved frames" };
            app::finish(&frame, "Saving…", title, "Save failed", first, move |_| {
                ts.iter().all(|&t| {
                    let out = frame_path(&src, t);
                    media::run(std::process::Command::new("ffmpeg").args(["-v", "error", "-ss", &format!("{t:.3}"), "-i"]).arg(&src).args(["-frames:v", "1"]).arg(&out))
                })
            });
        });
    }

    deck.attach(&frame.window);
    frame.window.present();
}

/// `name-frame-0m01.50s.png` next to the video, numbered if taken.
fn frame_path(src: &Path, t: f64) -> PathBuf {
    let m = (t / 60.0).floor();
    media::output_path(src, &format!("-frame-{}m{:05.2}s", m, t - m * 60.0), Some("png"))
}
