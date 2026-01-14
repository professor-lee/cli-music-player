use crate::app::state::{AppState, PlayMode, PlaybackState};
use crate::utils::input::Action;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let play = match app.player.playback {
        PlaybackState::Playing => "[]",
        _ => "[]",
    };

    // 需求：外部音源（SystemMonitor）固定显示“顺序播放()”
    let repeat_symbol = match app.player.mode {
        PlayMode::LocalPlayback => app.player.repeat_mode.symbol(),
        _ => "",
    };

    let line = Line::from(vec![
        Span::styled("[] ", Style::default().fg(app.theme.color_text())),
        Span::styled(format!("{} ", play), Style::default().fg(app.theme.color_text())),
        Span::styled("[] ", Style::default().fg(app.theme.color_text())),
        Span::styled(repeat_symbol, Style::default().fg(app.theme.color_subtext())),
    ]);

    f.render_widget(
        Paragraph::new(line)
            .style(Style::default())
            .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

pub fn hit_test(area: Rect, app: &AppState, col: u16, row: u16) -> Option<Action> {
    if row < area.y || row >= area.y + area.height {
        return None;
    }
    let play = match app.player.playback {
        PlaybackState::Playing => "[]",
        _ => "[]",
    };
    let repeat_symbol = match app.player.mode {
        PlayMode::LocalPlayback => app.player.repeat_mode.symbol(),
        _ => "",
    };

    // Must match render() exactly (including spaces) because we align by glyph width.
    let s_prev = "[]";
    let s_play = play;
    let s_next = "[]";
    let sep = " ";
    let label = format!("{s_prev}{sep}{s_play}{sep}{s_next}{sep}{repeat_symbol}");

    let text_w = UnicodeWidthStr::width(label.as_str()) as u16;
    if text_w == 0 || area.width == 0 {
        return None;
    }

    let start_x = area.x + area.width.saturating_sub(text_w) / 2;
    if col < start_x || col >= start_x + text_w {
        return None;
    }
    let mut x = start_x;

    let w_prev = UnicodeWidthStr::width(s_prev) as u16;
    if col >= x && col < x + w_prev {
        return Some(Action::Prev);
    }
    x += w_prev;
    x += 1; // sep

    let w_play = UnicodeWidthStr::width(s_play) as u16;
    if col >= x && col < x + w_play {
        return Some(Action::TogglePlayPause);
    }
    x += w_play;
    x += 1; // sep

    let w_next = UnicodeWidthStr::width(s_next) as u16;
    if col >= x && col < x + w_next {
        return Some(Action::Next);
    }
    x += w_next;
    x += 1; // sep

    let w_mode = UnicodeWidthStr::width(repeat_symbol) as u16;
    if col >= x && col < x + w_mode {
        // 需求：仅本地有效；外部音源点击不生效
        if app.player.mode == PlayMode::LocalPlayback {
            return Some(Action::ToggleRepeatMode);
        }
        return None;
    }

    None
}
