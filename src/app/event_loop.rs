use crate::app::mode_manager::ModeManager;
use crate::app::state::{AppState, CoverSnapshot, Overlay, PlayMode, PlaybackState, RepeatMode};
use crate::audio::capture::AudioCapture;
use crate::audio::cava::CavaRunner;
use crate::audio::spectrum::SpectrumProcessor;
use crate::data::theme_loader::ThemeLoader;
use crate::ui::tui::{Tui, UiLayout};
use crate::ui::theme::ThemeName;
use crate::utils::input::{map_key, map_mouse, Action};
use crate::utils::system_volume::SystemVolume;
use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::time::{Duration, Instant};

pub fn run(app: &mut AppState) -> Result<()> {
    enable_raw_mode()?;
    let mut tui = Tui::new()?;
    tui.enter()?;

    let mut mode_manager = ModeManager::new();

    // audio capture (best-effort: try monitor device)
    let mut audio_capture = AudioCapture::start()?;
    let mut spectrum = SpectrumProcessor::new(app.config.spectrum_hz, app.spectrum.fft_size);

    // Prefer cava for system-wide visualization (keeps our renderer/style; cava only provides bars).
    // If cava isn't installed, we fall back to the existing internal FFT pipeline.
    let cava = match CavaRunner::start(app.config.spectrum_hz) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!("cava unavailable; falling back to internal spectrum: {e}");
            None
        }
    };

    let system_volume = SystemVolume::try_new().ok();

    let mut last_spectrum = Instant::now();
    let mut last_mpris = Instant::now();

    let mut last_layout = UiLayout::default();

    loop {
        let frame_start = Instant::now();

        // poll input (non-blocking-ish)
        while event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Key(k) => {
                    let action = map_key(k, app.overlay);
                    handle_action(app, &mut mode_manager, system_volume.as_ref(), action, &last_layout)?;
                }
                Event::Mouse(m) => {
                    let action = map_mouse(m);
                    handle_action(app, &mut mode_manager, system_volume.as_ref(), action, &last_layout)?;
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        // mpris poll
        if frame_start.duration_since(last_mpris) >= Duration::from_millis(app.config.mpris_poll_ms) {
            last_mpris = frame_start;
            if let Ok(Some(snapshot)) = mode_manager.mpris.poll_snapshot() {
                let before_track = app.player.track.clone();

                // auto-switch to system monitor when system playback is active
                if snapshot.playback == PlaybackState::Playing && app.player.mode != PlayMode::SystemMonitor {
                    mode_manager.pause_other(PlayMode::SystemMonitor);
                    app.player.mode = PlayMode::SystemMonitor;
                }

                // Only let MPRIS overwrite state when we're monitoring system playback
                // (or when system playback is active and we just switched).
                if app.player.mode != PlayMode::LocalPlayback {
                    app.player.track = snapshot.track;
                    app.player.position = snapshot.position;
                    app.player.playback = snapshot.playback;

                    if let Some(sysvol) = system_volume.as_ref() {
                        if let Ok(v) = sysvol.get() {
                            app.player.volume = v;
                        } else {
                            app.player.volume = snapshot.volume;
                        }
                    } else {
                        app.player.volume = snapshot.volume;
                    }
                }

                // If user requested next/prev in SystemMonitor, animate when the track actually changes.
                if app.player.mode == PlayMode::SystemMonitor {
                    if let Some((from, dir, _at)) = app.pending_system_cover_anim.take() {
                        let changed = before_track.title != app.player.track.title
                            || before_track.artist != app.player.track.artist
                            || before_track.album != app.player.track.album
                            || before_track.cover_hash != app.player.track.cover_hash;
                        if changed {
                            let to = CoverSnapshot::from(&app.player.track);
                            app.start_cover_anim(from, to, dir, frame_start);
                        }
                    }
                }
            }
        }

        // spectrum update
        if frame_start.duration_since(last_spectrum)
            >= Duration::from_millis((1000 / app.config.spectrum_hz.max(1)) as u64)
        {
            last_spectrum = frame_start;

            if let Some(c) = cava.as_ref() {
                app.spectrum.bars = c.latest_bars();
            } else {
                if app.player.mode == PlayMode::SystemMonitor && app.player.playback == PlaybackState::Playing {
                    audio_capture.maybe_restart_for_system_playback(frame_start);
                }

                let samples = if app.player.mode == PlayMode::LocalPlayback {
                    mode_manager.local.latest_samples(app.spectrum.fft_size)
                } else {
                    audio_capture.latest_samples(app.spectrum.fft_size)
                };

                let bars = if samples.len() >= app.spectrum.fft_size / 4 {
                    spectrum.process(samples)
                } else {
                    fallback_bars(app.player.volume, app.player.playback)
                };
                app.spectrum.bars = bars;
            }
        }

        // local player position update
        if app.player.mode == PlayMode::LocalPlayback {
            // Detect end-of-track and stop position accumulation.
            let just_finished = mode_manager.local.poll_end();
            if just_finished {
                handle_local_track_finished(app, &mut mode_manager);
            }
            if let Some(pos) = mode_manager.local.position() {
                app.player.position = pos;
            }
            if let Some(dur) = mode_manager.local.duration() {
                app.player.track.duration = dur;
            }
            app.player.volume = mode_manager.local.volume();
            app.player.playback = mode_manager.local.playback_state();
        }

        if app.player.mode == PlayMode::SystemMonitor {
            if let Some(sysvol) = system_volume.as_ref() {
                if let Ok(v) = sysvol.get() {
                    app.player.volume = v;
                }
            }
        }

        app.tick(frame_start);

        // draw
        last_layout = tui.draw(app)?;

        // frame pacing
        let frame_dt = fps_to_dt(app.config.ui_fps);
        let elapsed = frame_start.elapsed();
        if elapsed < frame_dt {
            std::thread::sleep(frame_dt - elapsed);
        }

        if tui.should_quit {
            break;
        }
    }

    tui.exit()?;
    disable_raw_mode()?;
    Ok(())
}

