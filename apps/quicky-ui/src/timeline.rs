//! A timeline strip: a filmstrip or waveform backdrop with a playhead, and
//! one of three ways to mark time on it. Shared by the trim, split,
//! snapshot, redact-video, and bleep tools.

use crate::theme::Palette;
use gtk::{gdk, gdk_pixbuf::Pixbuf, prelude::*};
use std::{cell::RefCell, rc::Rc};

/// How close (px) a press must be to a range end to grab it.
const GRAB: f64 = 16.0;
/// Shortest range the handles allow, in seconds.
const MIN_SPAN: f64 = 0.1;
/// Height of the regions layer above the backdrop.
const LAYER: f64 = 22.0;

/// What the strip edits.
#[derive(Clone, Copy, PartialEq)]
pub enum Marks {
    /// One selected range with thick draggable ends (trim, a redaction).
    Range,
    /// Only the playhead, plus fixed markers (split points, snapshots).
    Points,
    /// Any number of ranges in a layer above the backdrop (bleeps): drag
    /// on empty layer to add one, drag one or its edges to adjust it.
    Regions,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Start,
    End,
    Head,
    /// A region: which one, which part, and where it was.
    Region { i: usize, part: Part, orig: (f64, f64) },
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Start,
    End,
    Body,
}

struct State {
    marks: Marks,
    duration: f64,
    start: f64,
    end: f64,
    playhead: f64,
    markers: Vec<f64>,
    regions: Vec<(f64, f64)>,
    selected: Option<usize>,
    backdrop: Option<Pixbuf>,
    palette: Palette,
    drag: Drag,
    drag_origin: f64,
    on_range: Option<Rc<dyn Fn(f64, f64, bool)>>,
    on_seek: Option<Rc<dyn Fn(f64)>>,
    on_regions: Option<Rc<dyn Fn()>>,
}

#[derive(Clone)]
pub struct Timeline {
    area: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
}

impl Timeline {
    pub fn new(duration: f64, palette: Palette, height: i32) -> Self {
        Self::with_marks(duration, palette, height, Marks::Range)
    }

