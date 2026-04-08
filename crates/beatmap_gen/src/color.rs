/// Dominant-colour extraction from raw image bytes (JPEG / PNG).
///
/// Algorithm:
///   1. Decode & resize to 32×32 for fast pixel access.
///   2. Convert each pixel to HSV; discard near-black, near-white, and grey.
///   3. Bucket pixels into 36 hue buckets (10° each), weighted by saturation×value.
///   4. Pick the heaviest bucket and return its weighted-average RGB.
///
/// Yields a single vivid [r, g, b] that represents the most prominent
/// colourful region of the cover art — exactly what an ESP32 needs.
pub fn dominant_color(image_bytes: &[u8]) -> Option<[u8; 3]> {
    let img = image::load_from_memory(image_bytes).ok()?.into_rgb8();
    let small = image::imageops::resize(&img, 32, 32, image::imageops::FilterType::Nearest);

    // Per-hue-bucket: (weighted_r, weighted_g, weighted_b, total_weight)
    let mut buckets = [(0u64, 0u64, 0u64, 0u64); 36];

    for pixel in small.pixels() {
        let r = pixel[0] as f32 / 255.0;
        let g = pixel[1] as f32 / 255.0;
        let b = pixel[2] as f32 / 255.0;

        let (h, s, v) = rgb_to_hsv(r, g, b);

        // Skip achromatic, near-black, and near-white pixels.
        if s < 0.15 || v < 0.10 || v > 0.95 {
            continue;
        }

        let bucket = ((h / 360.0) * 36.0) as usize % 36;
        let weight = ((s * v) * 1000.0) as u64;
        buckets[bucket].0 += pixel[0] as u64 * weight;
        buckets[bucket].1 += pixel[1] as u64 * weight;
        buckets[bucket].2 += pixel[2] as u64 * weight;
        buckets[bucket].3 += weight;
    }

    // Pick the bucket with the highest total weight.
    let best = buckets
        .iter()
        .enumerate()
        .max_by_key(|(_, b)| b.3)?;

    let (_, (wr, wg, wb, total)) = best;
    if *total == 0 {
        return None;
    }

    Some([
        (*wr / total) as u8,
        (*wg / total) as u8,
        (*wb / total) as u8,
    ])
}

/// Convert linear RGB [0,1] to HSV.  Returns (hue°, saturation, value).
fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;
    let s = if max > 0.0 { delta / max } else { 0.0 };

    let h = if delta < 1e-6 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    let h = if h < 0.0 { h + 360.0 } else { h };
    (h, s, v)
}
