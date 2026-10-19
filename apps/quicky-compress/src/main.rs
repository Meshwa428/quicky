//! quicky-compress: make smaller copies of one file or a selection.
//!
//!     quicky-compress FILE...
//!
//! Pick a preset, optionally shrink images and video, or aim for a target
//! size, then Compress. The encoders live in the Quicky backend
//! (`quicky tool compress`); this window only gathers the settings. Each
//! original is kept; results land next to them as `name-small.ext`.

use gtk::{gio, glib, prelude::*};
use quicky_ui::{
    Frame, Palette,
    media::{self, Kind},
};
use std::{path::PathBuf, process::Command, rc::Rc};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.compress";

const PRESETS: [(&str, &str, &str); 2] = [
    ("Balanced", "balanced", "Smaller files with little visible quality loss."),
    ("Strong", "strong", "Smallest files; quality drops more."),
];
const SIZES: [(&str, u32); 4] = [("Keep original dimensions", 100), ("75%", 75), ("50%", 50), ("25%", 25)];

fn main() -> glib::ExitCode {
    let files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        Palette::load().install();
        if files.is_empty() {
            media::notify("Nothing to compress", "Run quicky-compress with one or more files");
            return;
        }
        build(app, files.clone());
    });
    app.run_with_args::<&str>(&[])
}

fn build(app: &gtk::Application, files: Vec<PathBuf>) {
    let kinds: Vec<Kind> = files.iter().map(|f| media::kind(f)).collect();
    if kinds.iter().any(|k| *k == Kind::Other) {
        media::notify("Can't compress", "Compress works on images, video, audio, and PDFs");
        return;
    }
    let subject = match files.as_slice() {
        [one] => one.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string(),
        many => format!("{} files", many.len()),
    };
    let total: u64 = files.iter().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum();
    // Only pictures and video can be resized.
    let resizable = kinds.iter().any(|k| matches!(k, Kind::Image | Kind::Video));

    let frame = Frame::new(app, "Compress", &subject, 360, "Compress");
    let body = &frame.body;

    let original = gtk::Label::builder().label(format!("Original size: {}", media::fmt_size(total))).xalign(0.0).build();
    original.add_css_class("dim");
    body.append(&original);

    let preset_label = gtk::Label::builder().label("Compression preset").xalign(0.0).build();
    body.append(&preset_label);
    let (presets, buttons) = Frame::choices(&PRESETS.map(|(label, _, _)| label));
    body.append(&presets);
    let help = gtk::Label::builder().label(PRESETS[0].2).xalign(0.0).wrap(true).build();
    help.add_css_class("dim");
    body.append(&help);

    let size_list = gtk::StringList::new(&SIZES.map(|(label, _)| label));
    let size = gtk::DropDown::builder().model(&size_list).build();
    if resizable {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let what = if kinds.iter().all(|k| *k == Kind::Video) { "Video size" } else { "Image size" };
        let label = gtk::Label::builder().label(what).xalign(0.0).hexpand(true).build();
        row.append(&label);
        row.append(&size);
        body.append(&row);
    }

    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    // Target size: an amount, in MB or as a percentage smaller.
    let target = gtk::CheckButton::with_label("Compress to target file size");
    body.append(&target);
    let amount = gtk::Entry::builder().width_chars(6).input_purpose(gtk::InputPurpose::Number).build();
    let unit_list = gtk::StringList::new(&["MB per file", "% smaller"]);
    let unit = gtk::DropDown::builder().model(&unit_list).build();
    let target_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    target_row.append(&amount);
    target_row.append(&unit);
    target_row.set_sensitive(false);
    body.append(&target_row);

    // ---------------------------------------------------------------- wiring

    for (i, b) in buttons.iter().enumerate() {
        let help = help.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                help.set_text(PRESETS[i].2);
            }
        });
    }
    {
        let (target_row, amount, unit) = (target_row.clone(), amount.clone(), unit.clone());
        target.connect_toggled(move |t| {
            target_row.set_sensitive(t.is_active());
            if t.is_active() && amount.text().is_empty() {
                // Start from something sensible: half the size.
                unit.set_selected(1);
                amount.set_text("50");
            }
            if t.is_active() {
                amount.grab_focus();
            }
        });
    }

    let run = {
        let (frame, buttons, size, target, amount, unit) =
            (frame.clone(), buttons.clone(), size.clone(), target.clone(), amount.clone(), unit.clone());
        Rc::new(move || {
            if !frame.primary.is_sensitive() {
                return;
            }
            let preset = PRESETS[buttons.iter().position(|b| b.is_active()).unwrap_or(0)].1;
            let mut args: Vec<String> = vec!["tool".into(), "compress".into(), "--preset".into(), preset.into()];
            if resizable {
                args.extend(["--scale".into(), SIZES[size.selected() as usize].1.to_string()]);
            }
            if target.is_active() {
                let Ok(value) = amount.text().trim().parse::<f64>() else {
                    frame.status.set_text("Enter a number for the target");
                    return;
                };
                let percent = unit.selected() == 1;
                if value <= 0.0 || (percent && value >= 100.0) {
                    frame.status.set_text("That target can't be reached");
                    return;
                }
                args.extend([(if percent { "--target-pct" } else { "--target-mb" }).into(), value.to_string()]);
            }
            frame.busy(true, "Compressing…");
            let frame = frame.clone();
            let files = files.clone();
            glib::spawn_future_local(async move {
                // The backend notifies with the outcome, as every tool does.
                let ok = gio::spawn_blocking(move || {
                    Command::new(backend()).args(&args).args(&files).status().is_ok_and(|s| s.success())
                })
                .await
                .unwrap_or(false);
                if ok {
                    frame.window.close();
                } else {
                    frame.busy(false, "Some files didn't compress");
                }
            });
        })
    };
    {
        let run = run.clone();
        frame.primary.connect_clicked(move |_| run());
    }
    {
        let run = run.clone();
        amount.connect_activate(move |_| run());
    }

    frame.window.present();
}

/// The Quicky backend: passed in by the wheel, or the installed plugin's.
fn backend() -> PathBuf {
    std::env::var_os("QUICKY_BIN").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/omarchy/plugins/meshwa.oma-quicky/bin/quicky")
    })
}
