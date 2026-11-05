//! quicky-annotate: draw arrows, shapes, text, and callouts on an image,
//! saved as a copy.
//!
//!     quicky-annotate FILE
//!
//! Pick a tool and drag on the picture; drag an annotation to move it.
//! Ctrl+Z undoes, Delete removes the selected one. The original is never
//! touched; the result lands next to it as `name-annotated.ext`.

use gtk::{cairo, gdk, gdk_pixbuf::Pixbuf, glib, prelude::*};
use quicky_ui::{
    Frame, Palette, app, canvas,
    draw::{self, Rgb},
    media,
};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.annotate";

/// Space kept around the picture inside the drawing area.
const PAD: f64 = 8.0;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Arrow,
    Line,
    Text,
    Rect,
    Ellipse,
    Pen,
    Number,
    Magnify,
}

/// Toolbar: tool, icon (apps/icons, installed into hicolor), tooltip.
const TOOLS: [(Tool, &str, &str); 9] = [
    (Tool::Select, "quicky-select-symbolic", "Select and move"),
    (Tool::Arrow, "quicky-arrow-symbolic", "Arrow: drag from the tail toward the head"),
    (Tool::Line, "quicky-line-symbolic", "Line"),
    (Tool::Text, "quicky-text-symbolic", "Text: click where it goes"),
    (Tool::Rect, "quicky-rect-symbolic", "Box"),
    (Tool::Ellipse, "quicky-ellipse-symbolic", "Ellipse"),
    (Tool::Pen, "quicky-pen-symbolic", "Pencil"),
    (Tool::Number, "quicky-number-symbolic", "Numbered label: click to place the next number"),
    (Tool::Magnify, "quicky-magnify-symbolic", "Magnifier: drag out a loupe"),
];

const COLORS: [&str; 6] = ["#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#0a84ff", "#ffffff"];
const SIZES: [(&str, f64); 3] = [("S", 0.6), ("M", 1.0), ("L", 1.8)];

#[derive(Clone, PartialEq)]
enum Shape {
    Arrow(Pt, Pt),
    Line(Pt, Pt),
    Rect(Pt, Pt),
    Ellipse(Pt, Pt),
    Pen(Vec<Pt>),
    /// Top-left corner and the text.
    Text(Pt, String),
    Number(Pt, u32),
    /// Centre and edge of the loupe.
    Magnify(Pt, Pt),
}

type Pt = (f64, f64);

#[derive(Clone, PartialEq)]
struct Mark {
    shape: Shape,
    color: Rgb,
    /// Line width in image pixels; text and labels scale from it.
    width: f64,
}

impl Mark {
    /// Text size in image pixels.
    fn font(&self) -> f64 {
        self.width * 8.0
    }

    fn bounds(&self) -> (f64, f64, f64, f64) {
        let pts: Vec<Pt> = match &self.shape {
            Shape::Arrow(a, b) | Shape::Line(a, b) | Shape::Rect(a, b) | Shape::Ellipse(a, b) => vec![*a, *b],
            Shape::Pen(p) => p.clone(),
            Shape::Text(a, t) => {
                let size = self.font();
                // An empty box still has room for the caret.
                let w = text_width(t, size).max(size * 0.6);
                vec![*a, (a.0 + w, a.1 + size * 1.2)]
            }
            Shape::Number(a, _) => {
                let r = self.width * 4.0;
                vec![(a.0 - r, a.1 - r), (a.0 + r, a.1 + r)]
            }
            Shape::Magnify(c, e) => {
                let r = dist(*c, *e);
                vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)]
            }
        };
        let (mut l, mut t, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in pts {
            l = l.min(x);
            t = t.min(y);
            r = r.max(x);
            b = b.max(y);
        }
        (l, t, r, b)
    }

    fn hit(&self, p: Pt, slack: f64) -> bool {
        let (l, t, r, b) = self.bounds();
        p.0 > l - slack && p.0 < r + slack && p.1 > t - slack && p.1 < b + slack
    }

    fn shifted(&self, dx: f64, dy: f64) -> Mark {
        let m = |p: &Pt| (p.0 + dx, p.1 + dy);
        let shape = match &self.shape {
            Shape::Arrow(a, b) => Shape::Arrow(m(a), m(b)),
            Shape::Line(a, b) => Shape::Line(m(a), m(b)),
            Shape::Rect(a, b) => Shape::Rect(m(a), m(b)),
            Shape::Ellipse(a, b) => Shape::Ellipse(m(a), m(b)),
            Shape::Pen(p) => Shape::Pen(p.iter().map(m).collect()),
            Shape::Text(a, s) => Shape::Text(m(a), s.clone()),
            Shape::Number(a, n) => Shape::Number(m(a), *n),
            // The loupe moves; what it magnifies moves with it.
            Shape::Magnify(c, e) => Shape::Magnify(m(c), m(e)),
        };
        Mark { shape, ..self.clone() }
    }
}

