//! Media helpers shared by the editors: probing, timeline backdrops, time
//! formatting, output naming, and notifications.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Default)]
pub struct Media {
    pub duration: f64,
    pub has_video: bool,
    pub has_audio: bool,
    /// Frame size of the first video stream, 0 without one.
    pub width: u32,
    pub height: u32,
}

pub fn probe(path: &Path) -> Media {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height", "-of", "default=nw=1"])
        .arg(path)
        .output();
    let mut media = Media::default();
    let Ok(out) = out else { return media };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        match line.split_once('=') {
            Some(("duration", v)) => media.duration = v.trim().parse().unwrap_or(0.0),
            Some(("codec_type", "video")) => media.has_video = true,
            Some(("codec_type", "audio")) => media.has_audio = true,
            Some(("width", v)) if media.width == 0 => media.width = v.trim().parse().unwrap_or(0),
            Some(("height", v)) if media.height == 0 => media.height = v.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    // Cover art shows up as a one-frame "video" stream on audio files.
    if media.has_video && is_audio_ext(path) {
        media.has_video = false;
    }
    media
}

fn is_audio_ext(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aiff" | "aif" | "wma" | "aac")
}

/// Renders the timeline backdrop to a PNG: a filmstrip of frames for video,
/// a waveform in `color` for audio. `aspect` is the strip's width / height,
/// so filmstrip frames come out undistorted. Blocking; run it off the UI
/// thread.
pub fn backdrop(path: &Path, media: &Media, color: &str, aspect: f64) -> Option<PathBuf> {
    let out = std::env::temp_dir().join(format!("quicky-strip-{}.png", std::process::id()));
    let ok = if media.has_video { filmstrip(path, media, aspect, &out) } else { waveform(path, color, &out) };
    (ok && out.exists()).then_some(out)
}

// Seeks to each frame rather than decoding the whole video: a long clip
// takes as long as a short one. Frames are grabbed in parallel.
fn filmstrip(path: &Path, media: &Media, aspect: f64, out: &Path) -> bool {
    let frame_aspect = if media.height > 0 { media.width as f64 / media.height as f64 } else { 16.0 / 9.0 };
    let count = (aspect / frame_aspect).round().clamp(2.0, 24.0) as usize;
    let dir = std::env::temp_dir();
    let frames: Vec<PathBuf> = (0..count).map(|i| dir.join(format!("quicky-frame-{}-{i}.png", std::process::id()))).collect();
    std::thread::scope(|scope| {
        for (i, frame) in frames.iter().enumerate() {
            let t = media.duration * (i as f64 + 0.5) / count as f64;
            scope.spawn(move || {
                let _ = Command::new("ffmpeg")
                    .args(["-v", "error", "-y", "-ss", &format!("{t:.3}"), "-i"])
                    .arg(path)
                    .args(["-frames:v", "1", "-vf", "scale=-2:120"])
                    .arg(frame)
                    .status();
            });
        }
    });
    let present: Vec<&PathBuf> = frames.iter().filter(|f| f.exists()).collect();
    let ok = !present.is_empty()
        && Command::new("magick").args(&present).arg("+append").arg(out).status().is_ok_and(|s| s.success());
    for f in &frames {
        let _ = std::fs::remove_file(f);
    }
    ok
}

fn waveform(path: &Path, color: &str, out: &Path) -> bool {
    waveform_of(path, color, None, out)
}

/// A waveform PNG of one channel (`Some(0)` left, `Some(1)` right) or of
/// the whole mix. Mono and on a cube-root scale, so quiet speech still
/// reads as a shape instead of a flat line. Blocking.
pub fn waveform_of(path: &Path, color: &str, channel: Option<usize>, out: &Path) -> bool {
    let pick = match channel {
        Some(c) => format!("pan=mono|c0=c{c},"),
        None => "aformat=channel_layouts=mono,".into(),
    };
    let filter = format!("{pick}showwavespic=s=1600x240:colors={color}:scale=cbrt");
    Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(path)
        .args(["-filter_complex", &filter, "-frames:v", "1", "-update", "1"])
        .arg(out)
        .status()
        .is_ok_and(|s| s.success())
}

/// What kind of file this is, by extension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Image,
    Video,
    Audio,
    Pdf,
    Other,
}

pub fn kind(path: &Path) -> Kind {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "heic" | "heif" | "tif" | "tiff" | "avif" | "bmp" | "svg" | "ico" | "gif" => Kind::Image,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "m4v" | "flv" => Kind::Video,
        "pdf" => Kind::Pdf,
        _ if is_audio_ext(path) => Kind::Audio,
        _ => Kind::Other,
    }
}

/// `83_000_000` → `79.2 MB`
pub fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    match b {
        b if b >= 1024.0 * 1024.0 * 1024.0 => format!("{:.2} GB", b / 1024.0 / 1024.0 / 1024.0),
        b if b >= 1024.0 * 1024.0 => format!("{:.1} MB", b / 1024.0 / 1024.0),
        b if b >= 1024.0 => format!("{:.0} KB", b / 1024.0),
        b => format!("{b} B"),
    }
}

