//! The compact window every Quicky editor lives in: narrow and portrait, a
//! centred title with the file under it, the tool's content, then a footer
//! with an optional Reset on the left and one primary button on the right.
//! Esc closes the window; there is no separate Cancel.

use gtk::{glib, prelude::*};

/// Width of every editor window. Tall content scrolls or shrinks to fit.
pub const WIDTH: i32 = 420;

#[derive(Clone)]
pub struct Frame {
    pub window: gtk::ApplicationWindow,
    /// Where the tool puts its controls, top to bottom.
    pub body: gtk::Box,
    /// Short progress or error text, left of the buttons.
    pub status: gtk::Label,
    pub primary: gtk::Button,
    /// Set while the tool takes typing itself (text drawn on a picture), so
    /// Esc and Enter go to it instead of closing or saving.
    pub typing: std::rc::Rc<std::cell::Cell<bool>>,
    footer: gtk::Box,
}

impl Frame {
    /// `title` names the tool ("Trim Video"); `subject` is the file name or
    /// "3 files"; `primary` labels the button that does the work.
    pub fn new(app: &gtk::Application, title: &str, subject: &str, height: i32, primary: &str) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(format!("{title} — {subject}"))
            .default_width(WIDTH)
            .default_height(height)
            .build();

        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.set_margin_top(14);
        root.set_margin_bottom(14);
        root.set_margin_start(16);
        root.set_margin_end(16);

        let heading = gtk::Label::new(Some(title));
        heading.add_css_class("title");
        let file = gtk::Label::builder().label(subject).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        file.add_css_class("dim");
        let header = gtk::Box::new(gtk::Orientation::Vertical, 2);
        header.append(&heading);
        header.append(&file);
        root.append(&header);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        body.set_vexpand(true);
        root.append(&body);

        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let status = gtk::Label::builder().hexpand(true).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
        status.add_css_class("dim");
        let primary = gtk::Button::with_label(primary);
        primary.add_css_class("suggested-action");
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.append(&status);
        footer.append(&primary);
        root.append(&footer);

        window.set_child(Some(&root));

        // Esc closes; Enter presses the primary button. Capture phase, so a
        // focused button can't swallow Enter as its own click; while typing
        // in an entry, Enter stays the entry's.
        let typing_elsewhere = std::rc::Rc::new(std::cell::Cell::new(false));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let (window, primary, elsewhere) = (window.clone(), primary.clone(), typing_elsewhere.clone());
            keys.connect_key_pressed(move |_, key, _, _| {
                use gtk::gdk::Key;
                if elsewhere.get() {
                    return glib::Propagation::Proceed;
                }
                let typing = GtkWindowExt::focus(&window).is_some_and(|w| w.is::<gtk::Text>());
                match key {
                    Key::Escape => window.close(),
                    Key::Return | Key::KP_Enter if !typing && primary.is_sensitive() => primary.emit_clicked(),
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            });
        }
        window.add_controller(keys);

        Frame { window, body, status, primary, typing: typing_elsewhere, footer }
    }

    /// Adds a Reset button at the bottom-left.
    pub fn reset_button(&self) -> gtk::Button {
        let reset = gtk::Button::with_label("Reset");
        self.footer.prepend(&reset);
        reset
    }

    /// Adds a second action just left of the primary one (Remove All).
    pub fn secondary_button(&self, label: &str) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        self.footer.insert_child_after(&button, Some(&self.status));
        button
    }

    /// Disables the buttons and shows `text` while work runs.
    pub fn busy(&self, on: bool, text: &str) {
        let mut child = self.footer.first_child();
        while let Some(w) = child {
            if w.is::<gtk::Button>() {
                w.set_sensitive(!on);
            }
            child = w.next_sibling();
        }
        self.status.set_text(text);
    }

    /// A dim, left-aligned caption row: `left` text and a `right` readout.
    pub fn caption(left: &str) -> (gtk::Box, gtk::Label) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let l = gtk::Label::builder().label(left).xalign(0.0).hexpand(true).build();
        l.add_css_class("dim");
        let r = gtk::Label::new(None);
        r.add_css_class("dim");
        row.append(&l);
        row.append(&r);
        (row, r)
    }

    /// A small bold heading that starts a group of controls ("Light").
    pub fn section(text: &str) -> gtk::Label {
        let label = gtk::Label::builder().label(text).xalign(0.0).margin_top(4).build();
        label.add_css_class("section");
        label
    }

    /// A labelled slider with its value on the right and a ↺ that puts it
    /// back to `value`. `fmt` turns the value into its readout.
    pub fn slider(label: &str, range: (f64, f64), step: f64, value: f64, fmt: fn(f64) -> String) -> (gtk::Box, gtk::Scale) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let name = gtk::Label::builder().label(label).xalign(0.0).width_chars(12).max_width_chars(12).build();
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, range.0, range.1, step);
        scale.set_value(value);
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        let readout = gtk::Label::builder().label(fmt(value)).xalign(1.0).width_chars(9).build();
        readout.add_css_class("dim");
        let reset = gtk::Button::builder().label("↺").tooltip_text("Reset").build();
        reset.add_css_class("flat");
        reset.set_opacity(0.0);
        {
            let (readout, reset) = (readout.clone(), reset.clone());
            scale.connect_value_changed(move |s| {
                readout.set_text(&fmt(s.value()));
                reset.set_opacity(if (s.value() - value).abs() < step / 2.0 { 0.0 } else { 1.0 });
            });
        }
        {
            let scale = scale.clone();
            reset.connect_clicked(move |_| scale.set_value(value));
        }
        row.append(&name);
        row.append(&scale);
        row.append(&readout);
        row.append(&reset);
        (row, scale)
    }

    /// A row with a label on the left and `control` on the right.
    pub fn labelled(text: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::builder().label(text).xalign(0.0).hexpand(true).build();
        row.append(&label);
        row.append(control);
        row
    }

    /// A row of linked toggle buttons, one active at a time.
    pub fn choices(labels: &[&str]) -> (gtk::Box, Vec<gtk::ToggleButton>) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
        for label in labels {
            let b = gtk::ToggleButton::with_label(label);
            b.set_hexpand(true);
            if let Some(first) = buttons.first() {
                b.set_group(Some(first));
            }
            row.append(&b);
            buttons.push(b);
        }
        if let Some(first) = buttons.first() {
            first.set_active(true);
        }
        (row, buttons)
    }
}
