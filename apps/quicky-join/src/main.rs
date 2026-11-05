//! quicky-join: put several files in order and save them as one.
//!
//!     quicky-join --mode video|audio|pdf|images FILE...
//!
//! One window serves four tools: Join Videos (one MP4), Join Audio (one
//! M4A), Merge PDFs, and Create PDF from images (a page per image). Arrange
//! the order with ▲ ▼, then save. The result lands next to the first file.

use gtk::{
    gdk_pixbuf::{InterpType, Pixbuf},
    gio, glib,
    prelude::*,
};
use quicky_ui::{Frame, Palette, canvas, media};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

// Not org.omarchy.*: Omarchy tags that namespace as terminals.
const APP_ID: &str = "quicky.join";

/// Thumbnail height in the list.
const THUMB: i32 = 40;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Video,
    Audio,
    Pdf,
    Images,
}

impl Mode {
    fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "video" => Mode::Video,
            "audio" => Mode::Audio,
            "pdf" => Mode::Pdf,
            "images" => Mode::Images,
            _ => return None,
        })
    }

    /// Window title (also the button), list caption, output name and type.
    fn words(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Mode::Video => ("Join Videos", "Playback order", "joined", "mp4"),
            Mode::Audio => ("Join Audio", "Playback order", "joined", "m4a"),
            Mode::Pdf => ("Merge PDFs", "Document order", "merged", "pdf"),
            Mode::Images => ("Create PDF", "Page order", "images", "pdf"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Mode::Video => "video-x-generic-symbolic",
            Mode::Audio => "audio-x-generic-symbolic",
            Mode::Pdf => "x-office-document-symbolic",
            Mode::Images => "image-x-generic-symbolic",
        }
    }
}

fn main() -> glib::ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    let mut mode = None;
    if args.peek().map(String::as_str) == Some("--mode") {
        args.next();
        mode = args.next().as_deref().and_then(Mode::parse);
    }
    let files: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        Palette::load().install();
        let Some(mode) = mode else {
            media::notify("quicky-join", "Usage: quicky-join --mode video|audio|pdf|images FILE...");
            return;
        };
        if files.len() < 2 {
            media::notify("Nothing to join", "Choose two or more files");
            return;
        }
        build(app, mode, files.clone());
    });
    app.run_with_args::<&str>(&[])
}

fn build(app: &gtk::Application, mode: Mode, files: Vec<PathBuf>) {
    let (title, caption, out_name, out_ext) = mode.words();
    let frame = Frame::new(app, title, &format!("{} files", files.len()), 420, title);
    let body = &frame.body;

    let heading = gtk::Label::builder().label(caption).xalign(0.0).build();
    heading.add_css_class("dim");
    body.append(&heading);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    body.append(&scroller);

    let order = Rc::new(RefCell::new(files.clone()));
    let thumbs: Rc<RefCell<HashMap<PathBuf, Pixbuf>>> = Rc::default();

    // Rows are rebuilt on every move: lists here are a handful of files.
    let render: Rc<dyn Fn()> = {
        let (list, order, thumbs) = (list.clone(), order.clone(), thumbs.clone());
        let slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::default();
        let render: Rc<dyn Fn()> = {
            let slot = slot.clone();
            Rc::new(move || {
                while let Some(child) = list.first_child() {
                    list.remove(&child);
                }
                let files = order.borrow().clone();
                let rerender = slot.borrow().clone();
                for (i, file) in files.iter().enumerate() {
                    let row = row_for(i, file, files.len(), mode, thumbs.borrow().get(file));
                    let (up, down) = (row.1, row.2);
                    for (button, delta) in [(up, -1i32), (down, 1)] {
                        let (order, rerender) = (order.clone(), rerender.clone());
                        button.connect_clicked(move |_| {
                            let j = (i as i32 + delta) as usize;
                            order.borrow_mut().swap(i, j);
                            if let Some(r) = &rerender {
                                r();
                            }
                        });
                    }
                    list.append(&row.0);
                }
            })
        };
        *slot.borrow_mut() = Some(render.clone());
        render
    };
    render();

    // Thumbnails load in the background, then the list redraws with them.
    if matches!(mode, Mode::Video | Mode::Images) {
        let (thumbs, render) = (thumbs.clone(), render.clone());
        let files = files.clone();
        glib::spawn_future_local(async move {
            let made = gio::spawn_blocking(move || files.iter().map(|f| (f.clone(), thumbnail(f, mode))).collect::<Vec<_>>())
                .await
                .unwrap_or_default();
            for (file, png) in made {
                if let Some(png) = png {
                    if let Ok(p) = Pixbuf::from_file(&png) {
                        thumbs.borrow_mut().insert(file, p);
                    }
                    let _ = std::fs::remove_file(png);
                }
            }
            render();
        });
    }

    let running = Rc::new(Cell::new(false));
    let run = {
        let (frame, order, running) = (frame.clone(), order.clone(), running.clone());
        Rc::new(move || {
            if running.replace(true) {
                return;
            }
            let files = order.borrow().clone();
            let dir = files[0].parent().unwrap_or(Path::new(".")).to_path_buf();
            let out = media::output_named(&dir, out_name, out_ext);
            frame.busy(true, if mode == Mode::Video { "Joining… (re-encoding)" } else { "Working…" });
            let (frame, running) = (frame.clone(), running.clone());
            glib::spawn_future_local(async move {
                let dst = out.clone();
                let ok = gio::spawn_blocking(move || join(mode, &files, &dst)).await.unwrap_or(false);
                if ok {
                    let shown = out.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    media::notify(title, &shown);
                    frame.window.close();
                } else {
                    let _ = std::fs::remove_file(&out);
                    frame.busy(false, "Failed — see notification");
                    media::notify(&format!("{title} failed"), "One of the files couldn't be read");
                    running.set(false);
                }
            });
        })
    };
    {
        let run = run.clone();
        frame.primary.connect_clicked(move |_| run());
    }

    frame.window.present();
}

