use crate::app::state::{LyricLine, TrackMetadata};
use anyhow::Result;
use lofty::{Accessor, AudioFile, TaggedFileExt};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

pub fn read_metadata(path: &Path) -> Result<TrackMetadata> {
    let mut meta = TrackMetadata::default();

    let tagged = lofty::read_from_path(path)?;
    let properties = tagged.properties();
    meta.duration = properties.duration();

    if let Some(tag) = tagged.primary_tag() {
        if let Some(t) = tag.title() {
            meta.title = t.to_string();
        }
        if let Some(a) = tag.artist() {
            meta.artist = a.to_string();
        }
        if let Some(al) = tag.album() {
            meta.album = al.to_string();
        }

        if let Some(pic) = tag.pictures().first() {
            let bytes = pic.data().to_vec();
            meta.cover_hash = Some(hash_bytes(&bytes));
            meta.cover = Some(bytes);
        }
    }

    // fallback title from filename
    if meta.title == "Unknown" {
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            meta.title = name.to_string();
        }
    }

    // local lyrics (best-effort): same basename, .lrc extension
    meta.lyrics = read_lrc_for_audio(path);

    Ok(meta)
}

fn read_lrc_for_audio(audio_path: &Path) -> Option<Vec<LyricLine>> {
    let lrc_path = audio_path.with_extension("lrc");
    let content = fs::read_to_string(lrc_path).ok()?;
    parse_lrc(&content)
}

fn parse_lrc(content: &str) -> Option<Vec<LyricLine>> {
    let mut out: Vec<LyricLine> = Vec::new();

    for raw in content.lines() {
        let mut s = raw.trim();
        if s.is_empty() {
            continue;
        }

        // Collect leading [..] tags; keep all time tags, ignore metadata tags like [ti:]
        let mut times: Vec<u64> = Vec::new();
        while let Some(rest) = s.strip_prefix('[') {
            let Some(end) = rest.find(']') else {
                break;
            };
            let tag = &rest[..end];
            if let Some(ms) = parse_lrc_time_tag(tag) {
                times.push(ms);
            }
            s = &rest[end + 1..];
        }

        if times.is_empty() {
            continue;
        }

        let text = s.trim().to_string();
        for t in times {
            out.push(LyricLine {
                start_ms: t,
                text: text.clone(),
            });
        }
    }

    if out.is_empty() {
        return None;
    }
    out.sort_by_key(|l| l.start_ms);
    Some(out)
}

fn parse_lrc_time_tag(tag: &str) -> Option<u64> {
    // Supports mm:ss, mm:ss.xx, mm:ss.xxx
    // Rejects metadata tags like "ti:xxx" by requiring numeric mm and ss.
    let (mm_s, rest) = tag.split_once(':')?;
    let mm: u64 = mm_s.trim().parse().ok()?;

    let rest = rest.trim();
    let (ss_s, frac_s) = if let Some((a, b)) = rest.split_once('.') {
        (a, Some(b))
    } else {
        (rest, None)
    };
    let ss: u64 = ss_s.trim().parse().ok()?;
    if ss >= 60 {
        // be lenient but avoid obvious non-timestamps
        return None;
    }

    let mut ms: u64 = 0;
    if let Some(frac) = frac_s {
        let frac = frac.trim();
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        if digits.is_empty() {
            ms = 0;
        } else if digits.len() == 1 {
            ms = digits.parse::<u64>().ok()? * 100;
        } else if digits.len() == 2 {
            ms = digits.parse::<u64>().ok()? * 10;
        } else {
            ms = digits.parse::<u64>().ok()?;
        }
    }

    Some(mm * 60_000 + ss * 1_000 + ms)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}
