//! quicky-collage: lay several images out as one picture.
//!
//!     quicky-collage FILE...
//!
//! Choose a style (grid, row, column, or one featured image), the canvas
//! shape and width, how images fill their cells, the spacing, and the
//! order, then Create Collage. The result lands next to the first image as
//! `collage.jpg`.

use gtk::{cairo, gdk_pixbuf::Pixbuf, glib, prelude::*};
use quicky_ui::{
    Frame, Palette, app, canvas,
    draw::{self, Rgb},
    media,
};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.collage";

/// Longest edge of the copies the preview draws from.
const THUMB: i32 = 700;

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Grid,
    Row,
    Column,
    Featured,
}

const STYLES: [(&str, Style); 4] = [("Grid", Style::Grid), ("Row", Style::Row), ("Column", Style::Column), ("Featured", Style::Featured)];
/// Canvas shapes: label and width / height (`None` follows the images).
const CANVASES: [(&str, Option<f64>); 5] = [
    ("Auto · fit images", None),
    ("Square · 1:1", Some(1.0)),
    ("Landscape · 16:9", Some(16.0 / 9.0)),
    ("Portrait · 9:16", Some(9.0 / 16.0)),
    ("Classic · 4:3", Some(4.0 / 3.0)),
];
const WIDTHS: [u32; 4] = [1080, 2048, 3000, 4096];
const BACKGROUNDS: [&str; 3] = ["#ffffff", "#000000", "#f2efe9"];

#[derive(Clone)]
struct Layout {
    style: Style,
    columns: usize,
    canvas: Option<f64>,
    width: f64,
    fill: bool,
    spacing: f64,
    corners: f64,
    background: Rgb,
}

impl Layout {
    fn default_for(n: usize) -> Layout {
        Layout {
            style: Style::Grid,
            columns: (n as f64).sqrt().ceil().max(1.0) as usize,
            canvas: Some(1.0),
            width: 2048.0,
            fill: true,
            spacing: 16.0,
            corners: 0.0,
            background: draw::hex(BACKGROUNDS[0]),
        }
    }

    /// Canvas size and each image's cell, in output pixels. `aspects` are
    /// the images' width / height, in order.
    fn cells(&self, aspects: &[f64]) -> ((f64, f64), Vec<(f64, f64, f64, f64)>) {
        let n = aspects.len().max(1);
        let avg = aspects.iter().sum::<f64>() / n as f64;
        let (w, g) = (self.width, self.spacing * self.width / 2048.0);
        let (cols, rows) = match self.style {
            Style::Grid => {
                let c = self.columns.clamp(1, n);
                (c, n.div_ceil(c))
            }
            Style::Row => (n, 1),
            Style::Column => (1, n),
            Style::Featured => (2, 1),
        };
        // Without a fixed shape, cells take the images' average shape.
        let h = match self.canvas {
            Some(r) => w / r,
            None => {
                let cw = (w - g * (cols as f64 + 1.0)) / cols as f64;
                let ch = if self.style == Style::Featured { cw * 1.2 } else { cw / avg };
                ch * rows as f64 + g * (rows as f64 + 1.0)
            }
        };
        let mut out = Vec::with_capacity(n);
        if self.style == Style::Featured && n > 1 {
            // The first image takes two thirds; the rest stack beside it.
            let big = (w - 3.0 * g) * 2.0 / 3.0;
            out.push((g, g, big, h - 2.0 * g));
            let rest = n - 1;
            let sw = w - 3.0 * g - big;
            let sh = (h - g * (rest as f64 + 1.0)) / rest as f64;
            for i in 0..rest {
                out.push((2.0 * g + big, g + i as f64 * (sh + g), sw, sh));
            }
            return ((w, h), out);
        }
        let (cols, rows) = if self.style == Style::Featured { (1, 1) } else { (cols, rows) };
        let cw = (w - g * (cols as f64 + 1.0)) / cols as f64;
        let ch = (h - g * (rows as f64 + 1.0)) / rows as f64;
        for i in 0..n {
            let (r, c) = (i / cols, i % cols);
            // A short last row is centred.
            let in_row = if r == rows - 1 { n - r * cols } else { cols };
            let shift = (cols - in_row) as f64 * (cw + g) / 2.0;
            out.push((g + c as f64 * (cw + g) + shift, g + r as f64 * (ch + g), cw, ch));
        }
        ((w, h), out)
    }
}

