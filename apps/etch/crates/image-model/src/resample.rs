//! Resize Image's resampling: nearest neighbour, and smooth (a triangle filter, widened
//! when shrinking so every source pixel counts), both on straight RGBA. Smooth filters
//! premultiplied (each pixel's colour weighted by its alpha) and divides the alpha back
//! out, so a transparent pixel's hidden colour never bleeds into its neighbours; it's
//! ImageMagick's `-filter Triangle -resize`, which tests compare it with.

use crate::par::bands;
use crate::Image;

/// `image` at `width` × `height` (each at least 1), each pixel the source pixel its centre
/// falls in: doubling copies each pixel into a 2×2 square exactly.
pub(crate) fn nearest(image: &Image, width: u32, height: u32) -> Vec<u8> {
    let (sw, sh) = (image.width() as u64, image.height() as u64);
    // The source column of each column: its centre, (x + ½) · sw / width, floored.
    let cols: Vec<usize> = (0..width as u64)
        .map(|x| ((2 * x + 1) * sw / (2 * width as u64)) as usize * 4)
        .collect();
    let src = image.data();
    let stride = image.stride();
    let row = width as usize * 4;
    let mut out = vec![0; row * height as usize];
    bands(&mut out, row, |first, band| {
        for (k, line) in band.chunks_exact_mut(row).enumerate() {
            let y = first as u64 + k as u64;
            let sy = ((2 * y + 1) * sh / (2 * height as u64)) as usize;
            let from = &src[sy * stride..(sy + 1) * stride];
            for (px, &c) in line.as_chunks_mut::<4>().0.iter_mut().zip(&cols) {
                px.copy_from_slice(&from[c..c + 4]);
            }
        }
    });
    out
}

/// The source pixels one destination pixel takes along an axis: from `start`, with these
/// weights (summing to 1).
struct Taps {
    start: usize,
    weights: Vec<f32>,
}

/// The taps of each of `dst` pixels resampled from `src` along an axis: a triangle filter
/// of radius 1 source pixel, `src / dst` when shrinking, centred on the destination
/// pixel's centre, weights normalized (ImageMagick's `HorizontalFilter` and
/// `VerticalFilter` with the Triangle filter).
fn taps(src: u32, dst: u32) -> Vec<Taps> {
    let factor = dst as f64 / src as f64;
    let scale = (1.0 / factor).max(1.0);
    let support = scale;
    (0..dst)
        .map(|i| {
            let centre = (i as f64 + 0.5) / factor;
            // The source pixel the centre is in is always among the taps (the support
            // is at least 1), within half a pixel of it: the weights never sum to 0.
            let start = (centre - support + 0.5).max(0.0) as usize;
            let stop = (centre + support + 0.5).min(src as f64) as usize;
            let weights: Vec<f64> = (start..stop)
                .map(|j| (1.0 - ((j as f64 - centre + 0.5) / scale).abs()).max(0.0))
                .collect();
            let sum: f64 = weights.iter().sum();
            debug_assert!(sum > 0.0);
            let weights = weights.into_iter().map(|w| (w / sum) as f32).collect();
            Taps { start, weights }
        })
        .collect()
}

/// `image` at `width` × `height` (each at least 1), smoothly: each destination pixel the
/// triangle-weighted average of the source pixels around its centre, premultiplied.
pub(crate) fn smooth(image: &Image, width: u32, height: u32) -> Vec<u8> {
    let xs = taps(image.width(), width);
    let ys = taps(image.height(), height);
    let src = image.data();
    let stride = image.stride();
    let w = width as usize;
    let row = w * 4;
    let mut out = vec![0; row * height as usize];
    bands(&mut out, row, |first, band| {
        // One source row filtered across (premultiplied), and the sums down.
        let mut across = vec![[0f32; 4]; w];
        let mut sums = vec![[0f32; 4]; w];
        for (k, line) in band.chunks_exact_mut(row).enumerate() {
            let t = &ys[first as usize + k];
            sums.iter_mut().for_each(|s| *s = [0.0; 4]);
            for (j, &v) in t.weights.iter().enumerate() {
                if v == 0.0 {
                    continue;
                }
                let sy = t.start + j;
                filter_row(&src[sy * stride..(sy + 1) * stride], &xs, &mut across);
                for (s, a) in sums.iter_mut().zip(&across) {
                    for c in 0..4 {
                        s[c] += v * a[c];
                    }
                }
            }
            for (px, s) in line.as_chunks_mut::<4>().0.iter_mut().zip(&sums) {
                *px = unpremultiply(*s);
            }
        }
    });
    out
}

/// A source row filtered across into `out`: per destination pixel the weighted sums of
/// red, green, and blue times alpha, and of alpha (0 to 255).
fn filter_row(row: &[u8], xs: &[Taps], out: &mut [[f32; 4]]) {
    let px = row.as_chunks::<4>().0;
    for (o, t) in out.iter_mut().zip(xs) {
        let mut s = [0f32; 4];
        for (p, &w) in px[t.start..t.start + t.weights.len()]
            .iter()
            .zip(&t.weights)
        {
            let a = w * p[3] as f32;
            s[0] += a * p[0] as f32;
            s[1] += a * p[1] as f32;
            s[2] += a * p[2] as f32;
            s[3] += a;
        }
        *o = s;
    }
}

/// Premultiplied sums back to a straight RGBA8 pixel: the colour divided by the alpha,
/// rounded; no alpha at all is transparent black.
fn unpremultiply([r, g, b, a]: [f32; 4]) -> [u8; 4] {
    if a <= 0.0 {
        return [0; 4];
    }
    let byte = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    [byte(r / a), byte(g / a), byte(b / a), byte(a)]
}
