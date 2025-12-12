use crate::app::state::TrackMetadata;
use anyhow::Result;
use lofty::{Accessor, AudioFile, TaggedFileExt};
use std::collections::hash_map::DefaultHasher;
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

    Ok(meta)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}
