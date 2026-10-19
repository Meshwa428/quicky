//! quicky-edit: adjust a photo's light, colour, and detail, saved as a copy.
//!
//!     quicky-edit FILE
//!
//! Move the sliders and watch the preview; each slider's ↺ puts it back.
//! The original is never touched; the result lands next to it as
//! `name-edited.ext`.

mod adjust;

use adjust::Settings;
use gtk::{gdk_pixbuf::InterpType, glib, prelude::*};
use quicky_ui::{Frame, Preview, app, canvas, media};
use std::{cell::Cell, path::PathBuf, rc::Rc};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.edit";

/// Longest edge of the preview, in pixels.
const PREVIEW: i32 = 900;

type Set = fn(&mut Settings, f64);

/// Every slider: group, label, range, step, neutral value, readout, field.
const SLIDERS: [(&str, &str, (f64, f64), f64, f64, fn(f64) -> String, Set); 17] = [
    ("Light", "Exposure", (-3.0, 3.0), 0.05, 0.0, ev, |s, v| s.exposure = v),
    ("Light", "Brightness", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.brightness = v),
    ("Light", "Contrast", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.contrast = v),
    ("Light", "Highlights", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.highlights = v),
    ("Light", "Shadows", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.shadows = v),
    ("Light", "Whites", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.whites = v),
    ("Light", "Blacks", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.blacks = v),
    ("Light", "Gamma", (0.2, 3.0), 0.01, 1.0, power, |s, v| s.gamma = v),
    ("Color", "Temperature", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.temperature = v),
    ("Color", "Tint", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.tint = v),
    ("Color", "Saturation", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.saturation = v),
    ("Color", "Vibrance", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.vibrance = v),
    ("Color", "Hue", (-180.0, 180.0), 1.0, 0.0, deg, |s, v| s.hue = v),
    ("Detail", "Clarity", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.clarity = v),
    ("Detail", "Dehaze", (-100.0, 100.0), 1.0, 0.0, num, |s, v| s.dehaze = v),
    ("Detail", "Grain", (0.0, 100.0), 1.0, 0.0, num, |s, v| s.grain = v),
    ("Detail", "Noise red.", (0.0, 100.0), 1.0, 0.0, num, |s, v| s.noise = v),
];

fn ev(v: f64) -> String {
    format!("{v:+.2} EV")
}
fn num(v: f64) -> String {
    format!("{v:+.0}")
}
fn power(v: f64) -> String {
    format!("{v:.2}")
}
fn deg(v: f64) -> String {
    format!("{v:+.0}°")
}

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Edit Photo", &["image/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let Some(image) = canvas::load_image(&path) else {
        media::notify("Can't edit", &format!("{name} isn't an image Quicky can open"));
        return;
    };
    let k = (PREVIEW as f64 / image.width().max(image.height()) as f64).min(1.0);
    let small = image
        .scale_simple(((image.width() as f64 * k) as i32).max(1), ((image.height() as f64 * k) as i32).max(1), InterpType::Bilinear)
        .unwrap_or_else(|| image.clone());

    let frame = Frame::new(app, "Edit Photo", &name, 720, "Save Edited Copy");
    let reset = frame.reset_button();
    let body = &frame.body;

    let preview = Preview::new(260);
    preview.set(Some(small.clone()));
    body.append(preview.widget());

    let controls = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let scroller = gtk::ScrolledWindow::builder().child(&controls).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    body.append(&scroller);

    let mut scales = Vec::new();
    let mut group = "";
    for (g, label, range, step, neutral, fmt, _) in SLIDERS {
        if g != group {
            group = g;
            controls.append(&Frame::section(g));
        }
        let (row, scale) = Frame::slider(label, range, step, neutral, fmt);
        controls.append(&row);
        scales.push(scale);
    }

    // ---------------------------------------------------------------- wiring

    let settings = {
        let scales = scales.clone();
        move || {
            let mut s = Settings::default();
            for (scale, (.., set)) in scales.iter().zip(SLIDERS) {
                set(&mut s, scale.value());
            }
            s
        }
    };

    // Redraw at most once per frame however fast the sliders move.
    let pending = Rc::new(Cell::new(false));
    let refresh = {
        let (preview, settings, pending, small) = (preview.clone(), settings.clone(), pending.clone(), small.clone());
        move || {
            if pending.replace(true) {
                return;
            }
            let (preview, settings, pending, small) = (preview.clone(), settings.clone(), pending.clone(), small.clone());
            glib::idle_add_local_once(move || {
                pending.set(false);
                preview.set(adjust::apply(&small, &settings()));
            });
        }
    };
    for scale in &scales {
        let refresh = refresh.clone();
        scale.connect_value_changed(move |_| refresh());
    }
    {
        let scales = scales.clone();
        reset.connect_clicked(move |_| {
            for (scale, (.., neutral, _, _)) in scales.iter().zip(SLIDERS) {
                scale.set_value(neutral);
            }
        });
    }

    // The buttons stay disabled while saving, so one click saves once.
    {
        let (frame, settings) = (frame.clone(), settings.clone());
        frame.primary.clone().connect_clicked(move |_| {
            let s = settings();
            if s == Settings::default() {
                frame.status.set_text("Nothing changed yet");
                return;
            }
            let out = media::output_path(&path, "-edited", media::image_out_ext(&path));
            // Pixbufs aren't Send: hand the worker the file, not the picture.
            let src = path.clone();
            app::finish(&frame, "Saving…", "Edited", "Save failed", out, move |dst| {
                canvas::load_image(&src).and_then(|full| adjust::apply(&full, &s)).is_some_and(|p| media::save_pixbuf(&p, dst))
            });
        });
    }

    frame.window.present();
}
