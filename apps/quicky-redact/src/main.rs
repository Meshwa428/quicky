//! quicky-redact: hide parts of an image, saved as a copy.
//!
//!     quicky-redact FILE
//!
//! Drag boxes over whatever should go, pick how it's hidden (solid,
//! pixelated, or blurred), then save. The original is never touched; the
//! result lands next to it as `name-redacted.ext`.

use gtk::{gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{
    Canvas, Frame, Palette,
    canvas::{self, Mode, Rect},
    media,
};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.redact";

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Solid,
    Pixelate,
    Blur,
}

const STYLES: [(&str, Style); 3] = [("Solid", Style::Solid), ("Pixelate", Style::Pixelate), ("Blur", Style::Blur)];

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
    filter.add_mime_type("image/*");
    filter.set_name(Some("Images"));
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder().title("Redact").filters(&filters).build();
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
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let Some(image) = canvas::load_image(&path) else {
        media::notify("Can't redact", &format!("{name} isn't an image Quicky can open"));
        return;
    };
    let (iw, ih) = (image.width(), image.height());
    // Block and blur sizes scale with the image, so the effect looks the
    // same on a thumbnail and on a 12-megapixel photo.
    let strength = (iw.max(ih) / 60).max(8);

    let frame = Frame::new(app, "Redact Photo", &name, 640, "Save Redacted Copy");
    let reset = frame.reset_button();
    let body = &frame.body;

    let canvas = Canvas::new(image.clone(), palette, Mode::Many);
    body.append(canvas.widget());

    let (caption, count) = Frame::caption("Drag over what to hide · Del removes");
    body.append(&caption);
    let (styles, buttons) = Frame::choices(&STYLES.map(|(label, _)| label));
    body.append(&styles);

    // ---------------------------------------------------------------- wiring

    let show_count = {
        let (canvas, count, apply, reset) = (canvas.clone(), count.clone(), frame.primary.clone(), reset.clone());
        move || {
            let n = canvas.boxes().len();
            count.set_text(&match n {
                0 => String::new(),
                1 => "1 area".into(),
                n => format!("{n} areas"),
            });
            apply.set_sensitive(n > 0);
            reset.set_sensitive(n > 0);
        }
    };
    show_count();
    {
        let show_count = show_count.clone();
        canvas.on_change(move || show_count());
    }

    let style = {
        let buttons = buttons.clone();
        move || STYLES[buttons.iter().position(|b| b.is_active()).unwrap_or(0)].1
    };
    // The canvas previews the chosen style inside the boxes.
    for b in &buttons {
        let (canvas, image, style) = (canvas.clone(), image.clone(), style.clone());
        b.connect_toggled(move |b| {
            if b.is_active() {
                canvas.set_cover(cover(&image, style(), strength));
            }
        });
    }
    {
        let canvas = canvas.clone();
        reset.connect_clicked(move |_| canvas.reset());
    }

    let running = Rc::new(Cell::new(false));
    let run = {
        let (frame, canvas, running) = (frame.clone(), canvas.clone(), running.clone());
        let (path, style) = (path.clone(), style.clone());
        Rc::new(move || {
            if !frame.primary.is_sensitive() || running.replace(true) {
                return;
            }
            let boxes = canvas.boxes();
            let out = media::output_path(&path, "-redacted", output_ext(&path));
            frame.busy(true, "Redacting…");
            let (frame, running) = (frame.clone(), running.clone());
            let (src, style) = (path.clone(), style());
            glib::spawn_future_local(async move {
                let dst = out.clone();
                let ok = gio::spawn_blocking(move || redact(&src, &dst, &boxes, style, strength, (iw, ih)))
                    .await
                    .unwrap_or(false);
                if ok {
                    let shown = out.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    media::notify("Redacted", &shown);
                    frame.window.close();
                } else {
                    frame.busy(false, "Redact failed");
                    media::notify("Redact failed", "ImageMagick couldn't write the redacted copy");
                    running.set(false);
                }
            });
        })
    };
    {
        let run = run.clone();
        frame.primary.connect_clicked(move |_| run());
    }

    // Keys: Delete / Backspace remove the selected box, Enter saves. Esc
    // closes (Frame).
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let canvas = canvas.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            use gtk::gdk::Key;
            match key {
                Key::Delete | Key::BackSpace | Key::KP_Delete => {
                    canvas.remove_selected();
                }
                Key::Return | Key::KP_Enter => run(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
    }
    frame.window.add_controller(keys);

    frame.window.present();
}

fn cover(image: &Pixbuf, style: Style, strength: i32) -> Option<Pixbuf> {
    match style {
        Style::Solid => None,
        Style::Pixelate => canvas::pixelated(image, strength),
        Style::Blur => canvas::blurred(image, strength),
    }
}

/// Vector images come out as PNG: redacting rasterizes them anyway.
fn output_ext(src: &Path) -> Option<&'static str> {
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    (ext == "svg").then_some("png")
}

/// One pass for every box: the hidden version of the whole image is laid
/// over the original through a mask that is white only inside the boxes.
fn redact(src: &Path, dst: &Path, boxes: &[Rect], style: Style, strength: i32, (iw, ih): (i32, i32)) -> bool {
    let hidden: Vec<String> = match style {
        Style::Solid => vec!["-fill".into(), "black".into(), "-colorize".into(), "100".into()],
        Style::Pixelate => vec![
            "-sample".into(),
            format!("{}x{}!", (iw / strength).max(1), (ih / strength).max(1)),
            "-sample".into(),
            format!("{iw}x{ih}!"),
        ],
        Style::Blur => vec!["-blur".into(), format!("0x{strength}")],
    };
    let mut cmd = Command::new("magick");
    cmd.arg(src).arg("-auto-orient");
    cmd.args(["(", "-clone", "0"]).args(&hidden).arg(")");
    // The mask must be a plain grayscale canvas: a colour one (say, a clone
    // of the photo) is ignored and the whole image gets hidden.
    cmd.args(["(", "-size", &format!("{iw}x{ih}"), "xc:black", "-fill", "white"]);
    for b in boxes {
        let (x, y, w, h) = b.pixels(iw, ih);
        cmd.args(["-draw", &format!("rectangle {x},{y} {},{}", x + w - 1, y + h - 1)]);
    }
    cmd.args(["-alpha", "off", "-colorspace", "gray", ")", "-composite"]).arg(dst);
    cmd.status().is_ok_and(|s| s.success())
}