fn fps_to_dt(fps: u32) -> Duration {
    let fps = fps.clamp(30, 60);
    Duration::from_millis((1000 / fps) as u64)
}

fn handle_local_track_finished(app: &mut AppState, mode_manager: &mut ModeManager) {
    // 自动续播仅用于本地播放。
    if app.player.mode != PlayMode::LocalPlayback {
        return;
    }
    if app.playlist.items.is_empty() {
        return;
    }

    let from = CoverSnapshot::from(&app.player.track);
    let next = match app.player.repeat_mode {
        RepeatMode::Sequence => app.playlist.next_index_no_wrap(),
        RepeatMode::LoopAll => app.playlist.next_index_sequence(),
        RepeatMode::LoopOne => app.playlist.current,
        RepeatMode::Shuffle => pick_shuffle_index(&app.playlist),
    };

    let Some(i) = next else {
        // Sequence mode at end: stop.
        app.player.playback = PlaybackState::Stopped;
        return;
    };

    app.playlist.current = Some(i);
    let Some(path) = app.playlist.current_path().cloned() else {
        app.player.playback = PlaybackState::Stopped;
        return;
    };

    match mode_manager.local.play_file(&path) {
        Ok(track) => {
            app.player.track = track;
            let to = CoverSnapshot::from(&app.player.track);
            app.start_cover_anim(from, to, -1, Instant::now());
        }
        Err(e) => {
            app.player.playback = PlaybackState::Stopped;
            app.set_toast(format!("Play error: {e}"));
        }
    }
}

