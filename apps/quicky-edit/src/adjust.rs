//! The photo adjustments, done per pixel in Rust so the preview and the
//! saved copy come from the same code. Values are on the sliders' scales:
//! mostly -100..100, exposure in EV, gamma as a power, hue in degrees.

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub exposure: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub highlights: f64,
    pub shadows: f64,
    pub whites: f64,
    pub blacks: f64,
    pub gamma: f64,
    pub temperature: f64,
    pub tint: f64,
    pub saturation: f64,
    pub vibrance: f64,
    pub hue: f64,
    pub clarity: f64,
    pub dehaze: f64,
    pub grain: f64,
    pub noise: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            exposure: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            gamma: 1.0,
            temperature: 0.0,
            tint: 0.0,
            saturation: 0.0,
            vibrance: 0.0,
            hue: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            grain: 0.0,
            noise: 0.0,
        }
    }
}

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Returns an adjusted copy of `src`.
pub fn apply(src: &Pixbuf, s: &Settings) -> Option<Pixbuf> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let n = src.n_channels() as usize;
    let stride = src.rowstride() as usize;
    let bytes = src.read_pixel_bytes();
    let data: &[u8] = &bytes;

    // Planar RGB in 0..1.
    let mut rgb = vec![0f32; w * h * 3];
    let mut alpha = vec![255u8; if n == 4 { w * h } else { 0 }];
    for y in 0..h {
        let row = &data[y * stride..];
        for x in 0..w {
            let p = &row[x * n..x * n + n];
            let i = (y * w + x) * 3;
            rgb[i] = p[0] as f32 / 255.0;
            rgb[i + 1] = p[1] as f32 / 255.0;
            rgb[i + 2] = p[2] as f32 / 255.0;
            if n == 4 {
                alpha[y * w + x] = p[3];
            }
        }
    }

    // Neighbourhood work first: noise reduction smooths the source, clarity
    // compares each pixel with a wide blur of the luminance. Radii follow
    // the picture size so the preview looks like the saved copy.
    let edge = w.max(h) as f32;
    if s.noise > 0.0 {
        let soft = blur3(&rgb, w, h, ((edge / 900.0) * 1.5).max(1.0) as usize);
        let k = (s.noise / 100.0) as f32;
        for (c, b) in rgb.iter_mut().zip(soft) {
            *c += (b - *c) * k;
        }
    }
    let local = if s.clarity != 0.0 {
        let l: Vec<f32> = rgb.chunks(3).map(|p| luma(p[0], p[1], p[2])).collect();
        Some(blur1(&l, w, h, (edge / 60.0).max(2.0) as usize))
    } else {
        None
    };

    let ev = 2f32.powf(s.exposure as f32);
    let bright = (s.brightness / 100.0) as f32 * 0.25;
    let contrast = 1.0 + (s.contrast / 100.0) as f32;
    let (hi, sh) = ((s.highlights / 100.0) as f32 * 0.5, (s.shadows / 100.0) as f32 * 0.5);
    let white = 1.0 - (s.whites / 100.0) as f32 * 0.25;
    let black = -(s.blacks / 100.0) as f32 * 0.15;
    let inv_gamma = 1.0 / s.gamma.max(0.05) as f32;
    let (temp, tint) = ((s.temperature / 100.0) as f32 * 0.12, (s.tint / 100.0) as f32 * 0.1);
    let sat = 1.0 + (s.saturation / 100.0) as f32;
    let vib = (s.vibrance / 100.0) as f32;
    let clarity = (s.clarity / 100.0) as f32 * 0.8;
    let haze = (s.dehaze / 100.0) as f32 * 0.35;
    let grain = (s.grain / 100.0) as f32 * 0.16;
    let (hs, hc) = ((s.hue as f32).to_radians().sin(), (s.hue as f32).to_radians().cos());

    for (i, px) in rgb.chunks_mut(3).enumerate() {
        let (mut r, mut g, mut b) = (px[0], px[1], px[2]);

        r *= ev;
        g *= ev;
        b *= ev;
        if haze != 0.0 {
            // Remove (or add) an even veil of light, then restore contrast.
            let k = 1.0 / (1.0 - haze).max(0.2);
            r = (r - haze) * k;
            g = (g - haze) * k;
            b = (b - haze) * k;
        }
        r += bright;
        g += bright;
        b += bright;
        r = (r - 0.5) * contrast + 0.5;
        g = (g - 0.5) * contrast + 0.5;
        b = (b - 0.5) * contrast + 0.5;

        let l = luma(r, g, b).clamp(0.0, 1.0);
        let tone = hi * l * l + sh * (1.0 - l) * (1.0 - l);
        r += tone;
        g += tone;
        b += tone;
        if let Some(local) = &local {
            // Midtone contrast against the neighbourhood.
            let d = (l - local[i]) * clarity * (1.0 - (2.0 * l - 1.0).powi(2));
            r += d;
            g += d;
            b += d;
        }

        let level = |c: f32| ((c - black) / (white - black)).max(0.0);
        r = level(r);
        g = level(g);
        b = level(b);
        if inv_gamma != 1.0 {
            r = r.powf(inv_gamma);
            g = g.powf(inv_gamma);
            b = b.powf(inv_gamma);
        }

        r += temp;
        b -= temp;
        g -= tint;

        if hs != 0.0 {
            // Rotate the hue in YIQ, which keeps brightness.
            let y = 0.299 * r + 0.587 * g + 0.114 * b;
            let ii = 0.596 * r - 0.274 * g - 0.322 * b;
            let q = 0.211 * r - 0.523 * g + 0.312 * b;
            let (ii, q) = (ii * hc - q * hs, ii * hs + q * hc);
            r = y + 0.956 * ii + 0.621 * q;
            g = y - 0.272 * ii - 0.647 * q;
            b = y - 1.106 * ii + 1.703 * q;
        }

        let l = luma(r, g, b);
        let chroma = r.max(g).max(b) - r.min(g).min(b);
        // Vibrance lifts dull colours more than vivid ones.
        let k = sat * (1.0 + vib * (1.0 - chroma.clamp(0.0, 1.0)));
        r = l + (r - l) * k;
        g = l + (g - l) * k;
        b = l + (b - l) * k;

        if grain > 0.0 {
            let n = (noise_at(i as u32) - 0.5) * grain;
            r += n;
            g += n;
            b += n;
        }

        px[0] = r;
        px[1] = g;
        px[2] = b;
    }

    let has_alpha = n == 4;
    let out_n = if has_alpha { 4 } else { 3 };
    let mut out = Vec::with_capacity(w * h * out_n);
    for (i, px) in rgb.chunks(3).enumerate() {
        for c in px {
            out.push((c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
        if has_alpha {
            out.push(alpha[i]);
        }
    }
    let bytes = gtk::glib::Bytes::from_owned(out);
    Some(Pixbuf::from_bytes(&bytes, Colorspace::Rgb, has_alpha, 8, w as i32, h as i32, (w * out_n) as i32))
}

// A cheap, repeatable pseudo-random value in 0..1 per pixel.
fn noise_at(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Three box blurs approximate a gaussian; one channel.
fn blur1(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut a = src.to_vec();
    let mut tmp = vec![0f32; a.len()];
    for _ in 0..3 {
        box_pass(&a, &mut tmp, w, h, r, 1, true);
        box_pass(&tmp, &mut a, w, h, r, 1, false);
    }
    a
}

/// The same for interleaved RGB.
fn blur3(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut a = src.to_vec();
    let mut tmp = vec![0f32; a.len()];
    for _ in 0..2 {
        box_pass(&a, &mut tmp, w, h, r, 3, true);
        box_pass(&tmp, &mut a, w, h, r, 3, false);
    }
    a
}

/// One running-sum box blur along rows (`horizontal`) or columns.
fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, ch: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let at = |line: usize, k: usize| if horizontal { line * w + k } else { k * w + line };
    let span = (2 * r + 1) as f32;
    for line in 0..lines {
        for c in 0..ch {
            let get = |k: isize| src[at(line, k.clamp(0, len as isize - 1) as usize) * ch + c];
            let mut sum: f32 = (-(r as isize)..=r as isize).map(get).sum();
            for k in 0..len {
                dst[at(line, k) * ch + c] = sum / span;
                sum += get(k as isize + r as isize + 1) - get(k as isize - r as isize);
            }
        }
    }
}
