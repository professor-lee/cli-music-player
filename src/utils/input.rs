use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::app::state::Overlay;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Quit,
    TogglePlayPause,
    Prev,
    Next,
    VolumeUp,
    VolumeDown,
    SetVolume(f32),
    ToggleRepeatMode,
    TogglePlaylist,
    Confirm,
    CloseOverlay,
    OpenFolder,

    OpenSettingsModal,
    OpenHelpModal,

    OpenEqModal,

    EqResetDefault,

    EqSetBandDb { band: usize, db: f32 },

    ModalUp,
    ModalDown,

    ModalLeft,
    ModalRight,

    PlaylistUp,
    PlaylistDown,
    PlaylistMoveItemUp,
    PlaylistMoveItemDown,
    PlaylistSelect(usize),

    PrevAlbum,
    NextAlbum,

    SeekToFraction(f32),

    FolderChar(char),
    FolderBackspace,

    MouseClick { col: u16, row: u16 },

    None,
}

pub fn map_key(ev: KeyEvent, overlay: Overlay) -> Action {
    if overlay == Overlay::FolderInput {
        if ev.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(ev.code, KeyCode::Char('f') | KeyCode::Char('F'))
        {
            return Action::CloseOverlay;
        }
        match ev.code {
            KeyCode::Esc => return Action::CloseOverlay,
            KeyCode::Enter => return Action::Confirm,
            KeyCode::Backspace => return Action::FolderBackspace,
            KeyCode::Char(c) => return Action::FolderChar(c),
            KeyCode::Left => return Action::None,
            KeyCode::Right => return Action::None,
            KeyCode::Up => return Action::None,
            KeyCode::Down => return Action::None,
            _ => {}
        }
        return Action::None;
    }

    if overlay == Overlay::AcoustIdModal {
        match ev.code {
            KeyCode::Esc => return Action::CloseOverlay,
            KeyCode::Enter => return Action::Confirm,
            KeyCode::Backspace => return Action::FolderBackspace,
            KeyCode::Char(c) => return Action::FolderChar(c),
            KeyCode::Left => return Action::None,
            KeyCode::Right => return Action::None,
            KeyCode::Up => return Action::None,
            KeyCode::Down => return Action::None,
            _ => {}
        }
        return Action::None;
    }

    // modal-specific handling first
    if overlay == Overlay::SettingsModal {
        return match ev.code {
            KeyCode::Esc => Action::CloseOverlay,
            KeyCode::Char('t') | KeyCode::Char('T') => Action::CloseOverlay,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Up => Action::ModalUp,
            KeyCode::Down => Action::ModalDown,
            KeyCode::Left => Action::ModalLeft,
            KeyCode::Right => Action::ModalRight,
            _ => Action::None,
        };
    }

    if overlay == Overlay::BarSettingsModal {
        return match ev.code {
            KeyCode::Esc => Action::CloseOverlay,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Up => Action::ModalUp,
            KeyCode::Down => Action::ModalDown,
            KeyCode::Left => Action::ModalLeft,
            KeyCode::Right => Action::ModalRight,
            _ => Action::None,
        };
    }

    if overlay == Overlay::LocalAudioSettingsModal {
        return match ev.code {
            KeyCode::Esc => Action::CloseOverlay,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Up => Action::ModalUp,
            KeyCode::Down => Action::ModalDown,
            KeyCode::Left => Action::ModalLeft,
            KeyCode::Right => Action::ModalRight,
            _ => Action::None,
        };
    }

    if overlay == Overlay::EqModal {
        // Alt+R: reset EQ to default
        if ev.modifiers.contains(KeyModifiers::ALT) {
            if matches!(ev.code, KeyCode::Char('r') | KeyCode::Char('R')) {
                return Action::EqResetDefault;
            }
        }

        return match ev.code {
            KeyCode::Esc => Action::CloseOverlay,
            KeyCode::Char('e') | KeyCode::Char('E') => Action::CloseOverlay,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Up => Action::ModalUp,
            KeyCode::Down => Action::ModalDown,
            KeyCode::Left => Action::ModalLeft,
            KeyCode::Right => Action::ModalRight,
            _ => Action::None,
        };
    }

    if overlay == Overlay::HelpModal {
        if ev.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(ev.code, KeyCode::Char('k') | KeyCode::Char('K'))
        {
            return Action::CloseOverlay;
        }
        return match ev.code {
            KeyCode::Esc => Action::CloseOverlay,
            _ => Action::None,
        };
    }

    if overlay == Overlay::AboutModal {
        return match ev.code {
            KeyCode::Esc => Action::CloseOverlay,
            _ => Action::None,
        };
    }

    // global shortcuts (except folder input)
    match ev.code {
        KeyCode::Char('t') | KeyCode::Char('T') => return Action::OpenSettingsModal,
        KeyCode::Char('e') | KeyCode::Char('E') => return Action::OpenEqModal,
        _ => {}
    }

    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        match ev.code {
            KeyCode::Char('f') | KeyCode::Char('F') => return Action::OpenFolder,
            KeyCode::Char('k') | KeyCode::Char('K') => return Action::OpenHelpModal,
            _ => {}
        }
    }

    if overlay == Overlay::Playlist {
        return match ev.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => Action::Quit,
            KeyCode::Char('p') | KeyCode::Char('P') => Action::TogglePlaylist,
            KeyCode::Esc => Action::CloseOverlay,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Left => {
                if ev.modifiers.contains(KeyModifiers::CONTROL) {
                    Action::PrevAlbum
                } else {
                    Action::None
                }
            }
            KeyCode::Right => {
                if ev.modifiers.contains(KeyModifiers::CONTROL) {
                    Action::NextAlbum
                } else {
                    Action::None
                }
            }
            KeyCode::Up => {
                if ev.modifiers.contains(KeyModifiers::CONTROL) {
                    Action::PlaylistMoveItemUp
                } else {
                    Action::PlaylistUp
                }
            }
            KeyCode::Down => {
                if ev.modifiers.contains(KeyModifiers::CONTROL) {
                    Action::PlaylistMoveItemDown
                } else {
                    Action::PlaylistDown
                }
            }
            _ => Action::None,
        };
    }

    match ev.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Action::Quit,
        KeyCode::Char('p') | KeyCode::Char('P') => Action::TogglePlaylist,
        KeyCode::Char('m') | KeyCode::Char('M') => Action::ToggleRepeatMode,
        KeyCode::Esc => Action::CloseOverlay,
        KeyCode::Enter => Action::Confirm,
        KeyCode::Left => Action::Prev,
        KeyCode::Right => Action::Next,
        KeyCode::Up => Action::VolumeUp,
        KeyCode::Down => Action::VolumeDown,
        KeyCode::Char(' ') => Action::TogglePlayPause,
        _ => Action::None,
    }
}

pub fn map_mouse(ev: MouseEvent) -> Action {
    if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
        return Action::MouseClick {
            col: ev.column,
            row: ev.row,
        };
    }
    Action::None
}