fn handle_action(
    app: &mut AppState,
    mode_manager: &mut ModeManager,
    system_volume: Option<&SystemVolume>,
    action: Action,
    layout: &UiLayout,
) -> Result<()> {
    match action {
        Action::Quit => {
            // handled by tui flag
            app.set_toast("Bye");
        }
        Action::OpenFolder => {
            app.open_folder_input();
        }
        Action::OpenSettingsModal => {
            app.overlay = Overlay::SettingsModal;
            app.settings_selected = 0;
        }
        Action::OpenHelpModal => {
            app.overlay = Overlay::HelpModal;
        }
        Action::OpenEqModal => {
            // 需求：均衡器仅对本地音频播放生效
            if app.player.mode == PlayMode::LocalPlayback {
                app.overlay = Overlay::EqModal;
                app.eq_selected = 0;
            } else {
                app.set_toast("EQ only for local playback");
            }
        }
        Action::EqSetBandDb { band, db } => {
            if app.overlay == Overlay::EqModal {
                app.eq_selected = band.min(2);
                let db = db.clamp(-12.0, 12.0);
                match app.eq_selected {
                    0 => app.eq.low_db = db,
                    1 => app.eq.mid_db = db,
                    2 => app.eq.high_db = db,
                    _ => {}
                }

                // 需求：均衡器自动生效
                if app.player.mode == PlayMode::LocalPlayback {
                    let _ = mode_manager.local.set_eq(app.eq);
                }
            }
        }
        Action::FolderChar(c) => {
            app.folder_input.buf.push(c);
        }
        Action::FolderBackspace => {
            app.folder_input.buf.pop();
        }
        Action::CloseOverlay => {
            if app.overlay == Overlay::Playlist {
                // close animation will be driven by ui
                // actual state closed after fully slid out
                // here just set target
                app.playlist_slide_target_x = -(layout.left_width as i16);
                app.overlay = Overlay::None;
            } else {
                app.close_overlay();
            }
        }
        Action::TogglePlaylist => {
            if app.overlay == Overlay::Playlist {
                app.playlist_slide_target_x = -(layout.left_width as i16);
                app.overlay = Overlay::None;
            } else {
                app.overlay = Overlay::Playlist;
                app.playlist_slide_x = -(layout.left_width as i16);
                app.playlist_slide_target_x = 0;
            }
        }
        Action::Confirm => {
            match app.overlay {
                Overlay::FolderInput => {
                    let folder = app.folder_input.buf.trim().to_string();
                    app.close_overlay();
                    if folder.is_empty() {
                        return Ok(());
                    }
                    match mode_manager.local.load_folder(&folder) {
                        Ok((playlist, first_track)) => {
                            mode_manager.pause_other(PlayMode::LocalPlayback);
                            app.player.mode = PlayMode::LocalPlayback;
                            app.playlist = playlist;
                            app.player.track = first_track;
                            app.player.volume = mode_manager.local.volume();
                            app.player.playback = mode_manager.local.playback_state();
                        }
                        Err(e) => {
                            app.set_toast(format!("Folder error: {e}"));
                        }
                    }
                }
                Overlay::Playlist => {
                    app.playlist.set_current_selected();
                    if let Some(path) = app.playlist.current_path().cloned() {
                        if let Ok(track) = mode_manager.local.play_file(&path) {
                            app.player.mode = PlayMode::LocalPlayback;
                            app.player.track = track;
                        }
                    }
                }
                Overlay::SettingsModal => {
                    // Enter toggles boolean settings only.
                    match app.settings_selected {
                        1 => {
                            app.config.transparent_background = !app.config.transparent_background;
                            let _ = app.config.save();
                        }
                        2 => {
                            app.config.album_border = !app.config.album_border;
                            let _ = app.config.save();
                        }
                        _ => {}
                    }
                }
                Overlay::HelpModal => {
                    app.close_overlay();
                }
                Overlay::EqModal => {
                    app.close_overlay();
                }
                _ => {}
            }
        }
        Action::PlaylistUp => {
            app.playlist.move_up();
            app.playlist.clamp_selected();
        }
        Action::PlaylistDown => {
            app.playlist.move_down();
            app.playlist.clamp_selected();
        }
        Action::ModalUp => {
            if app.overlay == Overlay::SettingsModal {
                let count = 4;
                if app.settings_selected == 0 {
                    app.settings_selected = count - 1;
                } else {
                    app.settings_selected -= 1;
                }
            } else if app.overlay == Overlay::EqModal {
                let step = 1.0;
                match app.eq_selected {
                    0 => app.eq.low_db = (app.eq.low_db + step).clamp(-12.0, 12.0),
                    1 => app.eq.mid_db = (app.eq.mid_db + step).clamp(-12.0, 12.0),
                    2 => app.eq.high_db = (app.eq.high_db + step).clamp(-12.0, 12.0),
                    _ => {}
                }

                // 需求：均衡器自动生效
                if app.player.mode == PlayMode::LocalPlayback {
                    let _ = mode_manager.local.set_eq(app.eq);
                }
            }
        }
        Action::ModalDown => {
            if app.overlay == Overlay::SettingsModal {
                let count = 4;
                app.settings_selected = (app.settings_selected + 1) % count;
            } else if app.overlay == Overlay::EqModal {
                let step = 1.0;
                match app.eq_selected {
                    0 => app.eq.low_db = (app.eq.low_db - step).clamp(-12.0, 12.0),
                    1 => app.eq.mid_db = (app.eq.mid_db - step).clamp(-12.0, 12.0),
                    2 => app.eq.high_db = (app.eq.high_db - step).clamp(-12.0, 12.0),
                    _ => {}
                }

                // 需求：均衡器自动生效
                if app.player.mode == PlayMode::LocalPlayback {
                    let _ = mode_manager.local.set_eq(app.eq);
                }
            }
        }
        Action::ModalLeft => {
            if app.overlay == Overlay::SettingsModal {
                apply_settings_delta(app, -1);
            } else if app.overlay == Overlay::EqModal {
                let count = 3;
                if app.eq_selected == 0 {
                    app.eq_selected = count - 1;
                } else {
                    app.eq_selected -= 1;
                }
            }
        }
        Action::ModalRight => {
            if app.overlay == Overlay::SettingsModal {
                apply_settings_delta(app, 1);
            } else if app.overlay == Overlay::EqModal {
                let count = 3;
                app.eq_selected = (app.eq_selected + 1) % count;
            }
        }
        Action::PlaylistSelect(idx) => {
            if idx < app.playlist.len() {
                app.playlist.selected = idx;
                app.playlist.clamp_selected();

                // double click => play
                let now = Instant::now();
                if let Some((at, last_col, last_row)) = app.last_mouse_click {
                    if now.duration_since(at) <= Duration::from_millis(400) {
                        // same row (best-effort)
                        if last_row == (layout.playlist_inner.y + idx as u16) {
                            return handle_action(app, mode_manager, system_volume, Action::Confirm, layout);
                        }
                        let _ = last_col;
                    }
                }
                app.last_mouse_click = Some((now, 0, layout.playlist_inner.y + idx as u16));
            }
        }
        Action::TogglePlayPause => {
            match app.player.mode {
                PlayMode::LocalPlayback => {
                    // If the track finished (sink empty), Space should restart it.
                    if mode_manager.local.playback_state() == PlaybackState::Stopped {
                        if let Ok(Some(track)) = mode_manager.local.restart_current() {
                            app.player.track = track;
                        }
                    } else {
                        let _ = mode_manager.local.toggle_play_pause();
                    }

                    // Keep UI position in sync immediately (avoids visual jump on key press).
                    if let Some(pos) = mode_manager.local.position() {
                        app.player.position = pos;
                    }
                }
                PlayMode::SystemMonitor => {
                    let _ = mode_manager.mpris.toggle_play_pause();
                }
                PlayMode::Idle => {}
            }
        }
        Action::Prev => match app.player.mode {
            PlayMode::LocalPlayback => {
                let from = CoverSnapshot::from(&app.player.track);
                let i = match app.player.repeat_mode {
                    RepeatMode::Sequence => app.playlist.prev_index_no_wrap(),
                    RepeatMode::LoopAll => app.playlist.prev_index_sequence(),
                    RepeatMode::LoopOne => app.playlist.current,
                    RepeatMode::Shuffle => pick_shuffle_index(&app.playlist),
                };
                if let Some(i) = i {
                    app.playlist.current = Some(i);
                    if let Some(path) = app.playlist.current_path().cloned() {
                        if let Ok(track) = mode_manager.local.play_file(&path) {
                            app.player.track = track;
                            let to = CoverSnapshot::from(&app.player.track);
                            app.start_cover_anim(from, to, 1, Instant::now());
                        }
                    }
                }
            }
            PlayMode::SystemMonitor => {
                app.pending_system_cover_anim = Some((CoverSnapshot::from(&app.player.track), 1, Instant::now()));
                let _ = mode_manager.mpris.prev();
            }
            PlayMode::Idle => {}
        },
        Action::Next => match app.player.mode {
            PlayMode::LocalPlayback => {
                let from = CoverSnapshot::from(&app.player.track);
                let next = match app.player.repeat_mode {
                    RepeatMode::Sequence => app.playlist.next_index_no_wrap(),
                    RepeatMode::LoopAll => app.playlist.next_index_sequence(),
                    RepeatMode::LoopOne => app.playlist.current,
                    RepeatMode::Shuffle => pick_shuffle_index(&app.playlist),
                };
                if let Some(i) = next {
                    app.playlist.current = Some(i);
                    if let Some(path) = app.playlist.current_path().cloned() {
                        if let Ok(track) = mode_manager.local.play_file(&path) {
                            app.player.track = track;
                            let to = CoverSnapshot::from(&app.player.track);
                            app.start_cover_anim(from, to, -1, Instant::now());
                        }
                    }
                }
            }
            PlayMode::SystemMonitor => {
                app.pending_system_cover_anim = Some((CoverSnapshot::from(&app.player.track), -1, Instant::now()));
                let _ = mode_manager.mpris.next();
            }
            PlayMode::Idle => {}
        },
        Action::VolumeUp => match app.player.mode {
            PlayMode::LocalPlayback => {
                mode_manager.local.set_volume((mode_manager.local.volume() + 0.05).min(1.0));
            }
            PlayMode::SystemMonitor => {
                if let Some(sysvol) = system_volume {
                    if let Ok(v) = sysvol.set_delta(0.05) {
                        app.player.volume = v;
                    } else {
                        let _ = mode_manager.mpris.set_volume_delta(0.05);
                    }
                } else {
                    let _ = mode_manager.mpris.set_volume_delta(0.05);
                }
            }
            PlayMode::Idle => {}
        },
        Action::VolumeDown => match app.player.mode {
            PlayMode::LocalPlayback => {
                mode_manager.local.set_volume((mode_manager.local.volume() - 0.05).max(0.0));
            }
            PlayMode::SystemMonitor => {
                if let Some(sysvol) = system_volume {
                    if let Ok(v) = sysvol.set_delta(-0.05) {
                        app.player.volume = v;
                    } else {
                        let _ = mode_manager.mpris.set_volume_delta(-0.05);
                    }
                } else {
                    let _ = mode_manager.mpris.set_volume_delta(-0.05);
                }
            }
            PlayMode::Idle => {}
        },
        Action::SetVolume(v) => match app.player.mode {
            PlayMode::LocalPlayback => {
                mode_manager.local.set_volume(v);
            }
            PlayMode::SystemMonitor => {
                if let Some(sysvol) = system_volume {
                    if sysvol.set(v).is_ok() {
                        app.player.volume = v;
                    } else {
                        // delta setter exists; approximate absolute set
                        let cur = app.player.volume;
                        let _ = mode_manager.mpris.set_volume_delta(v - cur);
                    }
                } else {
                    // delta setter exists; approximate absolute set
                    let cur = app.player.volume;
                    let _ = mode_manager.mpris.set_volume_delta(v - cur);
                }
            }
            PlayMode::Idle => {}
        },
        Action::ToggleRepeatMode => {
            // 需求：循环模式仅对本地音频有效；系统(MPRIS)来源固定显示“顺序(⇔)”且不受 m 影响。
            if app.player.mode == PlayMode::LocalPlayback {
                app.player.repeat_mode = app.player.repeat_mode.next();
            }
        }
        Action::SeekToFraction(r) => {
            let dur = app.player.track.duration;
            if dur.as_millis() == 0 {
                return Ok(());
            }
            let target = Duration::from_secs_f32(dur.as_secs_f32() * r.clamp(0.0, 1.0));
            match app.player.mode {
                PlayMode::LocalPlayback => {
                    if mode_manager.local.seek(target).is_ok() {
                        // Update UI immediately so the next user action (e.g. Space) doesn't look like a jump.
                        app.player.position = target;
                    }
                }
                PlayMode::SystemMonitor => {
                    let _ = mode_manager.mpris.seek_to(target);
                }
                PlayMode::Idle => {}
            }
        }
        Action::MouseClick { col, row } => {
            // map click to controls/progress/volume/playlist
            if let Some(a) = crate::ui::tui::hit_test(layout, app, col, row) {
                handle_action(app, mode_manager, system_volume, a, layout)?;
            }
        }
        Action::None => {}
    }

    Ok(())
}