fn main() -> glib::ExitCode {
    app::run_many(APP_ID, 2, "Choose two or more images for a collage", build)
}

fn build(app: &gtk::Application, files: Vec<PathBuf>) {
    let palette = Palette::load();
    let thumbs: Vec<(PathBuf, Pixbuf)> = files
        .iter()
        .filter_map(|f| canvas::load_image(f).map(|p| (f.clone(), shrink(&p, THUMB))))
        .collect();
    if thumbs.len() < 2 {
        media::notify("Can't make a collage", "Quicky couldn't open enough of those images");
        return;
    }
    let n = thumbs.len();
    let order = Rc::new(RefCell::new(thumbs));
    let layout = Rc::new(RefCell::new(Layout::default_for(n)));

    let frame = Frame::new(app, "Create Collage", &format!("{n} images"), 720, "Create Collage");
    let reset = frame.reset_button();
    reset.set_label("Reset Layout");
    let body = &frame.body;

    let preview = gtk::DrawingArea::builder().content_height(230).hexpand(true).build();
    {
        let (order, layout) = (order.clone(), layout.clone());
        preview.set_draw_func(move |_, cr, w, h| {
            let images: Vec<Pixbuf> = order.borrow().iter().map(|(_, p)| p.clone()).collect();
            let l = layout.borrow();
            let ((cw, ch), _) = l.cells(&aspects(&images));
            let k = (w as f64 / cw).min(h as f64 / ch);
            cr.translate((w as f64 - cw * k) / 2.0, (h as f64 - ch * k) / 2.0);
            cr.scale(k, k);
            render(cr, &images, &l);
        });
    }
    body.append(&preview);

    let controls = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let scroller = gtk::ScrolledWindow::builder().child(&controls).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    body.append(&scroller);

    let (style_row, style_buttons) = Frame::choices(&STYLES.map(|(l, _)| l));
    controls.append(&Frame::labelled("Style", &style_row));
    let columns = gtk::SpinButton::with_range(1.0, 8.0, 1.0);
    columns.set_value(layout.borrow().columns as f64);
    let columns_row = Frame::labelled("Columns", &columns);
    controls.append(&columns_row);
    let canvas_list = gtk::DropDown::from_strings(&CANVASES.map(|(l, _)| l));
    canvas_list.set_selected(1);
    controls.append(&Frame::labelled("Canvas", &canvas_list));
    let width_labels: Vec<String> = WIDTHS.iter().map(|w| format!("{w} px")).collect();
    let width_list = gtk::DropDown::from_strings(&width_labels.iter().map(String::as_str).collect::<Vec<_>>());
    width_list.set_selected(1);
    controls.append(&Frame::labelled("Width", &width_list));
    let fit_list = gtk::DropDown::from_strings(&["Fill cells (crop edges)", "Fit inside cells"]);
    controls.append(&Frame::labelled("Image fit", &fit_list));

    // Spacing & background, folded away until wanted.
    let spacing_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let px = |v: f64| format!("{v:.0} px");
    let (spacing_row, spacing) = Frame::slider("Spacing", (0.0, 120.0), 1.0, 16.0, px);
    let (corner_row, corners) = Frame::slider("Corners", (0.0, 120.0), 1.0, 0.0, px);
    spacing_box.append(&spacing_row);
    spacing_box.append(&corner_row);
    let bg_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bg_row.append(&gtk::Label::builder().label("Background").xalign(0.0).hexpand(true).build());
    let mut bg_buttons: Vec<gtk::ToggleButton> = Vec::new();
    for c in BACKGROUNDS.iter().copied().chain([palette.background.as_str()]) {
        let rgb = draw::hex(c);
        let dot = gtk::DrawingArea::builder().content_width(22).content_height(22).build();
        dot.set_draw_func(move |_, cr, _, _| {
            draw::set(cr, rgb, 1.0);
            let _ = cr.paint();
        });
        let b = gtk::ToggleButton::builder().child(&dot).build();
        b.add_css_class("swatch");
        if let Some(first) = bg_buttons.first() {
            b.set_group(Some(first));
        }
        bg_row.append(&b);
        bg_buttons.push(b);
    }
    bg_buttons[0].set_active(true);
    spacing_box.append(&bg_row);
    controls.append(&gtk::Expander::builder().label("Spacing & background").child(&spacing_box).build());

    // Image order: ▲ ▼ per image, like the Join list.
    let order_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    controls.append(&gtk::Expander::builder().label("Image order").child(&order_list).build());

    // ---------------------------------------------------------------- wiring

    let redraw = {
        let preview = preview.clone();
        move || preview.queue_draw()
    };

    let render_order: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::default();
    let fill_order: Rc<dyn Fn()> = {
        let (order_list, order, redraw, slot) = (order_list.clone(), order.clone(), redraw.clone(), render_order.clone());
        Rc::new(move || {
            while let Some(c) = order_list.first_child() {
                order_list.remove(&c);
            }
            let names: Vec<String> = order.borrow().iter().map(|(p, _)| p.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string()).collect();
            for (i, name) in names.iter().enumerate() {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let index = gtk::Label::builder().label((i + 1).to_string()).width_chars(2).build();
                index.add_css_class("dim");
                row.append(&index);
                row.append(&gtk::Label::builder().label(name).xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::Middle).build());
                for (icon, delta, ok) in [("go-up-symbolic", -1i32, i > 0), ("go-down-symbolic", 1, i + 1 < names.len())] {
                    let b = gtk::Button::builder().icon_name(icon).sensitive(ok).build();
                    b.add_css_class("flat");
                    let (order, redraw, slot) = (order.clone(), redraw.clone(), slot.clone());
                    b.connect_clicked(move |_| {
                        order.borrow_mut().swap(i, (i as i32 + delta) as usize);
                        redraw();
                        if let Some(f) = slot.borrow().clone() {
                            // Rebuild after this click handler returns.
                            glib::idle_add_local_once(move || f());
                        }
                    });
                    row.append(&b);
                }
                order_list.append(&row);
            }
        })
    };
    *render_order.borrow_mut() = Some(fill_order.clone());
    fill_order();

    let set = {
        let (layout, redraw) = (layout.clone(), redraw.clone());
        move |f: &dyn Fn(&mut Layout)| {
            f(&mut layout.borrow_mut());
            redraw();
        }
    };
    for (i, b) in style_buttons.iter().enumerate() {
        let (set, columns_row) = (set.clone(), columns_row.clone());
        b.connect_toggled(move |b| {
            if b.is_active() {
                columns_row.set_sensitive(STYLES[i].1 == Style::Grid);
                set(&|l| l.style = STYLES[i].1);
            }
        });
    }
    {
        let set = set.clone();
        columns.connect_value_changed(move |c| {
            let v = c.value() as usize;
            set(&|l| l.columns = v);
        });
    }
    {
        let set = set.clone();
        canvas_list.connect_selected_notify(move |d| {
            let r = CANVASES[d.selected() as usize].1;
            set(&|l| l.canvas = r);
        });
    }
    {
        let set = set.clone();
        width_list.connect_selected_notify(move |d| {
            let w = WIDTHS[d.selected() as usize] as f64;
            set(&|l| l.width = w);
        });
    }
    {
        let set = set.clone();
        fit_list.connect_selected_notify(move |d| {
            let fill = d.selected() == 0;
            set(&|l| l.fill = fill);
        });
    }
    {
        let set = set.clone();
        spacing.connect_value_changed(move |s| {
            let v = s.value();
            set(&|l| l.spacing = v);
        });
    }
    {
        let set = set.clone();
        corners.connect_value_changed(move |s| {
            let v = s.value();
            set(&|l| l.corners = v);
        });
    }
    let bg_colors: Vec<&str> = BACKGROUNDS.iter().copied().chain([palette.background.as_str()]).collect();
    for (i, b) in bg_buttons.iter().enumerate() {
        let (set, c) = (set.clone(), draw::hex(bg_colors[i]));
        b.connect_toggled(move |b| {
            if b.is_active() {
                set(&|l| l.background = c);
            }
        });
    }
    {
        let (style_buttons, columns, canvas_list, width_list, fit_list, spacing, corners, bg_buttons) =
            (style_buttons.clone(), columns.clone(), canvas_list.clone(), width_list.clone(), fit_list.clone(), spacing.clone(), corners.clone(), bg_buttons.clone());
        let d = Layout::default_for(n);
        reset.connect_clicked(move |_| {
            style_buttons[0].set_active(true);
            columns.set_value(d.columns as f64);
            canvas_list.set_selected(1);
            width_list.set_selected(1);
            fit_list.set_selected(0);
            spacing.set_value(16.0);
            corners.set_value(0.0);
            bg_buttons[0].set_active(true);
        });
    }

    {
        let (frame, order, layout) = (frame.clone(), order.clone(), layout.clone());
        frame.primary.clone().connect_clicked(move |_| {
            let paths: Vec<PathBuf> = order.borrow().iter().map(|(p, _)| p.clone()).collect();
            let l = layout.borrow().clone();
            let dir = paths[0].parent().map(|d| d.to_path_buf()).unwrap_or_default();
            let out = media::output_named(&dir, "collage", "jpg");
            app::finish(&frame, "Creating…", "Collage created", "Collage failed", out, move |dst| {
                // Full-size images, shrunk only to what their cells need.
                let edge = l.width as i32;
                let images: Vec<Pixbuf> = paths.iter().filter_map(|p| canvas::load_image(p).map(|i| shrink(&i, edge))).collect();
                let ((w, h), _) = l.cells(&aspects(&images));
                let Ok(mut surface) = cairo::ImageSurface::create(cairo::Format::ARgb32, w.round() as i32, h.round() as i32) else { return false };
                {
                    let Ok(cr) = cairo::Context::new(&surface) else { return false };
                    render(&cr, &images, &l);
                }
                media::save_surface(&mut surface, dst)
            });
        });
    }

    frame.window.present();
}

fn aspects(images: &[Pixbuf]) -> Vec<f64> {
    images.iter().map(|p| p.width() as f64 / p.height().max(1) as f64).collect()
}

fn shrink(image: &Pixbuf, edge: i32) -> Pixbuf {
    let k = (edge as f64 / image.width().max(image.height()) as f64).min(1.0);
    image
        .scale_simple(((image.width() as f64 * k) as i32).max(1), ((image.height() as f64 * k) as i32).max(1), gtk::gdk_pixbuf::InterpType::Bilinear)
        .unwrap_or_else(|| image.clone())
}

/// Draws the collage at output size from the top-left corner.
fn render(cr: &cairo::Context, images: &[Pixbuf], l: &Layout) {
    let ((w, h), cells) = l.cells(&aspects(images));
    draw::set(cr, l.background, 1.0);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let r = l.corners * l.width / 2048.0;
    for (image, (x, y, cw, ch)) in images.iter().zip(cells) {
        cr.save().ok();
        draw::rounded(cr, x, y, cw, ch, r);
        cr.clip();
        if l.fill {
            draw::cover(cr, image, x, y, cw, ch);
        } else {
            draw::contain(cr, image, x, y, cw, ch);
        }
        cr.restore().ok();
    }
}
