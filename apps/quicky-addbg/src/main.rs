//! quicky-addbg: place an image on a new background, saved as a copy.
//!
//!     quicky-addbg FILE
//!
//! Pick a gradient, a colour, or another picture (optionally blurred), then
//! set the padding, corner rounding, shadow, and canvas shape. The original
//! is never touched; the result lands next to it as `name-bg.ext`.

use gtk::{cairo, gdk_pixbuf::Pixbuf, gio, glib, prelude::*};
use quicky_ui::{
    Frame, Palette, app, canvas,
    draw::{self, Rgb},
    media,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.addbg";

/// Longest edge of the picture the preview draws from.
const PREVIEW: i32 = 1000;

const GRADIENTS: [(&str, &str); 6] = [
    ("#ff9a9e", "#fad0c4"),
    ("#a18cd1", "#fbc2eb"),
    ("#84fab0", "#8fd3f4"),
    ("#fccb90", "#d57eeb"),
    ("#4facfe", "#00f2fe"),
    ("#434343", "#000000"),
];
const SOLIDS: [&str; 4] = ["#ffffff", "#e9e9e9", "#1e1e1e", "#f5e6c8"];

/// Tangerine's canvas shapes: label and width / height (`None` hugs the
/// image).
const RATIOS: [(&str, Option<f64>); 4] = [("Auto", None), ("1:1", Some(1.0)), ("16:9", Some(16.0 / 9.0)), ("4:5", Some(0.8))];

#[derive(Clone, PartialEq)]
enum Fill {
    Gradient(Rgb, Rgb),
    Solid(Rgb),
    Picture(PathBuf),
}

/// Everything the render needs, in full-size pixels. Plain data, so the
/// save can run on a worker thread.
#[derive(Clone)]
struct Scene {
    size: (f64, f64),
    fill: Fill,
    blur: bool,
    padding: f64,
    corners: f64,
    shadow: f64,
    ratio: Option<f64>,
}

impl Scene {
    fn canvas(&self) -> (f64, f64) {
        let (w, h) = (self.size.0 + 2.0 * self.padding, self.size.1 + 2.0 * self.padding);
        match self.ratio {
            Some(r) if w / h < r => (h * r, h),
            Some(r) => (w, w / r),
            None => (w, h),
        }
    }
}

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Add Background", &["image/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let Some(image) = canvas::load_image(&path) else {
        media::notify("Can't add a background", &format!("{name} isn't an image Quicky can open"));
        return;
    };
    let size = (image.width() as f64, image.height() as f64);
    let small = shrink(&image, PREVIEW);
    // Padding and the other sizes follow the picture: 160 px on a
    // 1600 px wide image.
    let unit = size.0.max(size.1) / 1600.0;

    let frame = Frame::new(app, "Add Background", &name, 720, "Save with Background");
    let reset = frame.reset_button();
    let body = &frame.body;

    let scene = Rc::new(RefCell::new(Scene {
        size,
        fill: Fill::Gradient(draw::hex(GRADIENTS[0].0), draw::hex(GRADIENTS[0].1)),
        blur: true,
        padding: 160.0 * unit,
        corners: 24.0 * unit,
        shadow: 24.0 * unit,
        ratio: None,
    }));
    // The chosen background picture, scaled down for the preview.
    let backdrop: Rc<RefCell<Option<Pixbuf>>> = Rc::default();

    let preview = gtk::DrawingArea::builder().content_height(250).hexpand(true).build();
    {
        let (scene, backdrop, small) = (scene.clone(), backdrop.clone(), small.clone());
        preview.set_draw_func(move |_, cr, w, h| {
            let s = scene.borrow();
            let (cw, ch) = s.canvas();
            let k = (w as f64 / cw).min(h as f64 / ch);
            cr.translate((w as f64 - cw * k) / 2.0, (h as f64 - ch * k) / 2.0);
            render(cr, &small, backdrop.borrow().as_ref(), &s, k);
        });
    }
    body.append(&preview);

    let controls = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let scroller = gtk::ScrolledWindow::builder().child(&controls).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    body.append(&scroller);

    controls.append(&Frame::section("Background"));
    let mut swatches: Vec<(gtk::ToggleButton, Option<Fill>)> = Vec::new();
    let gradient_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for (a, b) in GRADIENTS {
        let fill = Fill::Gradient(draw::hex(a), draw::hex(b));
        let button = swatch(&fill);
        gradient_row.append(&button);
        swatches.push((button, Some(fill)));
    }
    controls.append(&gradient_row);
    let solid_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for c in SOLIDS.iter().copied().chain([palette.accent.as_str(), palette.background.as_str()]) {
        let fill = Fill::Solid(draw::hex(c));
        let button = swatch(&fill);
        solid_row.append(&button);
        swatches.push((button, Some(fill)));
    }
    controls.append(&solid_row);

    // A colour of your own, or another picture.
    let custom = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    custom.set_rgba(&Palette::rgba(&palette.accent, 1.0));
    let picture = gtk::ToggleButton::with_label("Image…");
    let blur = gtk::CheckButton::builder().label("Blur").active(true).sensitive(false).build();
    let own = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let custom_label = gtk::Label::new(Some("Custom"));
    own.append(&custom_label);
    own.append(&custom);
    own.append(&gtk::Box::builder().hexpand(true).build());
    own.append(&picture);
    own.append(&blur);
    controls.append(&own);
    swatches.push((picture.clone(), None));
    for (b, _) in &swatches[1..] {
        b.set_group(Some(&swatches[0].0));
    }
    swatches[0].0.set_active(true);

    controls.append(&Frame::section("Layout"));
    let px = |v: f64| format!("{v:.0} px");
    let (pad_row, padding) = Frame::slider("Padding", (0.0, 400.0), 1.0, 160.0, px);
    let (corner_row, corners) = Frame::slider("Corners", (0.0, 120.0), 1.0, 24.0, px);
    let (shadow_row, shadow) = Frame::slider("Shadow", (0.0, 120.0), 1.0, 24.0, px);
    controls.append(&pad_row);
    controls.append(&corner_row);
    controls.append(&shadow_row);
    let (ratio_row, ratios) = Frame::choices(&RATIOS.map(|(label, _)| label));
    controls.append(&Frame::labelled("Ratio", &ratio_row));

    // ---------------------------------------------------------------- wiring

    let redraw = {
        let preview = preview.clone();
        move || preview.queue_draw()
    };
    for (button, fill) in &swatches {
        let Some(fill) = fill.clone() else { continue };
        let (scene, redraw) = (scene.clone(), redraw.clone());
        button.connect_toggled(move |b| {
            if b.is_active() {
                scene.borrow_mut().fill = fill.clone();
                redraw();
            }
        });
    }
    {
        let (scene, redraw, first) = (scene.clone(), redraw.clone(), swatches[0].0.clone());
        custom.connect_rgba_notify(move |c| {
            let rgba = c.rgba();
            // Choosing a colour picks it; no swatch is lit for it.
            first.set_active(true);
            first.set_active(false);
            scene.borrow_mut().fill = Fill::Solid((rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64));
            redraw();
        });
    }
    {
        let (scene, backdrop, redraw, window, blur) = (scene.clone(), backdrop.clone(), redraw.clone(), frame.window.clone(), blur.clone());
        let chosen: Rc<RefCell<Option<PathBuf>>> = Rc::default();
        picture.connect_toggled(move |b| {
            blur.set_sensitive(b.is_active());
            if !b.is_active() {
                return;
            }
            if let Some(p) = chosen.borrow().clone() {
                scene.borrow_mut().fill = Fill::Picture(p);
                redraw();
                return;
            }
            let filter = gtk::FileFilter::new();
            filter.add_mime_type("image/*");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder().title("Background image").filters(&filters).build();
            let (scene, backdrop, redraw, chosen, b) = (scene.clone(), backdrop.clone(), redraw.clone(), chosen.clone(), b.clone());
            dialog.open(Some(&window), None::<&gio::Cancellable>, move |res| {
                let Some(p) = res.ok().and_then(|f| f.path()) else { return };
                let Some(img) = canvas::load_image(&p) else { return };
                *backdrop.borrow_mut() = Some(shrink(&img, PREVIEW));
                *chosen.borrow_mut() = Some(p.clone());
                if b.is_active() {
                    scene.borrow_mut().fill = Fill::Picture(p);
                }
                redraw();
            });
        });
    }
    {
        let (scene, redraw) = (scene.clone(), redraw.clone());
        blur.connect_toggled(move |b| {
            scene.borrow_mut().blur = b.is_active();
            redraw();
        });
    }
    for (scale, field) in [(&padding, 0), (&corners, 1), (&shadow, 2)] {
        let (scene, redraw) = (scene.clone(), redraw.clone());
        scale.connect_value_changed(move |s| {
            let v = s.value() * unit;
            let mut sc = scene.borrow_mut();
            match field {
                0 => sc.padding = v,
                1 => sc.corners = v,
                _ => sc.shadow = v,
            }
            drop(sc);
            redraw();
        });
    }
    for (i, b) in ratios.iter().enumerate() {
        let (scene, redraw) = (scene.clone(), redraw.clone());
        b.connect_toggled(move |b| {
            if b.is_active() {
                scene.borrow_mut().ratio = RATIOS[i].1;
                redraw();
            }
        });
    }
    {
        let (swatches, padding, corners, shadow, ratios) = (swatches.clone(), padding.clone(), corners.clone(), shadow.clone(), ratios.clone());
        reset.connect_clicked(move |_| {
            swatches[0].0.set_active(true);
            padding.set_value(160.0);
            corners.set_value(24.0);
            shadow.set_value(24.0);
            ratios[0].set_active(true);
        });
    }

    {
        let (frame, scene) = (frame.clone(), scene.clone());
        frame.primary.clone().connect_clicked(move |_| {
            let s = scene.borrow().clone();
            let out = media::output_path(&path, "-bg", media::image_out_ext(&path));
            let src = path.clone();
            app::finish(&frame, "Saving…", "Background added", "Save failed", out, move |dst| save(&src, &s, dst));
        });
    }

    frame.window.present();
}