/// One list row: index, thumbnail or glyph, name, and ▲ ▼.
fn row_for(i: usize, file: &Path, count: usize, mode: Mode, thumb: Option<&Pixbuf>) -> (gtk::Box, gtk::Button, gtk::Button) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let index = gtk::Label::builder().label((i + 1).to_string()).width_chars(2).build();
    index.add_css_class("dim");
    row.append(&index);

    let picture: gtk::Widget = match thumb {
        Some(p) => {
            let texture = gtk::gdk::Texture::for_pixbuf(p);
            gtk::Picture::builder()
                .paintable(&texture)
                .content_fit(gtk::ContentFit::Cover)
                .can_shrink(true)
                .width_request(THUMB * 3 / 2)
                .height_request(THUMB)
                .build()
                .upcast()
        }
        None => {
            let icon = gtk::Image::builder().icon_name(mode.icon()).pixel_size(THUMB / 2).width_request(THUMB * 3 / 2).build();
            icon.add_css_class("dim");
            icon.upcast()
        }
    };
    row.append(&picture);

    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let label = gtk::Label::builder().label(name).xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
    row.append(&label);

    let up = gtk::Button::builder().icon_name("go-up-symbolic").tooltip_text("Move up").sensitive(i > 0).build();
    let down = gtk::Button::builder().icon_name("go-down-symbolic").tooltip_text("Move down").sensitive(i + 1 < count).build();
    for b in [&up, &down] {
        b.add_css_class("flat");
        row.append(b);
    }
    (row, up, down)
}

/// A small PNG to show in the list. Blocking.
fn thumbnail(file: &Path, mode: Mode) -> Option<PathBuf> {
    let out = std::env::temp_dir().join(format!("quicky-thumb-{}-{}.png", std::process::id(), glib::random_int()));
    let ok = match mode {
        Mode::Images => canvas::load_image(file)
            .and_then(|p| {
                let w = (p.width() * THUMB * 2 / p.height().max(1)).max(1);
                p.scale_simple(w, THUMB * 2, InterpType::Bilinear)
            })
            .is_some_and(|p| p.savev(&out, "png", &[]).is_ok()),
        Mode::Video => Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-ss", "1", "-i"])
            .arg(file)
            .args(["-frames:v", "1", "-vf", &format!("scale=-2:{}", THUMB * 2)])
            .arg(&out)
            .status()
            .is_ok_and(|s| s.success()),
        _ => false,
    };
    (ok && out.exists()).then_some(out)
}

fn join(mode: Mode, files: &[PathBuf], out: &Path) -> bool {
    let status = match mode {
        Mode::Video => join_av(files, out, true),
        Mode::Audio => join_av(files, out, false),
        Mode::Pdf => Command::new("qpdf").arg("--empty").arg("--pages").args(files).arg("--").arg(out).status(),
        Mode::Images => Command::new("magick").args(files).arg("-auto-orient").arg(out).status(),
    };
    status.is_ok_and(|s| s.success()) && out.exists()
}

/// Concatenates with re-encoding, so clips of any format, size, or frame
/// rate join cleanly. Video takes the first clip's frame size; the others
/// are fitted inside it. A clip without sound gets silence of its length.
fn join_av(files: &[PathBuf], out: &Path, video: bool) -> std::io::Result<std::process::ExitStatus> {
    let probes: Vec<media::Media> = files.iter().map(|f| media::probe(f)).collect();
    let (w, h) = (probes[0].width.max(2) / 2 * 2, probes[0].height.max(2) / 2 * 2);

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y"]);
    let mut filter = String::new();
    let mut inputs = 0;
    for (i, (file, probe)) in files.iter().zip(&probes).enumerate() {
        cmd.arg("-i").arg(file);
        let source = inputs;
        inputs += 1;
        let audio = if probe.has_audio {
            format!("[{source}:a]")
        } else {
            cmd.args(["-f", "lavfi", "-t", &format!("{:.3}", probe.duration.max(0.1)), "-i", "anullsrc=r=48000:cl=stereo"]);
            inputs += 1;
            format!("[{}:a]", inputs - 1)
        };
        if video {
            filter += &format!(
                "[{source}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p[v{i}];"
            );
        }
        filter += &format!("{audio}aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}];");
    }
    for i in 0..files.len() {
        if video {
            filter += &format!("[v{i}]");
        }
        filter += &format!("[a{i}]");
    }
    filter += &format!("concat=n={}:v={}:a=1", files.len(), if video { 1 } else { 0 });
    filter += if video { "[v][a]" } else { "[a]" };

    cmd.args(["-filter_complex", &filter]);
    if video {
        cmd.args(["-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-crf", "20", "-preset", "medium", "-movflags", "+faststart"]);
    } else {
        cmd.args(["-map", "[a]"]);
    }
    cmd.args(["-c:a", "aac", "-b:a", "192k"]).arg(out);
    cmd.status()
}