fn dist(a: Pt, b: Pt) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn text_font(cr: &cairo::Context, size: f64) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(size);
}

/// How wide `text` draws at `size`.
fn text_width(text: &str, size: f64) -> f64 {
    let Ok(surface) = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1) else { return 0.0 };
    let Ok(cr) = cairo::Context::new(&surface) else { return 0.0 };
    text_font(&cr, size);
    cr.text_extents(text).map(|e| e.x_advance()).unwrap_or(0.0)
}

struct State {
    image: Pixbuf,
    marks: Vec<Mark>,
    history: Vec<Vec<Mark>>,
    selected: Option<usize>,
    tool: Tool,
    color: Rgb,
    width: f64,
    /// While dragging: the mark being drawn or moved, and where the drag
    /// started (image pixels).
    drag: Option<(usize, Pt, Mark)>,
    /// The drag is the text box's corner handle: it scales the text.
    resizing: bool,
    /// The text being typed into, if any, and whether its caret shows.
    editing: Option<usize>,
    caret: bool,
    cache: Option<Pixbuf>,
}

impl State {
    fn view(&self, w: f64, h: f64) -> (f64, f64, f64) {
        let (iw, ih) = (self.image.width() as f64, self.image.height() as f64);
        let s = ((w - 2.0 * PAD) / iw).min((h - 2.0 * PAD) / ih).max(1e-6);
        (s, (w - iw * s) / 2.0, (h - ih * s) / 2.0)
    }

    fn remember(&mut self) {
        self.history.push(self.marks.clone());
    }

    fn next_number(&self) -> u32 {
        self.marks.iter().filter(|m| matches!(m.shape, Shape::Number(..))).count() as u32 + 1
    }

    /// Stops typing; a box left empty goes away.
    fn commit(&mut self) {
        if let Some(i) = self.editing.take()
            && matches!(&self.marks.get(i).map(|m| &m.shape), Some(Shape::Text(_, t)) if t.trim().is_empty())
        {
            self.marks.remove(i);
            self.history.pop();
            self.selected = None;
        }
    }

    /// The text box's outline in image pixels, padded like it is drawn.
    fn text_frame(&self, i: usize, scale: f64) -> Option<(f64, f64, f64, f64)> {
        let m = self.marks.get(i)?;
        let (l, t, r, b) = m.bounds();
        let pad = 6.0 / scale;
        Some((l - pad, t - pad, r + pad, b + pad))
    }
}

fn main() -> glib::ExitCode {
    app::run_one(APP_ID, "Annotate Photo", &["image/*"], build)
}