/// A toggle showing a gradient or colour.
fn swatch(fill: &Fill) -> gtk::ToggleButton {
    let area = gtk::DrawingArea::builder().content_width(26).content_height(26).build();
    let fill = fill.clone();
    area.set_draw_func(move |_, cr, w, h| {
        paint_fill(cr, &fill, None, false, w as f64, h as f64);
    });
    let button = gtk::ToggleButton::builder().child(&area).build();
    button.add_css_class("swatch");
    button
}

fn shrink(image: &Pixbuf, edge: i32) -> Pixbuf {
    let k = (edge as f64 / image.width().max(image.height()) as f64).min(1.0);
    image
        .scale_simple(((image.width() as f64 * k) as i32).max(1), ((image.height() as f64 * k) as i32).max(1), gtk::gdk_pixbuf::InterpType::Bilinear)
        .unwrap_or_else(|| image.clone())
}

/// Paints the background over a `w`×`h` canvas.
fn paint_fill(cr: &cairo::Context, fill: &Fill, backdrop: Option<&Pixbuf>, blur: bool, w: f64, h: f64) {
    match fill {
        Fill::Solid(c) => {
            draw::set(cr, *c, 1.0);
            let _ = cr.paint();
        }
        Fill::Gradient(a, b) => {
            let g = cairo::LinearGradient::new(0.0, 0.0, w, h);
            g.add_color_stop_rgb(0.0, a.0, a.1, a.2);
            g.add_color_stop_rgb(1.0, b.0, b.1, b.2);
            let _ = cr.set_source(&g);
            let _ = cr.paint();
        }
        Fill::Picture(_) => match backdrop {
            Some(img) => {
                // A heavy blur: shrink hard, let cairo smooth it back up.
                let soft = if blur { img.scale_simple((img.width() / 24).max(2), (img.height() / 24).max(2), gtk::gdk_pixbuf::InterpType::Hyper) } else { None };
                draw::cover(cr, soft.as_ref().unwrap_or(img), 0.0, 0.0, w, h);
            }
            None => {
                cr.set_source_rgb(0.5, 0.5, 0.5);
                let _ = cr.paint();
            }
        },
    }
}

