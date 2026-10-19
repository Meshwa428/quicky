//! quicky-organize: reorder and rotate a PDF's pages, saved as a copy.
//!
//!     quicky-organize FILE
//!
//! Drag a page to move it, ↻ turns it a quarter. Reset Order puts every
//! page back. "Make all pages the same width" scales pages to the widest.
//! The result lands next to the original as `name-organized.pdf`.

use gtk::{gdk, gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{Frame, app, media};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.organize";

/// Thumbnail height.
const THUMB: i32 = 112;

/// A page in the new document: its number in the original (1-based) and
/// how far it is turned, in quarter turns clockwise.
#[derive(Clone, Copy, PartialEq)]
struct Page {
    source: usize,
    turns: u8,
}

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Organize PDF", &["application/pdf"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let sizes = page_sizes(&path);
    if sizes.is_empty() {
        media::notify("Can't organize", &format!("{name} isn't a PDF Quicky can read"));
        return;
    }
    let original: Vec<Page> = (1..=sizes.len()).map(|source| Page { source, turns: 0 }).collect();
    let pages = Rc::new(RefCell::new(original.clone()));
    let thumbs: Rc<RefCell<BTreeMap<usize, Pixbuf>>> = Rc::default();

    let frame = Frame::new(app, "Organize PDF", &name, 640, "Save Organized PDF");
    let body = &frame.body;
    let (caption, count) = Frame::caption("Drag a page to move it");
    let reorder = gtk::Button::with_label("Reset Order");
    caption.append(&reorder);
    body.append(&caption);

    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(3)
        .max_children_per_line(3)
        .row_spacing(8)
        .column_spacing(8)
        .valign(gtk::Align::Start)
        .build();
    let scroller = gtk::ScrolledWindow::builder().child(&grid).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    body.append(&scroller);
    let same_width = gtk::CheckButton::with_label("Make all pages the same width");
    body.append(&same_width);

    // The grid is rebuilt after every change; documents here are small.
    let slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::default();
    let render: Rc<dyn Fn()> = {
        let (grid, pages, thumbs, count, slot) = (grid.clone(), pages.clone(), thumbs.clone(), count.clone(), slot.clone());
        Rc::new(move || {
            while let Some(c) = grid.first_child() {
                grid.remove(&c);
            }
            let list = pages.borrow().clone();
            count.set_text(&format!("{} pages", list.len()));
            let again = || slot.borrow().clone();
            for (i, page) in list.iter().enumerate() {
                let picture = gtk::Picture::builder().can_shrink(true).content_fit(gtk::ContentFit::Contain).height_request(THUMB).build();
                if let Some(p) = thumbs.borrow().get(&page.source) {
                    let turned = (0..page.turns).fold(p.clone(), |p, _| p.rotate_simple(gtk::gdk_pixbuf::PixbufRotation::Clockwise).unwrap_or(p));
                    picture.set_paintable(Some(&gdk::Texture::for_pixbuf(&turned)));
                }
                let badge = gtk::Label::builder().label((i + 1).to_string()).halign(gtk::Align::Start).valign(gtk::Align::End).build();
                badge.add_css_class("badge");
                let rotate = gtk::Button::builder().label("\u{f0465}").tooltip_text("Rotate").halign(gtk::Align::End).valign(gtk::Align::End).build();
                rotate.add_css_class("compact");
                let tile = gtk::Overlay::builder().child(&picture).build();
                tile.add_overlay(&badge);
                tile.add_overlay(&rotate);
                tile.add_css_class("tile");

                {
                    let (pages, again) = (pages.clone(), again());
                    rotate.connect_clicked(move |_| {
                        let t = &mut pages.borrow_mut()[i].turns;
                        *t = (*t + 1) % 4;
                        if let Some(f) = again.clone() {
                            glib::idle_add_local_once(move || f());
                        }
                    });
                }

                // Drag a tile onto another to move it there.
                let source = gtk::DragSource::builder().actions(gdk::DragAction::MOVE).build();
                source.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&(i as u32).to_value())));
                {
                    let picture = picture.clone();
                    source.connect_drag_begin(move |s, _| {
                        if let Some(p) = picture.paintable() {
                            s.set_icon(Some(&p), 20, 20);
                        }
                    });
                }
                tile.add_controller(source);
                let target = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
                {
                    let (pages, again) = (pages.clone(), again());
                    target.connect_drop(move |_, value, _, _| {
                        let Ok(from) = value.get::<u32>() else { return false };
                        let from = from as usize;
                        if from == i {
                            return false;
                        }
                        let mut p = pages.borrow_mut();
                        let moved = p.remove(from);
                        p.insert(i, moved);
                        drop(p);
                        if let Some(f) = again.clone() {
                            glib::idle_add_local_once(move || f());
                        }
                        true
                    });
                }
                tile.add_controller(target);
                grid.append(&tile);
            }
        })
    };
    *slot.borrow_mut() = Some(render.clone());
    render();

    // Thumbnails render in the background, then the grid fills in.
    {
        let (thumbs, render, src) = (thumbs.clone(), render.clone(), path.clone());
        glib::spawn_future_local(async move {
            let made = gio::spawn_blocking(move || thumbnails(&src)).await.unwrap_or_default();
            for (n, png) in made {
                if let Ok(p) = Pixbuf::from_file(&png) {
                    thumbs.borrow_mut().insert(n, p);
                }
                let _ = std::fs::remove_file(png);
            }
            render();
        });
    }
    {
        let (pages, render) = (pages.clone(), render.clone());
        reorder.connect_clicked(move |_| {
            *pages.borrow_mut() = original.clone();
            render();
        });
    }

    {
        let (frame, pages, same_width) = (frame.clone(), pages.clone(), same_width.clone());
        frame.primary.clone().connect_clicked(move |_| {
            let list = pages.borrow().clone();
            let (src, sizes, even) = (path.clone(), sizes.clone(), same_width.is_active());
            let out = media::output_path(&path, "-organized", Some("pdf"));
            app::finish(&frame, "Saving…", "Organized", "Save failed", out, move |dst| organize(&src, dst, &list, &sizes, even));
        });
    }

    frame.window.present();
}

