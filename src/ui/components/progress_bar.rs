use crate::app::state::AppState;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::time::Duration;

pub fn render(f: &mut Frame, area: Rect, app: &AppState, pos: Duration, dur: Duration) {
    let w = area.width as usize;
    if w == 0 {
        return;
    }

    let ratio = if dur.as_secs_f32() > 0.0 {
        (pos.as_secs_f32() / dur.as_secs_f32()).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // knob moves on [0, w-1]
    let knob = if w <= 1 {
        0usize
    } else {
        (ratio * (w as f32 - 1.0)).round() as usize
    };

    let left = "─".repeat(knob);
    let right = if w > 0 { "─".repeat(w.saturating_sub(1 + knob)) } else { String::new() };

    let line = Line::from(vec![
        Span::styled(left, Style::default().fg(app.theme.color_accent2())),
        Span::styled("○", Style::default().fg(app.theme.color_accent())),
        Span::styled(right, Style::default().fg(app.theme.color_subtext())),
    ]);

    f.render_widget(Paragraph::new(line), area);
}