/// `83.6` → `1:23.60`
pub fn fmt_time(t: f64) -> String {
    let t = t.max(0.0);
    let m = (t / 60.0).floor() as u64;
    format!("{m}:{:05.2}", t - m as f64 * 60.0)
}

/// Accepts `83.6`, `1:23.6`, or `0:01:23.6`.
pub fn parse_time(s: &str) -> Option<f64> {
    s.trim()
        .split(':')
        .try_fold(0.0, |acc, part| part.trim().parse::<f64>().ok().map(|v| acc * 60.0 + v))
}

/// `dir/stem<suffix>.ext`, numbered (`-1`, `-2`, …) rather than overwriting.
pub fn output_path(src: &Path, suffix: &str, ext: Option<&str>) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new("."));
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = ext
        .map(str::to_string)
        .or_else(|| src.extension().and_then(|e| e.to_str()).map(str::to_string))
        .unwrap_or_default();
    output_named(dir, &format!("{stem}{suffix}"), &ext)
}

/// `dir/name.ext`, numbered (`name-1.ext`, …) rather than overwriting.
pub fn output_named(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let candidate = |n: u32| {
        let num = if n == 0 { String::new() } else { format!("-{n}") };
        dir.join(format!("{name}{num}.{ext}"))
    };
    (0..).map(candidate).find(|p| !p.exists()).unwrap()
}

/// A temporary file path unique to this process and call.
pub fn temp(tag: &str, ext: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("quicky-{tag}-{}-{n}.{ext}", std::process::id()))
}

/// The video frame at `t` seconds as a PNG file, `height` px tall (`None`
/// keeps the frame size). Blocking.
pub fn grab_frame(path: &Path, t: f64, height: Option<i32>) -> Option<PathBuf> {
    let out = temp("frame", "png");
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-ss", &format!("{:.3}", t.max(0.0)), "-i"]).arg(path).args(["-frames:v", "1"]);
    if let Some(h) = height {
        cmd.args(["-vf", &format!("scale=-2:{h}")]);
    }
    let ok = cmd.arg(&out).status().is_ok_and(|s| s.success());
    (ok && out.exists()).then_some(out)
}

/// Saves a rendered picture in whatever format `dst`'s extension names.
/// Cairo keeps premultiplied BGRA; ImageMagick gets plain RGBA. Formats
/// without transparency get a white background. Blocking.
pub fn save_surface(surface: &mut gtk::cairo::ImageSurface, dst: &Path) -> bool {
    use std::io::Write;
    surface.flush();
    let (w, h, stride) = (surface.width() as usize, surface.height() as usize, surface.stride() as usize);
    let Ok(data) = surface.data() else { return false };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in data.chunks(stride).take(h) {
        for px in row[..w * 4].chunks(4) {
            let (b, g, r, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
            let un = |c: u32| if a == 0 { 0 } else { ((c * 255 + a / 2) / a).min(255) as u8 };
            rgba.extend_from_slice(&[un(r), un(g), un(b), a as u8]);
        }
    }
    drop(data);
    let ext = dst.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mut cmd = Command::new("magick");
    cmd.args(["-size", &format!("{w}x{h}"), "-depth", "8", "rgba:-"]);
    if matches!(ext.as_str(), "jpg" | "jpeg" | "bmp" | "pdf") {
        cmd.args(["-background", "white", "-alpha", "remove", "-alpha", "off"]);
    }
    cmd.args(["-quality", "92"]).arg(dst).stdin(std::process::Stdio::piped());
    let Ok(mut child) = cmd.spawn() else { return false };
    let wrote = child.stdin.take().is_some_and(|mut s| s.write_all(&rgba).is_ok());
    wrote && child.wait().is_ok_and(|s| s.success())
}

/// Saves a picture in whatever format `dst`'s extension names. Blocking.
pub fn save_pixbuf(image: &gtk::gdk_pixbuf::Pixbuf, dst: &Path) -> bool {
    let ext = dst.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" => image.savev(dst, "png", &[]).is_ok(),
        "jpg" | "jpeg" => image.savev(dst, "jpeg", &[("quality", "92")]).is_ok(),
        _ => {
            let tmp = temp("save", "png");
            let ok = image.savev(&tmp, "png", &[]).is_ok() && run(Command::new("magick").arg(&tmp).args(["-quality", "92"]).arg(dst));
            let _ = std::fs::remove_file(tmp);
            ok
        }
    }
}

/// Raster formats Quicky writes back as they are; anything else (SVG,
/// ICO, ...) is saved as PNG.
pub fn image_out_ext(src: &Path) -> Option<&'static str> {
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    (!matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "heic" | "heif" | "tif" | "tiff" | "avif" | "bmp")).then_some("png")
}

/// Runs a command, true when it exits 0.
pub fn run(cmd: &mut Command) -> bool {
    cmd.status().is_ok_and(|s| s.success())
}

pub fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send").args(["-a", "Quicky", title, body]).status();
}
