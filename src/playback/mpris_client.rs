#[cfg(target_os = "linux")]
mod imp {
    use crate::app::state::{PlaybackState, TrackMetadata};
    use anyhow::Result;
    use mpris::{PlaybackStatus, PlayerFinder, TrackID};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::path::PathBuf;
    use std::time::Duration;

    #[derive(Debug, Clone)]
    pub struct MprisSnapshot {
        pub track: TrackMetadata,
        pub position: Duration,
        pub volume: f32,
        pub playback: PlaybackState,
    }

    pub struct MprisClient {
        finder: PlayerFinder,
        last_track_id: Option<TrackID>,
    }

    impl MprisClient {
        pub fn new() -> Self {
            Self {
                finder: PlayerFinder::new().unwrap_or_else(|e| {
                    log::warn!("mpris finder init failed: {e}");
                    PlayerFinder::new().unwrap()
                }),
                last_track_id: None,
            }
        }

        pub fn poll_snapshot(&mut self) -> Result<Option<MprisSnapshot>> {
            let player = match self.finder.find_active() {
                Ok(p) => p,
                Err(_) => return Ok(None),
            };

            let status = player.get_playback_status().unwrap_or(PlaybackStatus::Stopped);
            let playback = match status {
                PlaybackStatus::Playing => PlaybackState::Playing,
                PlaybackStatus::Paused => PlaybackState::Paused,
                PlaybackStatus::Stopped => PlaybackState::Stopped,
            };

            let meta = player.get_metadata().ok();
            let mut track = TrackMetadata::default();
            if let Some(m) = meta {
                self.last_track_id = m.track_id();
                if let Some(t) = m.title() {
                    track.title = t.to_string();
                }
                if let Some(artists) = m.artists() {
                    if let Some(a) = artists.first() {
                        track.artist = a.to_string();
                    }
                }
                if let Some(al) = m.album_name() {
                    track.album = al.to_string();
                }
                if let Some(d) = m.length() {
                    track.duration = d;
                }
                if let Some(url) = m.art_url() {
                    if let Some(bytes) = read_art_url(url) {
                        track.cover_hash = Some(hash_bytes(&bytes));
                        track.cover = Some(bytes);
                    }
                }
            }

            let position = player.get_position().unwrap_or(Duration::from_secs(0));
            let volume = (player.get_volume().unwrap_or(0.0) as f32).clamp(0.0, 1.0);

            Ok(Some(MprisSnapshot {
                track,
                position,
                volume,
                playback,
            }))
        }

        pub fn toggle_play_pause(&mut self) -> Result<()> {
            if let Ok(p) = self.finder.find_active() {
                let _ = p.play_pause();
            }
            Ok(())
        }

        pub fn pause(&mut self) -> Result<()> {
            if let Ok(p) = self.finder.find_active() {
                let _ = p.pause();
            }
            Ok(())
        }

        pub fn next(&mut self) -> Result<()> {
            if let Ok(p) = self.finder.find_active() {
                let _ = p.next();
            }
            Ok(())
        }

        pub fn prev(&mut self) -> Result<()> {
            if let Ok(p) = self.finder.find_active() {
                let _ = p.previous();
            }
            Ok(())
        }

        pub fn seek_to(&mut self, pos: Duration) -> Result<()> {
            if let Ok(p) = self.finder.find_active() {
                if let Some(id) = self.last_track_id.clone() {
                    let _ = p.set_position(id, &pos);
                } else {
                    // fallback: relative seek
                    let cur = p.get_position().unwrap_or(Duration::from_secs(0));
                    let delta = (pos.as_micros() as i128) - (cur.as_micros() as i128);
                    let delta = delta.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
                    let _ = p.seek(delta);
                }
            }
            Ok(())
        }

        pub fn set_volume_delta(&mut self, delta: f32) -> Result<()> {
            if let Ok(p) = self.finder.find_active() {
                let v = p.get_volume().unwrap_or(0.0) as f32;
                let nv = (v + delta).clamp(0.0, 1.0);
                let _ = p.set_volume(nv as f64);
            }
            Ok(())
        }
    }

    fn hash_bytes(bytes: &[u8]) -> u64 {
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish()
    }

    fn read_art_url(url: &str) -> Option<Vec<u8>> {
        // Only support file:// URLs in MVP.
        let u = url.trim();
        if let Some(path) = u.strip_prefix("file://") {
            let p = PathBuf::from(path);
            return std::fs::read(p).ok();
        }
        None
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use crate::app::state::{PlaybackState, TrackMetadata};
    use anyhow::Result;
    use std::time::Duration;

    #[derive(Debug, Clone)]
    pub struct MprisSnapshot {
        pub track: TrackMetadata,
        pub position: Duration,
        pub volume: f32,
        pub playback: PlaybackState,
    }

    pub struct MprisClient;

    impl MprisClient {
        pub fn new() -> Self {
            Self
        }

        pub fn poll_snapshot(&mut self) -> Result<Option<MprisSnapshot>> {
            Ok(None)
        }

        pub fn toggle_play_pause(&mut self) -> Result<()> {
            Ok(())
        }

        pub fn pause(&mut self) -> Result<()> {
            Ok(())
        }

        pub fn next(&mut self) -> Result<()> {
            Ok(())
        }

        pub fn prev(&mut self) -> Result<()> {
            Ok(())
        }

        pub fn seek_to(&mut self, _pos: Duration) -> Result<()> {
            Ok(())
        }

        pub fn set_volume_delta(&mut self, _delta: f32) -> Result<()> {
            Ok(())
        }
    }
}

pub use imp::*;
