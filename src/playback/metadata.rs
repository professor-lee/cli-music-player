use crate::app::state::{LyricLine, TrackMetadata};
use anyhow::Result;
use lofty::{Accessor, AudioFile, ItemKey, Tag, TaggedFileExt};
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

    }

    // Embedded cover (prefer any embedded picture across all tags; best-effort)
    if meta.cover.is_none() {
        if let Some((bytes, hash)) = read_embedded_cover(&tagged) {
            meta.cover_hash = Some(hash);
            meta.cover = Some(bytes);
            meta.cover_folder = path.parent().map(|p| p.to_path_buf());
        }
    }

    // Fallback: local folder cover image near the audio file.
    if meta.cover.is_none() {
        let folder = path.parent().unwrap_or(Path::new("."));
        if let Some((bytes, hash)) = read_cover_from_folder(folder) {
            meta.cover_hash = Some(hash);
            meta.cover = Some(bytes);
            meta.cover_folder = Some(folder.to_path_buf());
        }
    }

    // fallback title from filename
    if meta.title == "Unknown" {
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            meta.title = name.to_string();
        }
    }

    // Embedded lyrics first; fallback to local .lrc.
    meta.lyrics = read_embedded_lyrics(&tagged).or_else(|| read_lrc_for_audio(path));

    Ok(meta)
}

fn read_embedded_cover(tagged: &lofty::TaggedFile) -> Option<(Vec<u8>, u64)> {
    // Try primary tag first, then other tags.
    if let Some(t) = tagged.primary_tag() {
        if let Some((b, h)) = read_cover_from_tag(t) {
            return Some((b, h));
        }
    }
    for t in tagged.tags() {
        if let Some((b, h)) = read_cover_from_tag(t) {
            return Some((b, h));
        }
    }
    None
}

fn read_cover_from_tag(tag: &Tag) -> Option<(Vec<u8>, u64)> {
    let pic = tag.pictures().first()?;
    let bytes = pic.data().to_vec();
    let hash = hash_bytes(&bytes);
    Some((bytes, hash))
}

pub fn read_cover_from_folder(dir: &Path) -> Option<(Vec<u8>, u64)> {

    // Common filenames used by many players.
    // Keep this list small and predictable.
    let candidates = [
        "cover",
        "folder",
        "front",
        "album",
        "artwork",
        "Cover",
        "Folder",
        "Front",
    ];
    let exts = ["jpg", "jpeg", "png"];

    for base in candidates {
        for ext in exts {
            let p = dir.join(format!("{base}.{ext}"));
            if let Ok(bytes) = fs::read(&p) {
                if !bytes.is_empty() {
                    let hash = hash_bytes(&bytes);
                    return Some((bytes, hash));
                }
            }
        }
    }
    None
}

fn read_embedded_lyrics(tagged: &lofty::TaggedFile) -> Option<Vec<LyricLine>> {
    // Try primary tag first, then other tags.
    if let Some(t) = tagged.primary_tag() {
        if let Some(lines) = read_lyrics_from_tag(t) {
            return Some(lines);
        }
    }
    for t in tagged.tags() {
        if let Some(lines) = read_lyrics_from_tag(t) {
            return Some(lines);
        }
    }
    None
}

fn read_lyrics_from_tag(tag: &Tag) -> Option<Vec<LyricLine>> {
    let raw = tag.get_string(&ItemKey::Lyrics)?.trim();
    if raw.is_empty() {
        return None;
    }

    // If it's LRC-like, parse timestamps.
    if let Some(parsed) = parse_lrc(raw) {
        return Some(parsed);
    }

    // Otherwise treat it as unsynchronized lyrics: show first 1-2 lines statically.
    let mut non_empty = raw.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = non_empty.next()?.to_string();
    let second = non_empty.next().map(|s| s.to_string());

    let mut out = Vec::new();
    out.push(LyricLine { start_ms: 0, text: first });
    if let Some(s2) = second {
        out.push(LyricLine { start_ms: u64::MAX, text: s2 });
    }
    Some(out)
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
