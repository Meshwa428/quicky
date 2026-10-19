//! An image with boxes drawn over it: one box to crop to, or several to
//! redact. Boxes are kept in image pixels, so they don't depend on the
//! window size and map straight onto ImageMagick geometry.

use crate::theme::Palette;
use gtk::{
    gdk,
    gdk_pixbuf::{InterpType, Pixbuf},
    prelude::*,
};
use std::{cell::RefCell, path::Path, process::Command, rc::Rc};

/// How close (px on screen) a press must be to an edge to grab it.
const GRAB: f64 = 10.0;
/// Smallest box, in image pixels.
const MIN_SIZE: f64 = 4.0;
/// Space kept around the image inside the widget.
pub const PAD: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn from_edges(l: f64, t: f64, r: f64, b: f64) -> Rect {
        Rect { x: l.min(r), y: t.min(b), w: (r - l).abs(), h: (b - t).abs() }
    }

    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// Whole pixels `(x, y, w, h)`, kept inside a `iw`×`ih` image.
    pub fn pixels(&self, iw: i32, ih: i32) -> (i32, i32, i32, i32) {
        let x = (self.x.round() as i32).clamp(0, iw - 1);
        let y = (self.y.round() as i32).clamp(0, ih - 1);
        let w = (self.w.round() as i32).clamp(1, iw - x);
        let h = (self.h.round() as i32).clamp(1, ih - y);
        (x, y, w, h)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// One box, dimmed outside: cropping.
    Single,
    /// Any number of boxes, filled with the cover: redacting.
    Many,
}

#[derive(Clone, Copy, PartialEq)]
struct Edges {
    l: bool,
    t: bool,
    r: bool,
    b: bool,
}

#[derive(Clone, Copy)]
enum Grab {
    None,
    New { ax: f64, ay: f64 },
    Move { orig: Rect },
    Resize { edges: Edges, orig: Rect },
}

struct State {
    image: Pixbuf,
    /// Drawn inside boxes in `Many` mode; `None` fills them black.
    cover: Option<Pixbuf>,
    palette: Palette,
    mode: Mode,
    boxes: Vec<Rect>,
    selected: Option<usize>,
    /// Width / height the boxes keep, if locked.
    aspect: Option<f64>,
    grab: Grab,
    /// Image point where the current drag began.
    origin: (f64, f64),
    /// The image and cover scaled to the last drawn size, so a redraw
    /// doesn't rescale a full-resolution picture.
    cache: Option<(i32, i32, Pixbuf, Option<Pixbuf>)>,
    /// Which boxes to draw; `None` draws all.
    visible: Option<Vec<bool>>,
    on_change: Option<Rc<dyn Fn()>>,
}

impl State {
    fn size(&self) -> (f64, f64) {
        (self.image.width() as f64, self.image.height() as f64)
    }

    /// Scale and offset that fit the image into a `w`×`h` widget.
    fn view(&self, w: f64, h: f64) -> (f64, f64, f64) {
        let (iw, ih) = self.size();
        let s = ((w - 2.0 * PAD) / iw).min((h - 2.0 * PAD) / ih).max(1e-6);
        (s, (w - iw * s) / 2.0, (h - ih * s) / 2.0)
    }
}

#[derive(Clone)]
pub struct Canvas {
    area: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
}

