use crate::app::state::PlayMode;
use crate::playback::local_player::LocalPlayer;
use crate::playback::mpris_client::MprisClient;

pub struct ModeManager {
    pub local: LocalPlayer,
    pub mpris: MprisClient,
}

impl ModeManager {
    pub fn new() -> Self {
        Self {
            local: LocalPlayer::new(),
            mpris: MprisClient::new(),
        }
    }

    pub fn pause_other(&mut self, target: PlayMode) {
        match target {
            PlayMode::LocalPlayback => {
                let _ = self.mpris.pause();
            }
            PlayMode::SystemMonitor => {
                let _ = self.local.pause();
            }
            PlayMode::Idle => {
                let _ = self.local.pause();
            }
        }
    }
}
