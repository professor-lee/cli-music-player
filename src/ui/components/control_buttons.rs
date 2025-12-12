use crate::app::state::{AppState, PlaybackState};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let play = match app.player.playback {
        PlaybackState::Playing => "[⏸]",
        _ => "[⏯]",
    };

    let line = Line::from(vec![
        Span::styled("[⏮︎] ", Style::default().fg(app.theme.color_text())),
        Span::styled(format!("{} ", play), Style::default().fg(app.theme.color_text())),
        Span::styled("[⏭] ", Style::default().fg(app.theme.color_text())),
    ]);

    f.render_widget(
        Paragraph::new(line)
            .style(Style::default())
            .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}
