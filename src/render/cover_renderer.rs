use image::imageops::FilterType;

pub const COVER_CHARSET: &str = "⠀░▒▓█";

pub fn render_cover_ascii(image_bytes: &[u8], width: u16, height: u16) -> Option<String> {
    let img = image::load_from_memory(image_bytes).ok()?;
    // Performance: large covers + Lanczos can be very expensive; Triangle is much faster.
    let resized = img.resize_exact(width as u32, (height as u32).saturating_mul(2), FilterType::Triangle);
    let gray = resized.to_luma8();

    let charset: Vec<char> = COVER_CHARSET.chars().collect();
    let mut out = String::new();

    let target_h = (height as u32).saturating_mul(2);
    let w = width as u32;

    for y in (0..target_h).step_by(2) {
        for x in 0..w {
            let pixel = gray.get_pixel(x, y)[0];
            let idx = ((pixel as usize) * (charset.len().saturating_sub(1))) / 255;
            out.push(charset[idx]);
        }
        out.push('\n');
    }

    Some(out)
}
