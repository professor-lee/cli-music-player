use image::imageops;

pub fn dominant_rgb_from_image_bytes(image_bytes: &[u8]) -> Option<(u8, u8, u8)> {
    let img = image::load_from_memory(image_bytes).ok()?;
    let mut rgba = img.to_rgba8();

    // Downsample aggressively for speed.
    let (w, h) = rgba.dimensions();
    let target: u32 = 48;
    if w > target || h > target {
        let scale_w = target as f32 / w as f32;
        let scale_h = target as f32 / h as f32;
        let scale = scale_w.min(scale_h).min(1.0);
        let new_w = ((w as f32) * scale).round().max(8.0) as u32;
        let new_h = ((h as f32) * scale).round().max(8.0) as u32;
        rgba = imageops::resize(&rgba, new_w, new_h, imageops::FilterType::Triangle);
    }

    // Quantize into 5-bit buckets per channel (32^3 = 32768 buckets).
    // Use a weighted count to prefer more saturated colors and de-emphasize very dark/bright pixels.
    let mut buckets = vec![0u32; 32 * 32 * 32];
    for p in rgba.pixels() {
        let [r, g, b, a] = p.0;
        if a < 16 {
            continue;
        }

        let max = r.max(g).max(b) as i32;
        let min = r.min(g).min(b) as i32;
        let sum = (r as i32) + (g as i32) + (b as i32);

        // Ignore extreme blacks/whites which are often borders/background.
        if sum <= 24 || sum >= 750 {
            continue;
        }

        let sat = (max - min).max(0) as u32;
        let weight = 1u32 + (sat / 24);

        let ri = (r >> 3) as usize;
        let gi = (g >> 3) as usize;
        let bi = (b >> 3) as usize;
        let idx = (ri << 10) | (gi << 5) | bi;
        buckets[idx] = buckets[idx].saturating_add(weight);
    }

    let (best_idx, best_count) = buckets
        .iter()
        .enumerate()
        .max_by_key(|&(_i, c)| c)
        .unwrap_or((0, &0));

    if *best_count == 0 {
        return None;
    }

    let ri = ((best_idx >> 10) & 31) as u8;
    let gi = ((best_idx >> 5) & 31) as u8;
    let bi = (best_idx & 31) as u8;

    // Convert bucket center back to 8-bit.
    let to_8 = |v5: u8| (v5 << 3) | (v5 >> 2);
    Some((to_8(ri), to_8(gi), to_8(bi)))
}
