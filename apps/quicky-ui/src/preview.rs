//! A picture of a fixed height that fits its image inside, whatever the
//! image's size. (`gtk::Picture` asks for its image's full height, which
//! squeezes everything below it.)

use gtk::{
    gdk_pixbuf::{InterpType, Pixbuf},
    prelude::*,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
pub struct Preview {
    area: gtk::DrawingArea,
    /// The image, and a copy scaled to the last drawn size.
    image: Rc<RefCell<(Option<Pixbuf>, Option<Pixbuf>)>>,
}

impl Preview {
    pub fn new(height: i32) -> Self {
        let area = gtk::DrawingArea::builder().content_height(height).hexpand(true).build();
        let image: Rc<RefCell<(Option<Pixbuf>, Option<Pixbuf>)>> = Rc::default();
        let state = image.clone();
        area.set_draw_func(move |area, cr, w, h| {
            let mut s = state.borrow_mut();
            let Some(img) = s.0.clone() else { return };
            let factor = area.scale_factor().max(1) as f64;
            let k = (w as f64 / img.width() as f64).min(h as f64 / img.height() as f64);
            let (vw, vh) = ((img.width() as f64 * k * factor).round().max(1.0) as i32, (img.height() as f64 * k * factor).round().max(1.0) as i32);
            if !s.1.as_ref().is_some_and(|c| c.width() == vw && c.height() == vh) {
                s.1 = img.scale_simple(vw, vh, InterpType::Bilinear);
            }
            let Some(scaled) = &s.1 else { return };
            let (x, y) = ((w as f64 - vw as f64 / factor) / 2.0, (h as f64 - vh as f64 / factor) / 2.0);
            cr.translate(x, y);
            cr.scale(1.0 / factor, 1.0 / factor);
            cr.set_source_pixbuf(scaled, 0.0, 0.0);
            let _ = cr.paint();
        });
        Preview { area, image }
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub fn set(&self, image: Option<Pixbuf>) {
        *self.image.borrow_mut() = (image, None);
        self.area.queue_draw();
    }
}
