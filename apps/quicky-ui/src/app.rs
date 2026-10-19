//! The start-up every editor shares: theme the process, then build the
//! window for the files on the command line, or ask for one.

use crate::theme::Palette;
use gtk::{gio, glib, prelude::*};
use std::path::PathBuf;

/// Runs a one-file editor. With no file argument it opens a file chooser
/// limited to `mimes` (`"image/*"`, `"video/*"`, ...).
pub fn run_one(app_id: &str, title: &'static str, mimes: &'static [&'static str], build: fn(&gtk::Application, PathBuf)) -> glib::ExitCode {
    let file = std::env::args().nth(1).map(PathBuf::from);
    let app = gtk::Application::builder()
        .application_id(app_id)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        Palette::load().install();
        match &file {
            Some(path) => build(app, path.clone()),
            None => choose(app, title, mimes, build),
        }
    });
    app.run_with_args::<&str>(&[])
}

/// Runs an editor that takes several files (at least `min`).
pub fn run_many(app_id: &str, min: usize, usage: &'static str, build: fn(&gtk::Application, Vec<PathBuf>)) -> glib::ExitCode {
    let files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let app = gtk::Application::builder()
        .application_id(app_id)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        Palette::load().install();
        if files.len() < min {
            crate::media::notify("Quicky", usage);
            return;
        }
        build(app, files.clone());
    });
    app.run_with_args::<&str>(&[])
}

fn choose(app: &gtk::Application, title: &str, mimes: &[&str], build: fn(&gtk::Application, PathBuf)) {
    let filter = gtk::FileFilter::new();
    for m in mimes {
        filter.add_mime_type(m);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder().title(title).filters(&filters).build();
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

/// Runs `work` off the UI thread, then reports: a notification naming the
/// output and the window closes, or the window stays with `failed` shown.
pub fn finish(frame: &crate::Frame, busy: &str, done: &'static str, failed: &'static str, out: PathBuf, work: impl FnOnce(&std::path::Path) -> bool + Send + 'static) {
    frame.busy(true, busy);
    let frame = frame.clone();
    glib::spawn_future_local(async move {
        let dst = out.clone();
        let ok = gio::spawn_blocking(move || work(&dst)).await.unwrap_or(false);
        if ok {
            let shown = out.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            crate::media::notify(done, &shown);
            frame.window.close();
        } else {
            frame.busy(false, failed);
            crate::media::notify(failed, "Quicky couldn't write the new file");
        }
    });
}