/// Each page's width and height in points, already turned by any rotation
/// the PDF itself sets.
fn page_sizes(path: &Path) -> Vec<(f64, f64)> {
    let Ok(out) = Command::new("pdfinfo").args(["-f", "1", "-l", "100000"]).arg(path).output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut sizes = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Page") else { continue };
        if let Some((_, dims)) = rest.split_once("size:") {
            let nums: Vec<f64> = dims.split_whitespace().filter_map(|w| w.parse().ok()).collect();
            if nums.len() >= 2 {
                sizes.push((nums[0], nums[1]));
            }
        } else if let Some((_, rot)) = rest.split_once("rot:")
            && rot.trim().parse::<i32>().is_ok_and(|r| r % 180 != 0)
            && let Some(last) = sizes.last_mut()
        {
            *last = (last.1, last.0);
        }
    }
    sizes
}

/// One small PNG per page, keyed by page number. Blocking.
fn thumbnails(src: &Path) -> Vec<(usize, PathBuf)> {
    let dir = media::temp("pages", "d");
    if std::fs::create_dir_all(&dir).is_err() {
        return Vec::new();
    }
    let ok = media::run(Command::new("pdftoppm").args(["-png", "-scale-to", &(THUMB * 2).to_string()]).arg(src).arg(dir.join("p")));
    let mut out: Vec<(usize, PathBuf)> = Vec::new();
    if ok && let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            // pdftoppm names pages p-1.png, p-01.png, ... by page count.
            let n = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.rsplit('-').next()).and_then(|n| n.parse().ok());
            if let Some(n) = n {
                let kept = media::temp("page", "png");
                if std::fs::rename(&p, &kept).is_ok() {
                    out.push((n, kept));
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// Pages in the new order, then rotations, then (optionally) every page
/// scaled to the widest. Blocking.
fn organize(src: &Path, dst: &Path, pages: &[Page], sizes: &[(f64, f64)], same_width: bool) -> bool {
    let order: Vec<String> = pages.iter().map(|p| p.source.to_string()).collect();
    let ordered = media::temp("ordered", "pdf");
    let mut cmd = Command::new("qpdf");
    cmd.arg("--empty").arg("--pages").arg(src).arg(order.join(",")).arg("--");
    // Rotations name pages of the new document.
    for turns in 1..4u8 {
        let which: Vec<String> = pages.iter().enumerate().filter(|(_, p)| p.turns == turns).map(|(i, _)| (i + 1).to_string()).collect();
        if !which.is_empty() {
            cmd.arg(format!("--rotate=+{}:{}", turns as u32 * 90, which.join(",")));
        }
    }
    if !media::run(cmd.arg(&ordered)) {
        return false;
    }
    if !same_width {
        return std::fs::rename(&ordered, dst).is_ok() || (std::fs::copy(&ordered, dst).is_ok() && std::fs::remove_file(&ordered).is_ok());
    }

    // Each page as it now stands (turned), then fitted to the widest.
    let dims: Vec<(f64, f64)> = pages
        .iter()
        .map(|p| {
            let (w, h) = sizes.get(p.source - 1).copied().unwrap_or((612.0, 792.0));
            if p.turns % 2 == 1 { (h, w) } else { (w, h) }
        })
        .collect();
    let widest = dims.iter().map(|d| d.0).fold(0.0, f64::max);
    let mut parts = Vec::new();
    let mut ok = true;
    for (i, (w, h)) in dims.iter().enumerate() {
        let one = media::temp("one", "pdf");
        let fitted = media::temp("fit", "pdf");
        ok &= media::run(Command::new("qpdf").arg("--empty").arg("--pages").arg(&ordered).arg((i + 1).to_string()).arg("--").arg(&one));
        ok &= media::run(
            Command::new("pdftocairo")
                .args(["-pdf", "-expand", "-paperw", &format!("{}", widest.round()), "-paperh", &format!("{}", (h * widest / w).round())])
                .arg(&one)
                .arg(&fitted),
        );
        let _ = std::fs::remove_file(&one);
        parts.push(fitted);
        if !ok {
            break;
        }
    }
    ok = ok && media::run(Command::new("qpdf").arg("--empty").arg("--pages").args(&parts).arg("--").arg(dst));
    for p in parts.iter().chain([&ordered]) {
        let _ = std::fs::remove_file(p);
    }
    ok
}