impl Canvas {
    pub fn new(image: Pixbuf, palette: Palette, mode: Mode) -> Self {
        let area = gtk::DrawingArea::builder().hexpand(true).vexpand(true).build();
        let full = Rect { x: 0.0, y: 0.0, w: image.width() as f64, h: image.height() as f64 };
        let state = Rc::new(RefCell::new(State {
            image,
            cover: None,
            palette,
            mode,
            boxes: if mode == Mode::Single { vec![full] } else { Vec::new() },
            selected: (mode == Mode::Single).then_some(0),
            aspect: None,
            grab: Grab::None,
            origin: (0.0, 0.0),
            cache: None,
            visible: None,
            on_change: None,
        }));
        let canvas = Canvas { area, state };
        canvas.connect_draw();
        canvas.connect_drag();
        canvas.connect_hover();
        canvas
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub fn image_size(&self) -> (i32, i32) {
        let s = self.state.borrow();
        (s.image.width(), s.image.height())
    }

    pub fn boxes(&self) -> Vec<Rect> {
        self.state.borrow().boxes.clone()
    }

    /// Fires whenever the boxes change.
    pub fn on_change(&self, f: impl Fn() + 'static) {
        self.state.borrow_mut().on_change = Some(Rc::new(f));
    }

    /// Swaps the picture for another of the same size (the next video
    /// frame), keeping the boxes.
    pub fn set_image(&self, image: Pixbuf) {
        let mut s = self.state.borrow_mut();
        s.image = image;
        s.cache = None;
        drop(s);
        self.area.queue_draw();
    }

    /// The selected box's index (`Many` mode).
    pub fn selected(&self) -> Option<usize> {
        self.state.borrow().selected
    }

    /// Selects box `i` (`Many` mode).
    pub fn select(&self, i: Option<usize>) {
        self.state.borrow_mut().selected = i;
        self.area.queue_draw();
    }

    /// Shows only the boxes `visible` says yes to (others are kept, just
    /// not drawn): a redaction outside its time range.
    pub fn set_visible(&self, visible: Vec<bool>) {
        self.state.borrow_mut().visible = Some(visible);
        self.area.queue_draw();
    }

    pub fn set_cover(&self, cover: Option<Pixbuf>) {
        let mut s = self.state.borrow_mut();
        s.cover = cover;
        s.cache = None;
        drop(s);
        self.area.queue_draw();
    }

    /// Locks boxes to `w / h`. The single box becomes the largest box of
    /// that shape that fits the image, centred where the box was.
    pub fn set_aspect(&self, aspect: Option<f64>) {
        {
            let mut s = self.state.borrow_mut();
            s.aspect = aspect;
            if let (Some(a), Mode::Single, Some(b)) = (aspect, s.mode, s.boxes.first().copied()) {
                let (iw, ih) = s.size();
                let (w, h) = if iw / ih > a { (ih * a, ih) } else { (iw, iw / a) };
                let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
                let x = (cx - w / 2.0).clamp(0.0, iw - w);
                let y = (cy - h / 2.0).clamp(0.0, ih - h);
                s.boxes[0] = Rect { x, y, w, h };
            }
        }
        self.changed();
    }

    /// Resizes the single box to `w`×`h` image pixels around its centre,
    /// kept inside the image. Typed sizes win over any aspect lock.
    pub fn set_size(&self, w: f64, h: f64) {
        {
            let mut s = self.state.borrow_mut();
            let (iw, ih) = s.size();
            let Some(b) = s.boxes.first().copied() else { return };
            let (w, h) = (w.clamp(MIN_SIZE, iw), h.clamp(MIN_SIZE, ih));
            let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
            s.aspect = None;
            s.boxes[0] = Rect { x: (cx - w / 2.0).clamp(0.0, iw - w), y: (cy - h / 2.0).clamp(0.0, ih - h), w, h };
        }
        self.changed();
    }

    /// Back to the starting state: the whole image, or no boxes.
    pub fn reset(&self) {
        {
            let mut s = self.state.borrow_mut();
            s.aspect = None;
            let (iw, ih) = s.size();
            match s.mode {
                Mode::Single => s.boxes = vec![Rect { x: 0.0, y: 0.0, w: iw, h: ih }],
                Mode::Many => {
                    s.boxes.clear();
                    s.selected = None;
                }
            }
        }
        self.changed();
    }

    /// Removes the selected box (`Many` mode). Returns whether one went.
    pub fn remove_selected(&self) -> bool {
        let removed = {
            let mut s = self.state.borrow_mut();
            match s.selected {
                Some(i) if s.mode == Mode::Many && i < s.boxes.len() => {
                    s.boxes.remove(i);
                    s.selected = s.boxes.len().checked_sub(1);
                    true
                }
                _ => false,
            }
        };
        if removed {
            self.changed();
        }
        removed
    }

    // Redraws, then notifies with the state released so the callback may
    // read the canvas.
    fn changed(&self) {
        self.area.queue_draw();
        let f = self.state.borrow().on_change.clone();
        if let Some(f) = f {
            f()
        }
    }

    fn connect_draw(&self) {
        let state = self.state.clone();
        self.area.set_draw_func(move |area, cr, w, h| {
            let factor = area.scale_factor().max(1) as f64;
            let mut s = state.borrow_mut();
            let (scale, ox, oy) = s.view(w as f64, h as f64);
            let (iw, ih) = s.size();
            let (vw, vh) = ((iw * scale * factor).round().max(1.0) as i32, (ih * scale * factor).round().max(1.0) as i32);
            if !matches!(&s.cache, Some((cw, ch, _, _)) if *cw == vw && *ch == vh) {
                let image = s.image.scale_simple(vw, vh, InterpType::Bilinear);
                let cover = s.cover.as_ref().and_then(|c| c.scale_simple(vw, vh, InterpType::Nearest));
                s.cache = image.map(|i| (vw, vh, i, cover));
            }
            let s = &*s;
            let p = &s.palette;
            let set = |c: gdk::RGBA| cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), c.alpha().into());
            let to_view = |r: &Rect| (ox + r.x * scale, oy + r.y * scale, r.w * scale, r.h * scale);

            // The picture, painted at device resolution.
            let paint = |pixbuf: &Pixbuf| {
                cr.save().ok();
                cr.translate(ox, oy);
                cr.scale(1.0 / factor, 1.0 / factor);
                cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
                let _ = cr.paint();
                cr.restore().ok();
            };
            if let Some((_, _, image, _)) = &s.cache {
                paint(image);
            }
            set(Palette::rgba(&p.foreground, 0.2));
            cr.set_line_width(1.0);
            cr.rectangle(ox - 0.5, oy - 0.5, iw * scale + 1.0, ih * scale + 1.0);
            let _ = cr.stroke();

            match s.mode {
                Mode::Single => {
                    if let Some(b) = s.boxes.first() {
                        let (x, y, bw, bh) = to_view(b);
                        // Dim what gets cut away.
                        set(Palette::rgba(&p.background, 0.7));
                        cr.set_fill_rule(gtk::cairo::FillRule::EvenOdd);
                        cr.rectangle(ox, oy, iw * scale, ih * scale);
                        cr.rectangle(x, y, bw, bh);
                        let _ = cr.fill();
                        // Thirds, to help framing.
                        set(Palette::rgba(&p.foreground, 0.25));
                        cr.set_line_width(1.0);
                        for k in [1.0, 2.0] {
                            cr.move_to(x + bw * k / 3.0, y);
                            cr.line_to(x + bw * k / 3.0, y + bh);
                            cr.move_to(x, y + bh * k / 3.0);
                            cr.line_to(x + bw, y + bh * k / 3.0);
                        }
                        let _ = cr.stroke();
                    }
                }
                Mode::Many => {
                    let shown = |i: usize| s.visible.as_ref().is_none_or(|v| v.get(i).copied().unwrap_or(true));
                    for (_, b) in s.boxes.iter().enumerate().filter(|(i, _)| shown(*i)) {
                        let (x, y, bw, bh) = to_view(b);
                        cr.save().ok();
                        cr.rectangle(x, y, bw, bh);
                        cr.clip();
                        match s.cache.as_ref().and_then(|(_, _, _, c)| c.as_ref()) {
                            Some(cover) => paint(cover),
                            None => {
                                cr.set_source_rgb(0.0, 0.0, 0.0);
                                let _ = cr.paint();
                            }
                        }
                        cr.restore().ok();
                    }
                }
            }

            // Outlines; the selected box gets handles.
            for (i, b) in s.boxes.iter().enumerate() {
                let (x, y, bw, bh) = to_view(b);
                let selected = s.selected == Some(i);
                set(if selected { Palette::rgba(&p.accent, 1.0) } else { Palette::rgba(&p.foreground, 0.6) });
                cr.set_line_width(if selected { 2.0 } else { 1.0 });
                cr.rectangle(x, y, bw, bh);
                let _ = cr.stroke();
                if selected {
                    let k = 7.0;
                    for (hx, hy) in [
                        (x, y), (x + bw / 2.0, y), (x + bw, y),
                        (x, y + bh / 2.0), (x + bw, y + bh / 2.0),
                        (x, y + bh), (x + bw / 2.0, y + bh), (x + bw, y + bh),
                    ] {
                        cr.rectangle(hx - k / 2.0, hy - k / 2.0, k, k);
                    }
                    let _ = cr.fill();
                }
            }
        });
    }

    fn connect_drag(&self) {
        let gesture = gtk::GestureDrag::new();

        let (state, area) = (self.state.clone(), self.area.clone());
        gesture.connect_drag_begin(move |_, x, y| {
            let mut s = state.borrow_mut();
            let (scale, ox, oy) = s.view(area.width() as f64, area.height() as f64);
            let (iw, ih) = s.size();
            let (px, py) = (((x - ox) / scale).clamp(0.0, iw), ((y - oy) / scale).clamp(0.0, ih));
            s.origin = (px, py);
            match hit(&s, x, y, scale, ox, oy) {
                Some((i, Some(edges))) => {
                    s.selected = Some(i);
                    s.grab = Grab::Resize { edges, orig: s.boxes[i] };
                }
                Some((i, None)) => {
                    s.selected = Some(i);
                    s.grab = Grab::Move { orig: s.boxes[i] };
                }
                None => {
                    // A fresh box: replaces the crop, or adds a redaction.
                    let fresh = Rect { x: px, y: py, w: 0.0, h: 0.0 };
                    match s.mode {
                        Mode::Single => s.boxes = vec![fresh],
                        Mode::Many => s.boxes.push(fresh),
                    }
                    s.selected = Some(s.boxes.len() - 1);
                    s.grab = Grab::New { ax: px, ay: py };
                }
            }
            drop(s);
            area.queue_draw();
        });

        let canvas = self.clone();
        gesture.connect_drag_update(move |_, dx, dy| {
            {
                let mut s = canvas.state.borrow_mut();
                let (scale, _, _) = s.view(canvas.area.width() as f64, canvas.area.height() as f64);
                let (iw, ih) = s.size();
                let (px, py) = ((s.origin.0 + dx / scale).clamp(0.0, iw), (s.origin.1 + dy / scale).clamp(0.0, ih));
                let Some(i) = s.selected else { return };
                let aspect = s.aspect;
                let next = match s.grab {
                    Grab::None => return,
                    Grab::New { ax, ay } => anchored(ax, ay, px, py, aspect, Axis::Both, iw, ih),
                    Grab::Move { orig } => Rect {
                        x: (orig.x + px - s.origin.0).clamp(0.0, iw - orig.w),
                        y: (orig.y + py - s.origin.1).clamp(0.0, ih - orig.h),
                        ..orig
                    },
                    Grab::Resize { edges, orig } => resize(orig, edges, px, py, aspect, iw, ih),
                };
                s.boxes[i] = next;
            }
            canvas.changed();
        });

        let canvas = self.clone();
        gesture.connect_drag_end(move |_, _, _| {
            {
                let mut s = canvas.state.borrow_mut();
                s.grab = Grab::None;
                // A click without a drag leaves a speck; drop it.
                if let Some(i) = s.selected
                    && s.boxes.get(i).is_some_and(|b| b.w < MIN_SIZE || b.h < MIN_SIZE)
                {
                    match s.mode {
                        Mode::Single => {
                            let (iw, ih) = s.size();
                            s.boxes = vec![Rect { x: 0.0, y: 0.0, w: iw, h: ih }];
                        }
                        Mode::Many => {
                            s.boxes.remove(i);
                            s.selected = s.boxes.len().checked_sub(1);
                        }
                    }
                }
            }
            canvas.changed();
        });

        self.area.add_controller(gesture);
    }

    // Cursor shapes hint at what a press will do.
    fn connect_hover(&self) {
        let motion = gtk::EventControllerMotion::new();
        let (state, area) = (self.state.clone(), self.area.clone());
        motion.connect_motion(move |_, x, y| {
            let s = state.borrow();
            if !matches!(s.grab, Grab::None) {
                return;
            }
            let (scale, ox, oy) = s.view(area.width() as f64, area.height() as f64);
            let name = match hit(&s, x, y, scale, ox, oy) {
                Some((_, Some(e))) => match (e.l || e.r, e.t || e.b) {
                    (true, true) if (e.l && e.t) || (e.r && e.b) => "nwse-resize",
                    (true, true) => "nesw-resize",
                    (true, false) => "ew-resize",
                    _ => "ns-resize",
                },
                Some((_, None)) => "move",
                None => "crosshair",
            };
            area.set_cursor_from_name(Some(name));
        });
        self.area.add_controller(motion);
    }
}

