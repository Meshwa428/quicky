//! quicky-metadata: inspect, change, or remove the information embedded in a
//! file, saved as a copy.
//!
//!     quicky-metadata FILE
//!
//! Reading and writing go through ExifTool, which understands photos, video,
//! audio, PDFs, and Office files alike. The original is never touched: Save
//! writes `name-edited.ext`, Remove All writes `name-clean.ext`.

use gtk::{gio, glib, prelude::*};
use quicky_ui::{Frame, Palette, media};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.metadata";

/// Groups ExifTool computes or reads from the filesystem; they can't be written.
const READ_ONLY_GROUPS: &[&str] = &["System", "File", "Composite", "ExifTool"];

/// One metadata field and its row in the list.
struct Field {
    group: String,
    tag: String,
    original: String,
    editable: bool,
    row: gtk::Box,
    entry: Option<gtk::Entry>,
    // Shared with the row's remove button.
    removed: Rc<Cell<bool>>,
    haystack: String,
}

impl Field {
    fn current(&self) -> String {
        self.entry.as_ref().map(|e| e.text().to_string()).unwrap_or_else(|| self.original.clone())
    }

    /// ExifTool arguments this field contributes on Save, if any.
    fn arg(&self) -> Option<String> {
        let name = format!("-{}:{}", self.group, self.tag);
        if self.removed.get() {
            Some(format!("{name}="))
        } else if self.editable && self.current() != self.original {
            Some(format!("{name}={}", self.current()))
        } else {
            None
        }
    }
}

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
    let dialog = gtk::FileDialog::builder().title("Metadata").build();
    let app = app.clone();
    let hold = app.hold();
    dialog.open(None::<&gtk::Window>, None::<&gio::Cancellable>, move |res| {
        drop(hold);
        if let Some(path) = res.ok().and_then(|f| f.path()) {
            build(&app, path);
        }
    });
}

fn build(app: &gtk::Application, path: PathBuf) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    if Command::new("exiftool").arg("-ver").output().is_err() {
        media::notify("ExifTool isn't installed", "sudo pacman -S perl-image-exiftool");
        return;
    }
    let tags = match read_tags(&path) {
        Ok(t) => t,
        Err(e) => {
            media::notify("Can't read metadata", &e.to_string());
            return;
        }
    };

    let frame = Frame::new(app, "Metadata", &name, 620, "Save");
    let remove_all = frame.secondary_button("Remove All");
    remove_all.add_css_class("destructive-action");
    let root = &frame.body;

    let search = gtk::SearchEntry::builder().placeholder_text("Search metadata").build();
    root.append(&search);

    // Body: one section per group.
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label_group = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
    let fields: Rc<RefCell<Vec<Rc<Field>>>> = Rc::default();
    let mut sections: Vec<(gtk::Widget, Vec<Rc<Field>>)> = Vec::new();

    for (group, entries) in tags {
        let heading = gtk::Label::builder().label(&group).xalign(0.0).margin_top(10).build();
        heading.add_css_class("title");
        heading.add_css_class("dim");
        list.append(&heading);
        let mut members = Vec::new();
        for (tag, value) in entries {
            let field = Rc::new(make_field(&group, &tag, &value, &label_group));
            list.append(&field.row);
            fields.borrow_mut().push(field.clone());
            members.push(field);
        }
        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
        sep.set_margin_top(6);
        list.append(&sep);
        sections.push((heading.upcast(), members));
    }

    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    root.append(&scroller);

    let note = gtk::Label::builder()
        .label("For photos, Remove All also clears hidden camera and processing data; the colour profile is kept. HDR and portrait effects may change.")
        .wrap(true)
        .xalign(0.0)
        .build();
    note.add_css_class("dim");
    root.append(&note);

    // Search hides non-matching rows, and headings left with nothing visible.
    {
        let sections: Vec<(gtk::Widget, Vec<Rc<Field>>)> = sections;
        search.connect_search_changed(move |s| {
            let needle = s.text().to_lowercase();
            for (heading, members) in &sections {
                let mut any = false;
                for f in members {
                    let show = needle.is_empty() || f.haystack.contains(&needle);
                    f.row.set_visible(show);
                    any |= show;
                }
                heading.set_visible(any);
            }
        });
    }

    let busy = {
        let frame = frame.clone();
        move |on: bool, text: &str| frame.busy(on, text)
    };
    let window = frame.window.clone();

    {
        let (fields, window, path, busy) = (fields.clone(), window.clone(), path.clone(), busy.clone());
        frame.primary.connect_clicked(move |_| {
            let args: Vec<String> = fields.borrow().iter().filter_map(|f| f.arg()).collect();
            if args.is_empty() {
                busy(false, "Nothing changed");
                return;
            }
            let out = media::output_path(&path, "-edited", None);
            run_exiftool(&window, &path, out, args, "Saved", busy.clone());
        });
    }
    {
        let (window, path, busy) = (window.clone(), path.clone(), busy.clone());
        remove_all.connect_clicked(move |_| {
            let out = media::output_path(&path, "-clean", None);
            // Strip everything, then copy the colour profile back so photos
            // don't shift colour.
            let args = ["-all=", "-tagsFromFile", "@", "-icc_profile"].map(String::from).to_vec();
            run_exiftool(&window, &path, out, args, "Metadata removed", busy.clone());
        });
    }

    window.present();
}

