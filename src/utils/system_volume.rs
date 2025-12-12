#[cfg(target_os = "linux")]
mod imp {
    use alsa::mixer::{Mixer, Selem, SelemChannelId, SelemId};
    use anyhow::{anyhow, Result};

    pub struct SystemVolume {
        mixer: Mixer,
        selem_id: SelemId,
        elem_name: String,
    }

    impl SystemVolume {
        pub fn try_new() -> Result<Self> {
            let mixer = Mixer::new("default", false)?;

            let preferred = ["Master", "PCM", "Speaker", "Headphone", "Line Out", "Front"];

            // 1) Prefer common element names.
            for name in preferred {
                let id = SelemId::new(name, 0);
                if let Some(selem) = mixer.find_selem(&id) {
                    if selem.has_playback_volume() {
                        return Ok(Self {
                            mixer,
                            selem_id: id,
                            elem_name: name.to_string(),
                        });
                    }
                }
            }

            // 2) Fall back to the first element that has playback volume.
            for elem in mixer.iter() {
                let Some(selem) = Selem::new(elem) else {
                    continue;
                };
                if !selem.has_playback_volume() {
                    continue;
                }
                let sid = selem.get_id();
                let name = sid.get_name().unwrap_or("Unknown").to_string();
                return Ok(Self {
                    mixer,
                    selem_id: sid,
                    elem_name: name,
                });
            }

            Err(anyhow!("No ALSA playback volume control found"))
        }

        pub fn get(&self) -> Result<f32> {
            let selem = self
                .mixer
                .find_selem(&self.selem_id)
                .ok_or_else(|| anyhow!("ALSA element not found: {}", self.elem_name))?;

            if !selem.has_playback_volume() {
                return Err(anyhow!("ALSA element has no playback volume: {}", self.elem_name));
            }

            let (min, max) = selem.get_playback_volume_range();
            if max <= min {
                return Ok(0.0);
            }

            let channels = [
                SelemChannelId::FrontLeft,
                SelemChannelId::FrontRight,
                SelemChannelId::mono(),
            ];

            let mut raw = None;
            for ch in channels {
                if selem.has_playback_channel(ch) {
                    raw = Some(selem.get_playback_volume(ch)?);
                    break;
                }
            }

            let raw = raw.ok_or_else(|| anyhow!("No playback channel found for: {}", self.elem_name))?;
            let v = (raw - min) as f32 / (max - min) as f32;
            Ok(v.clamp(0.0, 1.0))
        }

        pub fn set(&self, volume: f32) -> Result<()> {
            let selem = self
                .mixer
                .find_selem(&self.selem_id)
                .ok_or_else(|| anyhow!("ALSA element not found: {}", self.elem_name))?;

            let (min, max) = selem.get_playback_volume_range();
            if max <= min {
                return Ok(());
            }

            let v = volume.clamp(0.0, 1.0);
            let raw = min + (((max - min) as f32) * v).round() as i64;
            selem.set_playback_volume_all(raw)?;
            Ok(())
        }

        pub fn set_delta(&self, delta: f32) -> Result<f32> {
            let cur = self.get().unwrap_or(0.0);
            let next = (cur + delta).clamp(0.0, 1.0);
            let _ = self.set(next);
            Ok(next)
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use anyhow::{anyhow, Result};

    pub struct SystemVolume;

    impl SystemVolume {
        pub fn try_new() -> Result<Self> {
            Err(anyhow!("System volume control is only supported on Linux"))
        }

        pub fn get(&self) -> Result<f32> {
            Err(anyhow!("System volume control is only supported on Linux"))
        }

        pub fn set(&self, _volume: f32) -> Result<()> {
            Err(anyhow!("System volume control is only supported on Linux"))
        }

        pub fn set_delta(&self, _delta: f32) -> Result<f32> {
            Err(anyhow!("System volume control is only supported on Linux"))
        }
    }
}

pub use imp::SystemVolume;