fn build(app: &gtk::Application, path: PathBuf) {
    let palette = Palette::load();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let Some(image) = canvas::load_image(&path) else {
        media::notify("Can't annotate", &format!("{name} isn't an image Quicky can open"));
        return;
    };
    // Medium lines are about 0.6% of the long edge.
    let unit = (image.width().max(image.height()) as f64 / 160.0).max(2.0);

    let frame = Frame::new(app, "Annotate Photo", &name, 720, "Save Annotated Copy");
    let reset = frame.reset_button();
    let body = &frame.body;

    let state = Rc::new(RefCell::new(State {
        image,
        marks: Vec::new(),
        history: Vec::new(),
        selected: None,
        tool: Tool::Arrow,
        color: draw::hex(COLORS[0]),
        width: unit,
        drag: None,
        resizing: false,
        editing: None,
        caret: true,
        cache: None,
    }));

    // Toolbar.
    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let mut tool_buttons: Vec<gtk::ToggleButton> = Vec::new();
    for (tool, icon, tip) in TOOLS {
        let b = gtk::ToggleButton::builder().icon_name(icon).tooltip_text(tip).hexpand(true).build();
        b.add_css_class("compact");
        if let Some(first) = tool_buttons.first() {
            b.set_group(Some(first));
        }
        b.set_active(tool == Tool::Arrow);
        tools.append(&b);
        tool_buttons.push(b);
    }
    body.append(&tools);

    let area = gtk::DrawingArea::builder().hexpand(true).vexpand(true).build();
    body.append(&area);

    // Colour, size, undo, delete.
    let style = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let mut color_buttons: Vec<gtk::ToggleButton> = Vec::new();
    for c in COLORS {
        let dot = gtk::DrawingArea::builder().content_width(18).content_height(18).build();
        let rgb = draw::hex(c);
        dot.set_draw_func(move |_, cr, w, h| {
            draw::set(cr, rgb, 1.0);
            cr.arc(w as f64 / 2.0, h as f64 / 2.0, 7.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
        let b = gtk::ToggleButton::builder().child(&dot).build();
        b.add_css_class("flat");
        b.add_css_class("compact");
        if let Some(first) = color_buttons.first() {
            b.set_group(Some(first));
        }
        style.append(&b);
        color_buttons.push(b);
    }
    color_buttons[0].set_active(true);
    let (sizes, size_buttons) = Frame::choices(&SIZES.map(|(l, _)| l));
    sizes.set_hexpand(false);
    for b in &size_buttons {
        b.set_hexpand(false);
        b.add_css_class("compact");
        b.set_width_request(28);
    }
    size_buttons[1].set_active(true);
    style.append(&gtk::Box::builder().hexpand(true).build());
    style.append(&sizes);
    body.append(&style);

    // The hint, with Undo and Delete beside it.
    let hint = gtk::Label::builder()
        .label("Drag from the tail toward the arrowhead. Drag an annotation to move it.")
        .xalign(0.0)
        .wrap(true)
        .hexpand(true)
        .build();
    hint.add_css_class("dim");
    let undo = gtk::Button::builder().label("Undo").tooltip_text("Undo  Ctrl+Z").valign(gtk::Align::Center).build();
    let delete = gtk::Button::builder().label("Delete").tooltip_text("Delete the selected annotation  Del").valign(gtk::Align::Center).build();
    let edit_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    edit_row.append(&hint);
    edit_row.append(&undo);
    edit_row.append(&delete);
    body.append(&edit_row);

    // ---------------------------------------------------------------- drawing

    {
        let (state, accent) = (state.clone(), palette.accent.clone());
        area.set_draw_func(move |area, cr, w, h| {
            let factor = area.scale_factor().max(1) as f64;
            let mut s = state.borrow_mut();
            let (scale, ox, oy) = s.view(w as f64, h as f64);
            let (iw, ih) = (s.image.width() as f64, s.image.height() as f64);
            let (vw, vh) = ((iw * scale * factor).round().max(1.0) as i32, (ih * scale * factor).round().max(1.0) as i32);
            if !s.cache.as_ref().is_some_and(|c| c.width() == vw && c.height() == vh) {
                s.cache = s.image.scale_simple(vw, vh, gtk::gdk_pixbuf::InterpType::Bilinear);
            }
            cr.translate(ox, oy);
            if let Some(c) = &s.cache {
                cr.save().ok();
                cr.scale(1.0 / factor, 1.0 / factor);
                cr.set_source_pixbuf(c, 0.0, 0.0);
                let _ = cr.paint();
                cr.restore().ok();
            }
            cr.scale(scale, scale);
            let drawing = s.drag.as_ref().map(|(i, _, _)| *i);
            for m in &s.marks {
                paint(cr, &s.image, m);
            }
            let accent = draw::hex(&accent);
            if let Some(i) = s.editing
                && let (Some(m), Some((l, t, r, b))) = (s.marks.get(i), s.text_frame(i, scale))
            {
                // The text box being typed in: an outline with grips on its
                // sides, a resize handle in the corner, and the caret.
                draw::set(cr, accent, 1.0);
                cr.set_line_width(1.5 / scale);
                draw::rounded(cr, l, t, r - l, b - t, 8.0 / scale);
                let _ = cr.stroke();
                for (x, y) in [(l, (t + b) / 2.0), (r, (t + b) / 2.0)] {
                    cr.arc(x, y, 5.0 / scale, 0.0, std::f64::consts::TAU);
                    draw::set(cr, accent, 1.0);
                    let _ = cr.fill_preserve();
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    let _ = cr.stroke();
                }
                let k = 7.0 / scale;
                draw::set(cr, accent, 1.0);
                cr.rectangle(r - k / 2.0, b - k / 2.0, k, k);
                let _ = cr.fill_preserve();
                cr.set_source_rgb(1.0, 1.0, 1.0);
                let _ = cr.stroke();
                if s.caret
                    && let Shape::Text(a, text) = &m.shape
                {
                    let x = a.0 + text_width(text, m.font()) + m.width * 0.5;
                    draw::set(cr, m.color, 1.0);
                    cr.set_line_width((m.width * 0.5).max(1.5 / scale));
                    cr.move_to(x, a.1 + m.font() * 0.1);
                    cr.line_to(x, a.1 + m.font() * 1.1);
                    let _ = cr.stroke();
                }
            } else if let Some(i) = s.selected.filter(|_| drawing.is_none())
                && let Some(m) = s.marks.get(i)
            {
                // The selection: a dashed box around it.
                let (l, t, r, b) = m.bounds();
                let pad = 6.0 / scale;
                draw::set(cr, accent, 1.0);
                cr.set_line_width(1.5 / scale);
                cr.set_dash(&[4.0 / scale, 3.0 / scale], 0.0);
                cr.rectangle(l - pad, t - pad, r - l + 2.0 * pad, b - t + 2.0 * pad);
                let _ = cr.stroke();
            }
        });
    }

    // The caret blinks while typing.
    {
        let (state, area) = (state.clone(), area.clone());
        glib::timeout_add_local(std::time::Duration::from_millis(530), move || {
            let mut s = state.borrow_mut();
            if s.editing.is_some() {
                s.caret = !s.caret;
                drop(s);
                area.queue_draw();
            }
            glib::ControlFlow::Continue
        });
    }

    // ---------------------------------------------------------------- input

    let to_image = {
        let (state, area) = (state.clone(), area.clone());
        move |x: f64, y: f64| {
            let s = state.borrow();
            let (scale, ox, oy) = s.view(area.width() as f64, area.height() as f64);
            ((x - ox) / scale, (y - oy) / scale)
        }
    };

    let gesture = gtk::GestureDrag::new();
    {
        let (state, area, to_image, typing) = (state.clone(), area.clone(), to_image.clone(), frame.typing.clone());
        gesture.connect_drag_begin(move |_, x, y| {
            let p = to_image(x, y);
            let mut s = state.borrow_mut();
            let (scale, _, _) = s.view(area.width() as f64, area.height() as f64);
            let slack = 8.0 / scale;
            s.caret = true;

            // The text box's corner handle scales the text.
            if let Some(i) = s.editing
                && let Some((_, _, r, b)) = s.text_frame(i, scale)
                && dist(p, (r, b)) < 10.0 / scale
            {
                s.remember();
                let orig = s.marks[i].clone();
                s.drag = Some((i, p, orig));
                s.resizing = true;
                drop(s);
                area.queue_draw();
                return;
            }

            // Pressing an annotation picks it up, whatever the tool.
            let hit = (0..s.marks.len()).rev().find(|&i| s.marks[i].hit(p, slack));
            if s.editing.is_some() && hit != s.editing {
                s.commit();
            }
            let hit = (0..s.marks.len()).rev().find(|&i| s.marks[i].hit(p, slack));
            if let Some(i) = hit.filter(|_| s.tool != Tool::Pen) {
                if s.editing != Some(i) {
                    s.remember();
                }
                s.selected = Some(i);
                // Text picked up with the Text tool can be typed into again.
                if s.tool == Tool::Text && matches!(s.marks[i].shape, Shape::Text(..)) {
                    s.editing = Some(i);
                }
                let orig = s.marks[i].clone();
                s.drag = Some((i, p, orig));
                let editing = s.editing.is_some();
                drop(s);
                typing.set(editing);
                area.queue_draw();
                return;
            }
            s.selected = None;
            let (color, width) = (s.color, s.width);
            let shape = match s.tool {
                Tool::Select => None,
                Tool::Arrow => Some(Shape::Arrow(p, p)),
                Tool::Line => Some(Shape::Line(p, p)),
                Tool::Rect => Some(Shape::Rect(p, p)),
                Tool::Ellipse => Some(Shape::Ellipse(p, p)),
                Tool::Pen => Some(Shape::Pen(vec![p])),
                Tool::Magnify => Some(Shape::Magnify(p, p)),
                Tool::Number => {
                    let n = s.next_number();
                    s.remember();
                    s.marks.push(Mark { shape: Shape::Number(p, n), color, width });
                    s.selected = Some(s.marks.len() - 1);
                    None
                }
                Tool::Text => {
                    // A fresh, empty box with the caret where the click was.
                    s.remember();
                    let corner = (p.0, p.1 - width * 8.0 * 0.6);
                    s.marks.push(Mark { shape: Shape::Text(corner, String::new()), color, width });
                    let i = s.marks.len() - 1;
                    s.selected = Some(i);
                    s.editing = Some(i);
                    None
                }
            };
            if let Some(shape) = shape {
                s.remember();
                let mark = Mark { shape, color, width };
                s.marks.push(mark.clone());
                let i = s.marks.len() - 1;
                s.drag = Some((i, p, mark));
            }
            let editing = s.editing.is_some();
            drop(s);
            typing.set(editing);
            area.queue_draw();
        });
    }
    {
        let (state, area, to_image) = (state.clone(), area.clone(), to_image.clone());
        gesture.connect_drag_update(move |g, dx, dy| {
            let Some((x0, y0)) = g.start_point() else { return };
            let p = to_image(x0 + dx, y0 + dy);
            let mut s = state.borrow_mut();
            let Some((i, start, orig)) = s.drag.clone() else { return };
            if s.resizing {
                // Scale the text with the handle's distance from the corner.
                let (l, t, r, b) = orig.bounds();
                let k = (((p.0 - l) / (r - l).max(1.0)).max((p.1 - t) / (b - t).max(1.0))).clamp(0.2, 8.0);
                s.marks[i].width = orig.width * k;
                drop(s);
                area.queue_draw();
                return;
            }
            let moving = s.selected == Some(i) && s.marks.get(i).is_some() && !is_fresh(&orig, start);
            s.marks[i] = if moving {
                orig.shifted(p.0 - start.0, p.1 - start.1)
            } else {
                let shape = match &s.marks[i].shape {
                    Shape::Arrow(a, _) => Shape::Arrow(*a, p),
                    Shape::Line(a, _) => Shape::Line(*a, p),
                    Shape::Rect(a, _) => Shape::Rect(*a, p),
                    Shape::Ellipse(a, _) => Shape::Ellipse(*a, p),
                    Shape::Magnify(c, _) => Shape::Magnify(*c, p),
                    Shape::Pen(pts) => {
                        let mut pts = pts.clone();
                        pts.push(p);
                        Shape::Pen(pts)
                    }
                    other => other.clone(),
                };
                Mark { shape, ..s.marks[i].clone() }
            };
            drop(s);
            area.queue_draw();
        });
    }
    {
        let (state, area) = (state.clone(), area.clone());
        gesture.connect_drag_end(move |_, _, _| {
            let mut s = state.borrow_mut();
            s.resizing = false;
            if let Some((i, start, orig)) = s.drag.take() {
                // A click on an annotation that didn't move it isn't an edit.
                if s.marks.get(i) == Some(&orig) && s.editing != Some(i) && !is_fresh(&orig, start) {
                    s.history.pop();
                }
                // A click that drew nothing leaves a dot; drop it.
                let (l, t, r, b) = s.marks[i].bounds();
                let tiny = (r - l).max(b - t) < s.width * 2.0;
                if tiny && !matches!(s.marks[i].shape, Shape::Number(..) | Shape::Text(..)) {
                    s.marks.remove(i);
                    s.history.pop();
                    s.selected = None;
                } else {
                    s.selected = Some(i);
                }
            }
            drop(s);
            area.queue_draw();
        });
    }
    area.add_controller(gesture);

    // ---------------------------------------------------------------- wiring

    let hints = [
        "Click an annotation to select it; drag to move it.",
        "Drag from the tail toward the arrowhead. Drag an annotation to move it.",
        "Drag to draw a line. Drag an annotation to move it.",
        "Click where the text goes, type, then Enter.",
        "Drag to draw a box. Drag an annotation to move it.",
        "Drag to draw an ellipse. Drag an annotation to move it.",
        "Draw freely.",
        "Click to place the next number.",
        "Drag out from the spot to magnify.",
    ];
    // Finishes any text being typed.
    let stop_typing = {
        let (state, area, typing) = (state.clone(), area.clone(), frame.typing.clone());
        move || {
            state.borrow_mut().commit();
            typing.set(false);
            area.queue_draw();
        }
    };
    for (i, b) in tool_buttons.iter().enumerate() {
        let (state, hint, stop_typing) = (state.clone(), hint.clone(), stop_typing.clone());
        b.connect_toggled(move |b| {
            if b.is_active() {
                stop_typing();
                state.borrow_mut().tool = TOOLS[i].0;
                hint.set_text(hints[i]);
            }
        });
    }
    // Changing colour or size restyles the selected annotation too.
    let restyle = {
        let (state, area) = (state.clone(), area.clone());
        move |color: Option<Rgb>, width: Option<f64>| {
            let mut s = state.borrow_mut();
            if let Some(c) = color {
                s.color = c;
            }
            if let Some(w) = width {
                s.width = w;
            }
            if let Some(i) = s.selected {
                s.remember();
                let (c, w) = (s.color, s.width);
                s.marks[i].color = c;
                s.marks[i].width = w;
            }
            drop(s);
            area.queue_draw();
        }
    };
    for (i, b) in color_buttons.iter().enumerate() {
        let restyle = restyle.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                restyle(Some(draw::hex(COLORS[i])), None);
            }
        });
    }
    for (i, b) in size_buttons.iter().enumerate() {
        let restyle = restyle.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                restyle(None, Some(unit * SIZES[i].1));
            }
        });
    }
    let do_undo = {
        let (state, area, typing) = (state.clone(), area.clone(), frame.typing.clone());
        move || {
            let mut s = state.borrow_mut();
            s.editing = None;
            if let Some(prev) = s.history.pop() {
                s.marks = prev;
                s.selected = None;
            }
            drop(s);
            typing.set(false);
            area.queue_draw();
        }
    };
    let do_delete = {
        let (state, area, typing) = (state.clone(), area.clone(), frame.typing.clone());
        move || {
            let mut s = state.borrow_mut();
            s.editing = None;
            if let Some(i) = s.selected.take() {
                s.remember();
                s.marks.remove(i);
            }
            drop(s);
            typing.set(false);
            area.queue_draw();
        }
    };
    {
        let do_undo = do_undo.clone();
        undo.connect_clicked(move |_| do_undo());
    }
    {
        let do_delete = do_delete.clone();
        delete.connect_clicked(move |_| do_delete());
    }
    {
        let (state, area) = (state.clone(), area.clone());
        reset.connect_clicked(move |_| {
            let mut s = state.borrow_mut();
            if !s.marks.is_empty() {
                s.remember();
                s.marks.clear();
                s.selected = None;
            }
            drop(s);
            area.queue_draw();
        });
    }

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let (window, state, area) = (frame.window.clone(), state.clone(), area.clone());
        keys.connect_key_pressed(move |_, key, _, mods| {
            use gdk::Key;
            if GtkWindowExt::focus(&window).is_some_and(|w| w.is::<gtk::Text>()) {
                return glib::Propagation::Proceed;
            }
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            // Typing on the picture: keys go into the text box.
            let editing = state.borrow().editing;
            if let Some(i) = editing
                && !ctrl
            {
                match key {
                    Key::Escape | Key::Return | Key::KP_Enter => stop_typing(),
                    _ => {
                        let mut s = state.borrow_mut();
                        let Some(Shape::Text(_, text)) = s.marks.get_mut(i).map(|m| &mut m.shape) else { return glib::Propagation::Proceed };
                        match key {
                            Key::BackSpace => {
                                text.pop();
                            }
                            _ => match key.to_unicode().filter(|c| !c.is_control()) {
                                Some(c) => text.push(c),
                                None => return glib::Propagation::Proceed,
                            },
                        }
                        s.caret = true;
                        drop(s);
                        area.queue_draw();
                    }
                }
                return glib::Propagation::Stop;
            }
            match key {
                Key::z | Key::Z if mods.contains(gdk::ModifierType::CONTROL_MASK) => do_undo(),
                Key::Delete | Key::BackSpace | Key::KP_Delete => do_delete(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
    }
    frame.window.add_controller(keys);

    {
        let (frame, state) = (frame.clone(), state.clone());
        frame.primary.clone().connect_clicked(move |_| {
            let marks = state.borrow().marks.clone();
            if marks.is_empty() {
                frame.status.set_text("Nothing drawn yet");
                return;
            }
            let out = media::output_path(&path, "-annotated", media::image_out_ext(&path));
            let src = path.clone();
            app::finish(&frame, "Saving…", "Annotated", "Save failed", out, move |dst| {
                let Some(image) = canvas::load_image(&src) else { return false };
                let Ok(mut surface) = cairo::ImageSurface::create(cairo::Format::ARgb32, image.width(), image.height()) else { return false };
                {
                    let Ok(cr) = cairo::Context::new(&surface) else { return false };
                    cr.set_source_pixbuf(&image, 0.0, 0.0);
                    let _ = cr.paint();
                    for m in &marks {
                        paint(&cr, &image, m);
                    }
                }
                media::save_surface(&mut surface, dst)
            });
        });
    }

    frame.window.present();
}

/// Whether `orig` is a mark that has only just been started at `start`
/// (so dragging shapes it rather than moves it).
fn is_fresh(orig: &Mark, start: Pt) -> bool {
    match &orig.shape {
        Shape::Arrow(a, b) | Shape::Line(a, b) | Shape::Rect(a, b) | Shape::Ellipse(a, b) | Shape::Magnify(a, b) => *a == start && *b == start,
        Shape::Pen(p) => p.len() == 1 && p[0] == start,
        _ => false,
    }
}

/// Draws one annotation in image coordinates.
fn paint(cr: &cairo::Context, image: &Pixbuf, m: &Mark) {
    let w = m.width;
    draw::set(cr, m.color, 1.0);
    cr.set_line_width(w);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_dash(&[], 0.0);
    match &m.shape {
        Shape::Line(a, b) => {
            cr.move_to(a.0, a.1);
            cr.line_to(b.0, b.1);
            let _ = cr.stroke();
        }
        Shape::Arrow(a, b) => {
            let ang = (b.1 - a.1).atan2(b.0 - a.0);
            let head = w * 4.5;
            // Stop the shaft short so its round cap hides under the head.
            let tip = (b.0 - ang.cos() * head * 0.6, b.1 - ang.sin() * head * 0.6);
            cr.move_to(a.0, a.1);
            cr.line_to(tip.0, tip.1);
            let _ = cr.stroke();
            cr.move_to(b.0, b.1);
            cr.line_to(b.0 - head * (ang - 0.45).cos(), b.1 - head * (ang - 0.45).sin());
            cr.line_to(b.0 - head * (ang + 0.45).cos(), b.1 - head * (ang + 0.45).sin());
            cr.close_path();
            let _ = cr.fill();
        }
        Shape::Rect(a, b) => {
            draw::rounded(cr, a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs(), w);
            let _ = cr.stroke();
        }
        Shape::Ellipse(a, b) => {
            let (cx, cy, rx, ry) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, ((b.0 - a.0) / 2.0).abs().max(1.0), ((b.1 - a.1) / 2.0).abs().max(1.0));
            cr.save().ok();
            cr.translate(cx, cy);
            cr.scale(rx, ry);
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            cr.restore().ok();
            let _ = cr.stroke();
        }
        Shape::Pen(pts) => {
            if let Some(first) = pts.first() {
                cr.move_to(first.0, first.1);
                for p in &pts[1..] {
                    cr.line_to(p.0, p.1);
                }
                let _ = cr.stroke();
            }
        }
        Shape::Text(a, text) => {
            text_font(cr, m.font());
            // A dark outline keeps light text readable on light pictures.
            // `a` is the box's top; the baseline sits a line's ascent below.
            cr.move_to(a.0, a.1 + m.font() * 0.95);
            cr.text_path(text);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
            cr.set_line_width(w * 0.8);
            let _ = cr.stroke_preserve();
            draw::set(cr, m.color, 1.0);
            let _ = cr.fill();
        }
        Shape::Number(a, n) => {
            let r = w * 4.0;
            cr.arc(a.0, a.1, r, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            let light = m.color.0 * 0.3 + m.color.1 * 0.59 + m.color.2 * 0.11 > 0.7;
            if light { cr.set_source_rgb(0.0, 0.0, 0.0) } else { cr.set_source_rgb(1.0, 1.0, 1.0) }
            cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(r * 1.2);
            let label = n.to_string();
            if let Ok(ext) = cr.text_extents(&label) {
                cr.move_to(a.0 - ext.width() / 2.0 - ext.x_bearing(), a.1 - ext.height() / 2.0 - ext.y_bearing());
                let _ = cr.show_text(&label);
            }
        }
        Shape::Magnify(c, e) => {
            let r = dist(*c, *e).max(1.0);
            // The picture around the centre at twice the size, in a circle.
            cr.save().ok();
            cr.arc(c.0, c.1, r, 0.0, std::f64::consts::TAU);
            cr.clip();
            cr.translate(c.0, c.1);
            cr.scale(2.0, 2.0);
            cr.translate(-c.0, -c.1);
            cr.set_source_pixbuf(image, 0.0, 0.0);
            let _ = cr.paint();
            cr.restore().ok();
            cr.arc(c.0, c.1, r, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
        }
    }
}