fn themes() -> [ThemeName; 5] {
    [
        ThemeName::System,
        ThemeName::Latte,
        ThemeName::Frappe,
        ThemeName::Macchiato,
        ThemeName::Mocha,
    ]
}

fn theme_count() -> usize {
    themes().len()
}

fn theme_index(name: ThemeName) -> usize {
    themes().iter().position(|&t| t == name).unwrap_or(0)
}

fn theme_by_index(idx: usize) -> ThemeName {
    let t = themes();
    t[idx.min(t.len().saturating_sub(1))]
}

fn theme_key(name: ThemeName) -> &'static str {
    match name {
        ThemeName::System => "system",
        ThemeName::Latte => "latte",
        ThemeName::Frappe => "frappe",
        ThemeName::Macchiato => "macchiato",
        ThemeName::Mocha => "mocha",
    }
}

fn apply_settings_delta(app: &mut AppState, delta: i32) {
    match app.settings_selected {
        // Theme
        0 => {
            let count = theme_count() as i32;
            if count <= 0 {
                return;
            }
            let cur = theme_index(app.theme.name) as i32;
            let next = (cur + delta).rem_euclid(count) as usize;
            let name = theme_by_index(next);
            let key = theme_key(name);
            if let Ok(theme) = ThemeLoader::load(key) {
                app.theme = theme;
                app.config.theme = key.to_string();
                let _ = app.config.save();
            } else {
                app.set_toast("Theme load error");
            }
        }
        // Transparent background
        1 => {
            if delta != 0 {
                app.config.transparent_background = !app.config.transparent_background;
                let _ = app.config.save();
            }
        }
        // Album border
        2 => {
            if delta != 0 {
                app.config.album_border = !app.config.album_border;
                let _ = app.config.save();
            }
        }
        // UI FPS
        3 => {
            if delta != 0 {
                app.config.ui_fps = if app.config.ui_fps >= 60 { 30 } else { 60 };
                let _ = app.config.save();
            }
        }
        _ => {}
    }
}

fn pick_shuffle_index(pl: &crate::data::playlist::Playlist) -> Option<usize> {
    if pl.items.is_empty() {
        return None;
    }
    let len = pl.items.len();
    if len == 1 {
        return Some(0);
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut idx = (nanos as usize) % len;
    if Some(idx) == pl.current {
        idx = (idx + 1) % len;
    }
    Some(idx)
}

fn fallback_bars(volume: f32, playback: PlaybackState) -> [f32; 64] {
    // Best-effort visual fallback when no audio capture is available.
    // Keep it subtle and animated; scale by volume and playback state.
    let mut out = [0.0f32; 64];
    if playback != PlaybackState::Playing {
        return out;
    }

    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f32();
    let base = (0.15 + 0.60 * volume.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    for i in 0..64 {
        let x = i as f32 / 64.0;
        let a = (t * 2.3 + x * 8.0).sin().abs();
        let b = (t * 1.1 + x * 3.0).cos().abs();
        out[i] = (base * (0.25 + 0.75 * (0.6 * a + 0.4 * b))).clamp(0.0, 1.0);
    }
    out
}