/// Builds one row: label, value (an entry when writable), and a remove toggle.
fn make_field(group: &str, tag: &str, value: &str, label_group: &gtk::SizeGroup) -> Field {
    let pretty = humanize(tag);
    let binary = value.starts_with("(Binary data");
    let editable = !READ_ONLY_GROUPS.contains(&group) && !binary;

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::builder().label(format!("{pretty}:")).xalign(1.0).build();
    label_group.add_widget(&label);
    row.append(&label);

    let entry = if editable {
        let e = gtk::Entry::builder().text(value).hexpand(true).build();
        row.append(&e);
        Some(e)
    } else {
        let l = gtk::Label::builder()
            .label(value)
            .xalign(0.0)
            .hexpand(true)
            .selectable(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build();
        row.append(&l);
        None
    };

    let remove = gtk::Button::builder().icon_name("list-remove-symbolic").tooltip_text("Remove this field").build();
    remove.add_css_class("flat");
    remove.set_sensitive(editable);
    row.append(&remove);

    let removed = Rc::new(Cell::new(false));
    // Toggling removal greys the row; ↺ brings it back before saving.
    {
        let (row, entry, removed) = (row.clone(), entry.clone(), removed.clone());
        remove.connect_clicked(move |b| {
            let now = !removed.get();
            removed.set(now);
            row.set_opacity(if now { 0.4 } else { 1.0 });
            if let Some(e) = &entry {
                e.set_sensitive(!now);
            }
            b.set_icon_name(if now { "edit-undo-symbolic" } else { "list-remove-symbolic" });
            b.set_tooltip_text(Some(if now { "Keep this field" } else { "Remove this field" }));
        });
    }

    Field {
        group: group.into(),
        tag: tag.into(),
        original: value.into(),
        editable,
        row,
        entry,
        removed,
        haystack: format!("{pretty} {tag} {value}").to_lowercase(),
    }
}

/// Groups of (tag, value), in ExifTool's order.
fn read_tags(path: &Path) -> anyhow::Result<Vec<(String, Vec<(String, String)>)>> {
    let out = Command::new("exiftool").args(["-j", "-G1", "-a", "-struct"]).arg(path).output()?;
    anyhow::ensure!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr).trim());
    let json: Value = serde_json::from_slice(&out.stdout)?;
    let object = json.get(0).and_then(Value::as_object).ok_or_else(|| anyhow::anyhow!("no metadata"))?;

    let mut groups: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for (key, value) in object {
        let Some((group, tag)) = key.split_once(':') else { continue };
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Array(items) => items.iter().map(plain).collect::<Vec<_>>().join(", "),
            other => plain(other),
        };
        match groups.iter_mut().find(|(g, _)| g == group) {
            Some((_, entries)) => entries.push((tag.into(), text)),
            None => groups.push((group.into(), vec![(tag.into(), text)])),
        }
    }
    Ok(groups)
}

fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `ExifImageHeight` → `Exif Image Height`, `ICCProfileName` → `ICC Profile Name`.
fn humanize(tag: &str) -> String {
    let chars: Vec<char> = tag.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

/// Writes `out` from `src` with `args`, off the UI thread, then reports.
fn run_exiftool(
    window: &gtk::ApplicationWindow,
    src: &Path,
    out: PathBuf,
    args: Vec<String>,
    done: &'static str,
    busy: impl Fn(bool, &str) + 'static,
) {
    busy(true, "Writing…");
    let (src, window) = (src.to_path_buf(), window.clone());
    glib::spawn_future_local(async move {
        let dst = out.clone();
        let result = gio::spawn_blocking(move || {
            Command::new("exiftool")
                .args(["-q", "-q", "-m", "-o"])
                .arg(&dst)
                .args(&args)
                .arg(&src)
                .output()
        })
        .await;
        match result {
            Ok(Ok(o)) if o.status.success() && out.exists() => {
                let shown = out.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                media::notify(done, &shown);
                window.close();
            }
            Ok(Ok(o)) => {
                let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                busy(false, "Couldn't write — see notification");
                media::notify("Metadata not saved", if err.is_empty() { "ExifTool failed" } else { &err });
            }
            _ => {
                busy(false, "Couldn't run ExifTool");
            }
        }
    });
}