/// Which box a widget point lands on: its index, plus the edges under the
/// point (`None` inside). The selected box wins, then the topmost.
fn hit(s: &State, x: f64, y: f64, scale: f64, ox: f64, oy: f64) -> Option<(usize, Option<Edges>)> {
    let order = s.selected.into_iter().chain((0..s.boxes.len()).rev());
    for i in order {
        let Some(b) = s.boxes.get(i) else { continue };
        let (l, t, r, btm) = (ox + b.x * scale, oy + b.y * scale, ox + b.right() * scale, oy + b.bottom() * scale);
        let near_x = x > l - GRAB && x < r + GRAB;
        let near_y = y > t - GRAB && y < btm + GRAB;
        let edges = Edges {
            l: near_y && (x - l).abs() <= GRAB,
            r: near_y && (x - r).abs() <= GRAB,
            t: near_x && (y - t).abs() <= GRAB,
            b: near_x && (y - btm).abs() <= GRAB,
        };
        // On a tiny box, prefer the far edges so it can still grow.
        let edges = Edges { l: edges.l && !edges.r, t: edges.t && !edges.b, ..edges };
        if edges.l || edges.r || edges.t || edges.b {
            return Some((i, Some(edges)));
        }
        if x > l && x < r && y > t && y < btm {
            return Some((i, None));
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    Both,
    X,
    Y,
}

/// A box from a fixed corner `(ax, ay)` toward `(px, py)`, kept inside the
/// image and, when locked, at `aspect`. `axis` says which drag direction
/// sets the size when only one edge moves.
#[allow(clippy::too_many_arguments)]
fn anchored(ax: f64, ay: f64, px: f64, py: f64, aspect: Option<f64>, axis: Axis, iw: f64, ih: f64) -> Rect {
    let sx = if px < ax { -1.0 } else { 1.0 };
    let sy = if py < ay { -1.0 } else { 1.0 };
    let (mut w, mut h) = ((px - ax).abs(), (py - ay).abs());
    let max_w = if sx > 0.0 { iw - ax } else { ax };
    let max_h = if sy > 0.0 { ih - ay } else { ay };
    if let Some(a) = aspect {
        match axis {
            Axis::X => h = w / a,
            Axis::Y => w = h * a,
            Axis::Both if w / h.max(1e-6) > a => h = w / a,
            Axis::Both => w = h * a,
        }
        let k = 1f64.min(max_w / w.max(1e-6)).min(max_h / h.max(1e-6));
        w *= k;
        h *= k;
    } else {
        w = w.min(max_w);
        h = h.min(max_h);
    }
    Rect { x: if sx > 0.0 { ax } else { ax - w }, y: if sy > 0.0 { ay } else { ay - h }, w, h }
}

/// Moves the grabbed edges of `orig` to the pointer.
fn resize(orig: Rect, e: Edges, px: f64, py: f64, aspect: Option<f64>, iw: f64, ih: f64) -> Rect {
    if aspect.is_none() {
        let l = if e.l { px } else { orig.x };
        let r = if e.r { px } else { orig.right() };
        let t = if e.t { py } else { orig.y };
        let b = if e.b { py } else { orig.bottom() };
        return Rect::from_edges(l, t, r, b);
    }
    // Locked: pin the opposite corner and let the moved edge set the size.
    let ax = if e.l { orig.right() } else { orig.x };
    let ay = if e.t { orig.bottom() } else { orig.y };
    let horizontal = e.l || e.r;
    let vertical = e.t || e.b;
    // An untouched axis points away from the anchor, so the box keeps
    // growing right / down from it.
    let px = if horizontal { px } else { orig.right() };
    let py = if vertical { py } else { orig.bottom() };
    let axis = match (horizontal, vertical) {
        (true, true) => Axis::Both,
        (true, false) => Axis::X,
        _ => Axis::Y,
    };
    anchored(ax, ay, px, py, aspect, axis, iw, ih)
}

/// Loads an image upright. Formats GDK can't read (HEIC, AVIF, ...) go
/// through ImageMagick, which `-auto-orient`s them the same way.
pub fn load_image(path: &Path) -> Option<Pixbuf> {
    if let Ok(p) = Pixbuf::from_file(path) {
        return p.apply_embedded_orientation().or(Some(p));
    }
    let tmp = std::env::temp_dir().join(format!("quicky-view-{}.png", std::process::id()));
    let ok = Command::new("magick")
        .arg(format!("{}[0]", path.display()))
        .arg("-auto-orient")
        .arg(&tmp)
        .status()
        .is_ok_and(|s| s.success());
    let image = ok.then(|| Pixbuf::from_file(&tmp).ok()).flatten();
    let _ = std::fs::remove_file(&tmp);
    image
}

/// Blocky version of `image`, `block` image pixels per block. Matches
/// ImageMagick's `-sample` down and back up.
pub fn pixelated(image: &Pixbuf, block: i32) -> Option<Pixbuf> {
    let (w, h) = (image.width(), image.height());
    image
        .scale_simple((w / block).max(1), (h / block).max(1), InterpType::Nearest)?
        .scale_simple(w, h, InterpType::Nearest)
}

/// A soft preview of `-blur 0x{sigma}`: shrink, then smooth back up.
pub fn blurred(image: &Pixbuf, sigma: i32) -> Option<Pixbuf> {
    let (w, h) = (image.width(), image.height());
    let k = (sigma / 2).max(2);
    image
        .scale_simple((w / k).max(1), (h / k).max(1), InterpType::Hyper)?
        .scale_simple(w, h, InterpType::Bilinear)
}
