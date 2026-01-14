use anyhow::Result;
use base64::Engine;
use crossterm::{cursor, queue, style::Print};
use image::codecs::png::PngEncoder;
use image::{imageops, ColorType, ImageEncoder};
use ratatui::layout::Rect;
use std::io::{self, Write};

const CHUNK_SIZE: usize = 4096;

pub fn delete_placement(placement_id: u32) -> Result<()> {
    let mut out = io::stdout();
    // NOTE: Kitty protocol does not support deleting by placement id alone.
    // Kept only as a best-effort fallback (delete all visible placements).
    // Prefer `delete_image_placement()`.
    let esc = if placement_id == 0 {
        "\x1b_Ga=d,d=a,q=2;\x1b\\".to_string()
    } else {
        // Best-effort: clear all visible placements rather than leaving stale images.
        "\x1b_Ga=d,d=a,q=2;\x1b\\".to_string()
    };
    queue!(out, Print(esc))?;
    out.flush()?;
    Ok(())
}

pub fn delete_image_placement(image_id: u32, placement_id: u32, free_data: bool) -> Result<()> {
    let mut out = io::stdout();
    // d=i deletes placements for image id; with p also specified, deletes only that placement.
    // Uppercase variant also frees stored image data (if unreferenced).
    let d = if free_data { 'I' } else { 'i' };
    let esc = format!("\x1b_Ga=d,d={d},i={image_id},p={placement_id},q=2;\x1b\\");
    queue!(out, Print(esc))?;
    out.flush()?;
    Ok(())
}

pub fn delete_image(image_id: u32, free_data: bool) -> Result<()> {
    let mut out = io::stdout();
    // d=i deletes by image id; d=I also frees stored image data (if unreferenced).
    let d = if free_data { 'I' } else { 'i' };
    let esc = format!("\x1b_Ga=d,d={d},i={image_id},q=2;\x1b\\");
    queue!(out, Print(esc))?;
    out.flush()?;
    Ok(())
}

pub fn encode_image_bytes_to_png_base64(image_bytes: &[u8], max_w_px: u32, max_h_px: u32) -> Option<String> {
    // Normalize to PNG, as kitty supports f=100 and reads dimensions from PNG header.
    let img = image::load_from_memory(image_bytes).ok()?;

    let mut rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    let max_w_px = max_w_px.max(16);
    let max_h_px = max_h_px.max(16);

    // Downscale large images to reduce encode + transfer time.
    if w > max_w_px || h > max_h_px {
        let scale_w = max_w_px as f32 / w as f32;
        let scale_h = max_h_px as f32 / h as f32;
        let scale = scale_w.min(scale_h).min(1.0);

        let new_w = ((w as f32) * scale).round().max(16.0) as u32;
        let new_h = ((h as f32) * scale).round().max(16.0) as u32;
        rgba = imageops::resize(&rgba, new_w, new_h, imageops::FilterType::Triangle);
    }

    let (w, h) = rgba.dimensions();
    let mut png = Vec::new();
    {
        let enc = PngEncoder::new(&mut png);
        if enc.write_image(&rgba, w, h, ColorType::Rgba8).is_err() {
            return None;
        }
    }

    Some(base64::engine::general_purpose::STANDARD.encode(png))
}

pub fn transmit_png_base64(image_id: u32, b64: &str) -> Result<()> {
    transmit_chunks(b64, |first, more| {
        if first {
            // a=t: transmit only, f=100 PNG, q=2: suppress responses
            format!("a=t,f=100,i={image_id},q=2,m={more}")
        } else {
            format!("m={more},q=2")
        }
    })
}

pub fn place_image(rect: Rect, image_id: u32, placement_id: u32) -> Result<()> {
    if rect.width == 0 || rect.height == 0 {
        return Ok(());
    }
    let mut out = io::stdout();
    queue!(out, cursor::MoveTo(rect.x, rect.y))?;
    // a=p: place a previously transmitted image
    let esc = format!(
        "\x1b_Ga=p,i={image_id},p={placement_id},c={},r={},C=1,q=2;\x1b\\",
        rect.width, rect.height
    );
    queue!(out, Print(esc))?;
    out.flush()?;
    Ok(())
}

fn transmit_chunks<F>(b64: &str, mut control_for_chunk: F) -> Result<()>
where
    F: FnMut(bool, u8) -> String,
{
    let mut out = io::stdout();

    let mut first = true;
    let bytes = b64.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let end = (i + CHUNK_SIZE).min(bytes.len());
        let chunk = &b64[i..end];
        i = end;

        let more = if i < bytes.len() { 1 } else { 0 };
        let control = control_for_chunk(first, more);
        first = false;

        let esc = format!("\x1b_G{control};{chunk}\x1b\\");
        queue!(out, Print(esc))?;
    }

    out.flush()?;
    Ok(())
}
