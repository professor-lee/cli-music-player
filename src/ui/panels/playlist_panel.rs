use crate::app::state::AppState;
use crate::ui::borders::SOLID_BORDER;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    // solid background for playlist overlay
    f.render_widget(ratatui::widgets::Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(SOLID_BORDER)
        .style(Style::default().fg(app.theme.color_subtext()).bg(app.theme.color_surface()))
        .title(format!("Playlist ({} tracks)", app.playlist.len()));
    f.render_widget(block, area);

    let inner = area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });

    let mut lines: Vec<Line> = Vec::new();
    for (i, it) in app.playlist.items.iter().enumerate() {
        let prefix = if app.playlist.current == Some(i) { "[>]" } else { "   " };
        let label = format!("{} {:02}. {}", prefix, i + 1, it.title);
        let mut style = Style::default()
            .fg(app.theme.color_text())
            .bg(app.theme.color_surface());
        if i == app.playlist.selected {
            style = Style::default()
                .fg(app.theme.color_base())
                .bg(app.theme.color_accent())
                .add_modifier(Modifier::BOLD);
        }
        lines.push(Line::styled(label, style));
    }

    if lines.is_empty() {
        lines.push(
            Line::styled(
                "(empty)",
                Style::default()
                    .fg(app.theme.color_subtext())
                    .bg(app.theme.color_surface()),
            ),
        );
    }

    // footer hint (as PRD mock)
    if !lines.is_empty() {
        lines.push(Line::styled(
            "",
            Style::default().bg(app.theme.color_surface()),
        ));
        lines.push(Line::styled(
            "[Enter] Play  [P] Hide",
            Style::default()
                .fg(app.theme.color_subtext())
                .bg(app.theme.color_surface()),
        ));
    }

    let p = Paragraph::new(lines)
        .style(Style::default().bg(app.theme.color_surface()))
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

