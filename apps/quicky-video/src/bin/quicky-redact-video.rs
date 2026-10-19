//! quicky-redact-video: hide parts of a video for as long as needed, saved
//! as a copy.
//!
//!     quicky-redact-video FILE
//!
//! Drag boxes over the picture; each box gets a track under the video
//! whose ends set when it shows. Pick Solid, Pixelate, or Blur. The result
//! lands next to the original as `name-redacted.ext`.

use gtk::{
    gdk_pixbuf::{Colorspace, Pixbuf},
    glib,
    prelude::*,
};
use quicky_ui::{
    Canvas, Deck, Frame, Palette, Timeline, app,
    canvas::{Mode, PAD, Rect},
    media,
    timeline::Marks,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.redact-video";

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Solid,
    Pixelate,
    Blur,
}

const STYLES: [(&str, Style); 3] = [("Solid", Style::Solid), ("Pixelate", Style::Pixelate), ("Blur", Style::Blur)];

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Redact Video", &["video/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let m = media::probe(&path);
    if !m.has_video || m.width == 0 || m.duration <= 0.0 {
        media::notify("Can't redact", &format!("{name} has no video"));
        return;
    }
    let deck = match Deck::new(&path, &m, &palette, Marks::Points, 36) {
        Ok(d) => d,
        Err(e) => {
            media::notify("Can't start the player", &e.to_string());
            return;
        }
    };
    let (vw, vh) = (m.width as i32, m.height as i32);

    // Boxes are drawn on a clear layer the size of the video, laid exactly
    // over the picture: both fit the same area, inset by the canvas padding.
    let Some(layer) = Pixbuf::new(Colorspace::Rgb, true, 8, vw, vh) else { return };
    layer.fill(0);
    let canvas = Canvas::new(layer, palette.clone(), Mode::Many);
    let video = deck.player.widget();
    deck.screen.set_child(None::<&gtk::Widget>);
    for side in 0..4 {
        let m = PAD as i32;
        match side {
            0 => video.set_margin_top(m),
            1 => video.set_margin_bottom(m),
            2 => video.set_margin_start(m),
            _ => video.set_margin_end(m),
        }
    }
    let overlay = gtk::Overlay::builder().child(video).build();
    overlay.add_overlay(canvas.widget());
    deck.screen.set_child(Some(&overlay));

    let frame = Frame::new(app, "Redact Video", &name, 720, "Save Redacted Copy");
    let reset = frame.reset_button();
    let body = &frame.body;
    body.append(&deck.screen);
    body.append(deck.timeline.widget());
    body.append(&deck.transport);

    let (styles, style_buttons) = Frame::choices(&STYLES.map(|(l, _)| l));
    let color = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    color.set_rgba(&gtk::gdk::RGBA::BLACK);
    let style_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    style_row.append(&styles);
    style_row.append(&color);
    styles.set_hexpand(true);
    body.append(&style_row);

    let (caption, count) = Frame::caption("Redactions · drag over the video to add");
    body.append(&caption);
    let tracks = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let scroller = gtk::ScrolledWindow::builder().child(&tracks).min_content_height(60).vexpand(false).hscrollbar_policy(gtk::PolicyType::Never).build();
    body.append(&scroller);

    // Each box's time range, in the canvas's box order.
    let ranges: Rc<RefCell<Vec<(f64, f64)>>> = Rc::default();
    let duration = m.duration;

    // One track per box: its number and a range strip.
    let rebuild: Rc<dyn Fn()> = {
        let (tracks, ranges, canvas, count, apply, palette) = (tracks.clone(), ranges.clone(), canvas.clone(), count.clone(), frame.primary.clone(), palette.clone());
        Rc::new(move || {
            while let Some(c) = tracks.first_child() {
                tracks.remove(&c);
            }
            let n = ranges.borrow().len();
            for i in 0..n {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let index = gtk::Label::builder().label((i + 1).to_string()).width_chars(2).build();
                index.add_css_class("dim");
                row.append(&index);
                let strip = Timeline::new(duration, palette.clone(), 18);
                let (a, b) = ranges.borrow()[i];
                strip.set_range(a, b);
                {
                    let (ranges, canvas) = (ranges.clone(), canvas.clone());
                    strip.on_range(move |a, b, _| {
                        if let Some(r) = ranges.borrow_mut().get_mut(i) {
                            *r = (a, b);
                        }
                        canvas.select(Some(i));
                    });
                }
                row.append(strip.widget());
                tracks.append(&row);
            }
            count.set_text(&match n {
                0 => String::new(),
                1 => "1 box".into(),
                n => format!("{n} boxes"),
            });
            apply.set_sensitive(n > 0);
        })
    };
    rebuild();

    // A new box shows for the whole video until its track is trimmed.
    {
        let (canvas2, ranges, rebuild) = (canvas.clone(), ranges.clone(), rebuild.clone());
        canvas.on_change(move || {
            let n = canvas2.boxes().len();
            let grew = n > ranges.borrow().len();
            if grew {
                ranges.borrow_mut().resize(n, (0.0, duration));
                rebuild();
            }
        });
    }
    let remove = {
        let (canvas, ranges, rebuild) = (canvas.clone(), ranges.clone(), rebuild.clone());
        move || {
            let i = canvas.selected();
            if canvas.remove_selected()
                && let Some(i) = i
            {
                ranges.borrow_mut().remove(i);
                rebuild();
            }
        }
    };
    {
        let (canvas, ranges, rebuild) = (canvas.clone(), ranges.clone(), rebuild.clone());
        reset.connect_clicked(move |_| {
            canvas.reset();
            ranges.borrow_mut().clear();
            rebuild();
        });
    }

    let style = {
        let buttons = style_buttons.clone();
        move || STYLES[buttons.iter().position(|b| b.is_active()).unwrap_or(0)].1
    };
    // Solid boxes preview in their colour; blur and pixelate as a veil
    // (they can't be previewed on moving video).
    let cover = {
        let (canvas, color, style) = (canvas.clone(), color.clone(), style.clone());
        move || {
            let c = color.rgba();
            let (rgb, alpha) = match style() {
                Style::Solid => ((c.red(), c.green(), c.blue()), 255u8),
                _ => ((0.55, 0.55, 0.6), 200u8),
            };
            let pixel = |v: f32| (v * 255.0) as u32;
            if let Some(p) = Pixbuf::new(Colorspace::Rgb, true, 8, vw, vh) {
                p.fill(pixel(rgb.0) << 24 | pixel(rgb.1) << 16 | pixel(rgb.2) << 8 | alpha as u32);
                canvas.set_cover(Some(p));
            }
        }
    };
    cover();
    for b in &style_buttons {
        let cover = cover.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                cover();
            }
        });
    }
    {
        let cover = cover.clone();
        color.connect_rgba_notify(move |_| cover());
    }

    // Boxes show only inside their time range.
    {
        let (canvas, ranges, deck2) = (canvas.clone(), ranges.clone(), deck.clone());
        glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            let t = deck2.now();
            canvas.set_visible(ranges.borrow().iter().map(|&(a, b)| t >= a && t <= b).collect());
            glib::ControlFlow::Continue
        });
    }

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(move |_, key, _, _| {
        use gtk::gdk::Key;
        if matches!(key, Key::Delete | Key::BackSpace | Key::KP_Delete) {
            remove();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    frame.window.add_controller(keys);

    {
        let (frame, canvas, ranges, player, color) = (frame.clone(), canvas.clone(), ranges.clone(), deck.player.clone(), color.clone());
        frame.primary.clone().connect_clicked(move |_| {
            player.set("pause", "yes");
            let boxes: Vec<(Rect, (f64, f64))> = canvas.boxes().into_iter().zip(ranges.borrow().iter().copied()).collect();
            let c = color.rgba();
            let hex = format!("0x{:02x}{:02x}{:02x}", (c.red() * 255.0) as u8, (c.green() * 255.0) as u8, (c.blue() * 255.0) as u8);
            let (style, out, src) = (style(), media::output_path(&path, "-redacted", None), path.clone());
            app::finish(&frame, "Redacting… (re-encoding)", "Redacted", "Redact failed", out, move |dst| redact(&src, dst, &boxes, style, &hex, (vw, vh)));
        });
    }

    deck.attach(&frame.window);
    frame.window.present();
}

/// One ffmpeg pass. Solid boxes are drawn; blurred and pixelated ones are
/// cut out, softened, and laid back on top while their range lasts.
fn redact(src: &Path, dst: &Path, boxes: &[(Rect, (f64, f64))], style: Style, color: &str, (vw, vh): (i32, i32)) -> bool {
    let when = |(a, b): (f64, f64)| format!("enable='between(t,{a:.3},{b:.3})'");
    let mut graph = String::new();
    match style {
        Style::Solid => {
            let draws: Vec<String> = boxes
                .iter()
                .map(|(r, span)| {
                    let (x, y, w, h) = r.pixels(vw, vh);
                    format!("drawbox=x={x}:y={y}:w={w}:h={h}:color={color}@1:t=fill:{}", when(*span))
                })
                .collect();
            graph = format!("[0:v]{}[v]", draws.join(","));
        }
        _ => {
            let n = boxes.len();
            graph += &format!("[0:v]split={}[base]", n + 1);
            for i in 0..n {
                graph += &format!("[c{i}]");
            }
            graph += ";";
            let strength = (vw.max(vh) / 60).max(8);
            let mut last = "base".to_string();
            for (i, (r, span)) in boxes.iter().enumerate() {
                let (x, y, w, h) = r.pixels(vw, vh);
                let soften = match style {
                    Style::Blur => format!("boxblur={}:2", (strength).min(w.min(h) / 2 - 1).max(1)),
                    _ => format!("scale=max(1\\,iw/{strength}):max(1\\,ih/{strength}),scale={w}:{h}:flags=neighbor"),
                };
                graph += &format!("[c{i}]crop={w}:{h}:{x}:{y},{soften}[s{i}];[{last}][s{i}]overlay={x}:{y}:{}[o{i}];", when(*span));
                last = format!("o{i}");
            }
            graph += &format!("[{last}]null[v]");
        }
    }
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-i"]).arg(src);
    cmd.args(["-filter_complex", &graph, "-map", "[v]", "-map", "0:a?", "-c:v", "libx264", "-crf", "18", "-preset", "medium", "-c:a", "copy"]);
    media::run(cmd.arg(dst))
}