/// Draws the scene at `k` times full size, from the top-left corner.
fn render(cr: &cairo::Context, image: &Pixbuf, backdrop: Option<&Pixbuf>, s: &Scene, k: f64) {
    let (cw, ch) = s.canvas();
    let (cw, ch) = (cw * k, ch * k);
    cr.save().ok();
    cr.rectangle(0.0, 0.0, cw, ch);
    cr.clip();
    paint_fill(cr, &s.fill, backdrop, s.blur, cw, ch);
    let (w, h) = (s.size.0 * k, s.size.1 * k);
    let (x, y) = ((cw - w) / 2.0, (ch - h) / 2.0);
    let r = s.corners * k;
    draw::shadow(cr, x, y, w, h, r, s.shadow * k);
    cr.save().ok();
    draw::rounded(cr, x, y, w, h, r);
    cr.clip();
    draw::cover(cr, image, x, y, w, h);
    cr.restore().ok();
    cr.restore().ok();
}

/// Renders at full size and writes the copy. Blocking.
fn save(src: &Path, s: &Scene, dst: &Path) -> bool {
    let Some(image) = canvas::load_image(src) else { return false };
    let backdrop = match &s.fill {
        Fill::Picture(p) => canvas::load_image(p).map(|i| shrink(&i, 1600)),
        _ => None,
    };
    let (cw, ch) = s.canvas();
    let Ok(mut surface) = cairo::ImageSurface::create(cairo::Format::ARgb32, cw.round() as i32, ch.round() as i32) else { return false };
    {
        let Ok(cr) = cairo::Context::new(&surface) else { return false };
        render(&cr, &image, backdrop.as_ref(), s, 1.0);
    }
    media::save_surface(&mut surface, dst)
}