    pub fn with_marks(duration: f64, palette: Palette, height: i32, marks: Marks) -> Self {
        let area = gtk::DrawingArea::builder()
            .content_height(height)
            .hexpand(true)
            .build();
        let state = Rc::new(RefCell::new(State {
            marks,
            duration: duration.max(0.001),
            start: 0.0,
            end: duration.max(0.001),
            playhead: 0.0,
            markers: Vec::new(),
            regions: Vec::new(),
            selected: None,
            backdrop: None,
            palette,
            drag: Drag::None,
            drag_origin: 0.0,
            on_range: None,
            on_seek: None,
            on_regions: None,
        }));
        let timeline = Timeline { area, state };
        timeline.connect_draw();
        timeline.connect_drag();
        timeline
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub fn range(&self) -> (f64, f64) {
        let s = self.state.borrow();
        (s.start, s.end)
    }

    /// Sets the selection, clamped to the media. Doesn't fire `on_range`.
    pub fn set_range(&self, start: f64, end: f64) {
        {
            let mut s = self.state.borrow_mut();
            let d = s.duration;
            s.start = start.clamp(0.0, d - MIN_SPAN);
            s.end = end.clamp(s.start + MIN_SPAN, d);
        }
        self.area.queue_draw();
    }

    pub fn set_playhead(&self, t: f64) {
        self.state.borrow_mut().playhead = t;
        self.area.queue_draw();
    }

    pub fn set_backdrop(&self, image: Option<Pixbuf>) {
        self.state.borrow_mut().backdrop = image;
        self.area.queue_draw();
    }

    /// Fixed markers drawn across the strip (`Points`).
    pub fn set_markers(&self, markers: Vec<f64>) {
        self.state.borrow_mut().markers = markers;
        self.area.queue_draw();
    }

    /// The regions (`Regions`), sorted by start.
    pub fn regions(&self) -> Vec<(f64, f64)> {
        let mut r = self.state.borrow().regions.clone();
        r.sort_by(|a, b| a.0.total_cmp(&b.0));
        r
    }

    pub fn clear_regions(&self) {
        {
            let mut s = self.state.borrow_mut();
            s.regions.clear();
            s.selected = None;
        }
        self.regions_changed();
    }

    /// Removes the selected region. Returns whether one went.
    pub fn remove_selected(&self) -> bool {
        let removed = {
            let mut s = self.state.borrow_mut();
            match s.selected.take() {
                Some(i) if i < s.regions.len() => {
                    s.regions.remove(i);
                    true
                }
                _ => false,
            }
        };
        if removed {
            self.regions_changed();
        }
        removed
    }

    /// Fires while a range end is dragged: `(start, end, moving_start)`.
    pub fn on_range(&self, f: impl Fn(f64, f64, bool) + 'static) {
        self.state.borrow_mut().on_range = Some(Rc::new(f));
    }

    /// Fires when the strip is clicked or the playhead dragged.
    pub fn on_seek(&self, f: impl Fn(f64) + 'static) {
        self.state.borrow_mut().on_seek = Some(Rc::new(f));
    }

    /// Fires whenever a region is added, changed, or removed.
    pub fn on_regions(&self, f: impl Fn() + 'static) {
        self.state.borrow_mut().on_regions = Some(Rc::new(f));
    }

    fn regions_changed(&self) {
        self.area.queue_draw();
        let f = self.state.borrow().on_regions.clone();
        if let Some(f) = f {
            f()
        }
    }

    fn connect_draw(&self) {
        let state = self.state.clone();
        self.area.set_draw_func(move |_, cr, w, h| {
            let s = state.borrow();
            let (w, h) = (w as f64, h as f64);
            let x_of = |t: f64| t / s.duration * w;
            let set = |cr: &gtk::cairo::Context, c: gdk::RGBA| {
                cr.set_source_rgba(c.red().into(), c.green().into(), c.blue().into(), c.alpha().into())
            };
            let p = &s.palette;
            // Regions get their own layer on top; the backdrop sits below it.
            let top = if s.marks == Marks::Regions { LAYER + 4.0 } else { 0.0 };
            let bh = h - top;

            set(cr, Palette::rgba(&p.foreground, 0.05));
            cr.rectangle(0.0, top, w, bh);
            let _ = cr.fill();

            if let Some(img) = &s.backdrop {
                cr.save().ok();
                cr.translate(0.0, top);
                cr.scale(w / img.width() as f64, bh / img.height() as f64);
                cr.set_source_pixbuf(img, 0.0, 0.0);
                let _ = cr.paint();
                cr.restore().ok();
            }

            match s.marks {
                Marks::Range => {
                    // Dim everything outside the selection.
                    let (xs, xe) = (x_of(s.start), x_of(s.end));
                    set(cr, Palette::rgba(&p.background, 0.72));
                    cr.rectangle(0.0, 0.0, xs, h);
                    cr.rectangle(xe, 0.0, w - xe, h);
                    let _ = cr.fill();

                    // Selection frame and the two grab bars.
                    set(cr, Palette::rgba(&p.accent, 1.0));
                    cr.set_line_width(2.0);
                    cr.rectangle(xs + 1.0, 1.0, (xe - xs - 2.0).max(0.0), h - 2.0);
                    let _ = cr.stroke();
                    let bar = 14.0_f64.min(h / 3.0);
                    cr.rectangle(xs, 0.0, bar, h);
                    cr.rectangle(xe - bar, 0.0, bar, h);
                    let _ = cr.fill();
                    set(cr, Palette::rgba(&p.background, 0.9));
                    cr.set_line_width(1.5);
                    for x in [xs + bar / 2.0, xe - bar / 2.0] {
                        cr.move_to(x, h * 0.35);
                        cr.line_to(x, h * 0.65);
                    }
                    let _ = cr.stroke();
                }
                Marks::Points => {
                    set(cr, Palette::rgba(&p.accent, 1.0));
                    cr.set_line_width(2.0);
                    for &t in &s.markers {
                        let x = x_of(t);
                        cr.move_to(x, 0.0);
                        cr.line_to(x, h);
                        let _ = cr.stroke();
                        cr.move_to(x - 5.0, 0.0);
                        cr.line_to(x + 5.0, 0.0);
                        cr.line_to(x, 7.0);
                        cr.close_path();
                        let _ = cr.fill();
                    }
                }
                Marks::Regions => {
                    set(cr, Palette::rgba(&p.foreground, 0.08));
                    cr.rectangle(0.0, 0.0, w, LAYER);
                    let _ = cr.fill();
                    if s.regions.is_empty() {
                        set(cr, Palette::rgba(&p.foreground, 0.45));
                        cr.select_font_face("monospace", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
                        cr.set_font_size(11.0);
                        cr.move_to(8.0, LAYER / 2.0 + 4.0);
                        let _ = cr.show_text("Drag here to add a section");
                    }
                    for (i, &(a, b)) in s.regions.iter().enumerate() {
                        let (xa, xb) = (x_of(a), x_of(b));
                        let chosen = s.selected == Some(i);
                        // The section in the layer, and its shadow on the backdrop.
                        set(cr, Palette::rgba(&p.accent, if chosen { 1.0 } else { 0.75 }));
                        cr.rectangle(xa, 0.0, (xb - xa).max(2.0), LAYER);
                        let _ = cr.fill();
                        set(cr, Palette::rgba(&p.accent, 0.22));
                        cr.rectangle(xa, top, (xb - xa).max(2.0), bh);
                        let _ = cr.fill();
                        set(cr, Palette::rgba(&p.background, 0.9));
                        cr.set_line_width(1.5);
                        for x in [xa + 4.0, xb - 4.0] {
                            cr.move_to(x, LAYER * 0.3);
                            cr.line_to(x, LAYER * 0.7);
                        }
                        let _ = cr.stroke();
                    }
                }
            }

            // Playhead.
            let xp = x_of(s.playhead).clamp(1.0, w - 1.0);
            set(cr, Palette::rgba(&p.foreground, 0.95));
            cr.set_line_width(2.0);
            cr.move_to(xp, top);
            cr.line_to(xp, h);
            let _ = cr.stroke();
        });
    }

    fn connect_drag(&self) {
        let gesture = gtk::GestureDrag::new();

        let timeline = self.clone();
        gesture.connect_drag_begin(move |_, x, y| {
            let w = timeline.area.width() as f64;
            let t = {
                let mut s = timeline.state.borrow_mut();
                let t = (x / w * s.duration).clamp(0.0, s.duration);
                let (xs, xe) = (s.start / s.duration * w, s.end / s.duration * w);
                s.drag = match s.marks {
                    Marks::Range if (x - xs).abs() <= GRAB && (x - xs).abs() <= (x - xe).abs() => Drag::Start,
                    Marks::Range if (x - xe).abs() <= GRAB => Drag::End,
                    Marks::Regions if y <= LAYER + 4.0 => {
                        let found = region_at(&s.regions, x, w, s.duration);
                        let (i, part) = found.unwrap_or_else(|| {
                            s.regions.push((t, t));
                            (s.regions.len() - 1, Part::End)
                        });
                        s.selected = Some(i);
                        Drag::Region { i, part, orig: s.regions[i] }
                    }
                    _ => Drag::Head,
                };
                s.drag_origin = x;
                t
            };
            update(&timeline, t);
        });

        let timeline = self.clone();
        gesture.connect_drag_update(move |_, dx, _| {
            let w = timeline.area.width() as f64;
            let t = {
                let s = timeline.state.borrow();
                ((s.drag_origin + dx) / w * s.duration).clamp(0.0, s.duration)
            };
            update(&timeline, t);
        });

        let timeline = self.clone();
        gesture.connect_drag_end(move |_, _, _| {
            let changed = {
                let mut s = timeline.state.borrow_mut();
                let was = std::mem::replace(&mut s.drag, Drag::None);
                // A click on empty layer leaves a sliver; drop it.
                if let Drag::Region { i, .. } = was {
                    if s.regions.get(i).is_some_and(|(a, b)| b - a < MIN_SPAN) {
                        s.regions.remove(i);
                        s.selected = None;
                    }
                    true
                } else {
                    false
                }
            };
            if changed {
                timeline.regions_changed();
            }
        });

        self.area.add_controller(gesture);
    }
}

/// The region under `x` and the part of it grabbed.
fn region_at(regions: &[(f64, f64)], x: f64, w: f64, duration: f64) -> Option<(usize, Part)> {
    let x_of = |t: f64| t / duration * w;
    regions.iter().enumerate().rev().find_map(|(i, &(a, b))| {
        let (xa, xb) = (x_of(a), x_of(b));
        if (x - xa).abs() <= 6.0 {
            Some((i, Part::Start))
        } else if (x - xb).abs() <= 6.0 {
            Some((i, Part::End))
        } else {
            (x > xa && x < xb).then_some((i, Part::Body))
        }
    })
}

// Applies a drag to time `t`, then notifies with the state released, so
// callbacks may call back into the timeline.
fn update(timeline: &Timeline, t: f64) {
    let (drag, start, end, on_range, on_seek) = {
        let mut s = timeline.state.borrow_mut();
        let d = s.duration;
        match s.drag {
            Drag::Start => s.start = t.min(s.end - MIN_SPAN).max(0.0),
            Drag::End => s.end = t.max(s.start + MIN_SPAN).min(d),
            Drag::Head => s.playhead = t,
            Drag::Region { i, part, orig: (a, b) } => {
                let origin_t = s.drag_origin / timeline.area.width() as f64 * d;
                s.regions[i] = match part {
                    Part::Start => (t.min(b), b),
                    // A new region grows from where the press began.
                    Part::End if a == b => (a.min(t), a.max(t)),
                    Part::End => (a, t.max(a)),
                    Part::Body => {
                        let shift = (t - origin_t).clamp(-a, d - b);
                        (a + shift, b + shift)
                    }
                };
            }
            Drag::None => {}
        }
        (s.drag, s.start, s.end, s.on_range.clone(), s.on_seek.clone())
    };
    timeline.area.queue_draw();
    match drag {
        Drag::Start | Drag::End => {
            if let Some(f) = on_range {
                f(start, end, drag == Drag::Start)
            }
        }
        Drag::Head => {
            if let Some(f) = on_seek {
                f(t)
            }
        }
        Drag::Region { .. } | Drag::None => {}
    }
}
