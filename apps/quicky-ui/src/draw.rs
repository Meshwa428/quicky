//! Cairo helpers for the editors that render a new picture (Add BG,
//! Collage, Annotate): rounded boxes, soft shadows, fitted images.

use gtk::{cairo::Context, gdk::prelude::*, gdk_pixbuf::Pixbuf};

/// An sRGB colour, 0..1 per channel.
pub type Rgb = (f64, f64, f64);

/// `"#3a7bd5"` → `(0.23, 0.48, 0.84)`; black if it doesn't parse.
pub fn hex(s: &str) -> Rgb {
    let s = s.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0;
    (c(0), c(2), c(4))
}

pub fn set(cr: &Context, (r, g, b): Rgb, a: f64) {
    cr.set_source_rgba(r, g, b, a);
}

/// Adds a rounded rectangle to the path.
pub fn rounded(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 1.5 * pi);
    cr.close_path();
}

/// A soft drop shadow under a rounded box: stacked, growing, faint boxes
/// stand in for a blur. `size` is how far it spreads, in pixels.
pub fn shadow(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64, size: f64) {
    if size <= 0.0 {
        return;
    }
    let steps = 16;
    let drop = size / 3.0;
    for i in 0..steps {
        let grow = size * (i as f64 + 1.0) / steps as f64;
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.35 / steps as f64 * 1.6);
        rounded(cr, x - grow / 2.0, y - grow / 2.0 + drop, w + grow, h + grow, r + grow / 2.0);
        let _ = cr.fill();
    }
}

/// Paints `image` to fill the box, cropping what overflows.
pub fn cover(cr: &Context, image: &Pixbuf, x: f64, y: f64, w: f64, h: f64) {
    let k = (w / image.width() as f64).max(h / image.height() as f64);
    paint_scaled(cr, image, x, y, w, h, k);
}

/// Paints all of `image` inside the box, centred.
pub fn contain(cr: &Context, image: &Pixbuf, x: f64, y: f64, w: f64, h: f64) {
    let k = (w / image.width() as f64).min(h / image.height() as f64);
    paint_scaled(cr, image, x, y, w, h, k);
}

fn paint_scaled(cr: &Context, image: &Pixbuf, x: f64, y: f64, w: f64, h: f64, k: f64) {
    let (iw, ih) = (image.width() as f64 * k, image.height() as f64 * k);
    cr.save().ok();
    cr.rectangle(x, y, w, h);
    cr.clip();
    cr.translate(x + (w - iw) / 2.0, y + (h - ih) / 2.0);
    cr.scale(k, k);
    cr.set_source_pixbuf(image, 0.0, 0.0);
    // Smooth downscaling instead of cairo's default nearest-ish filter.
    cr.source().set_filter(gtk::cairo::Filter::Good);
    let _ = cr.paint();
    cr.restore().ok();
}
