use crate::app::state::AppState;
use crate::data::config::VisualizeMode;
use crate::render::{oscilloscope_renderer, spectrum_renderer};
use crate::ui::borders::SOLID_BORDER;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, lyric_area: Rect, spectrum_area: Rect, app: &AppState) {
    // Right side: one outer border for both lyrics + spectrum (no divider between them).
    let outer = Rect {
        x: lyric_area.x,
        y: lyric_area.y,
        width: lyric_area.width,
        height: lyric_area.height.saturating_add(spectrum_area.height),
    };
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_set(SOLID_BORDER)
        .style(Style::default().fg(app.theme.color_subtext()));
    f.render_widget(outer_block, outer);

    let inner = outer.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let lyric_h = lyric_area.height.saturating_sub(2).min(inner.height);
    let lyric_inner = Rect { x: inner.x, y: inner.y, width: inner.width, height: lyric_h };
    let spectrum_inner = Rect {
        x: inner.x,
        y: inner.y + lyric_h,
        width: inner.width,
        height: inner.height.saturating_sub(lyric_h),
    };

    // lyrics (keep empty if no lyrics)
    let (l1, l2) = current_two_lines(app);
    if lyric_inner.height >= 1 && !l1.is_empty() {
        f.render_widget(
            Paragraph::new(l1)
                .style(Style::default().fg(app.theme.color_text()))
                .alignment(Alignment::Center),
            Rect { x: lyric_inner.x, y: lyric_inner.y, width: lyric_inner.width, height: 1 },
        );
    }
    if lyric_inner.height >= 2 && !l2.is_empty() {
        f.render_widget(
            Paragraph::new(l2)
                .style(Style::default().fg(app.theme.color_subtext()))
                .alignment(Alignment::Center),
            Rect { x: lyric_inner.x, y: lyric_inner.y + 1, width: lyric_inner.width, height: 1 },
        );
    }

    // spectrum (no border here; outer border already drawn)
    match app.config.visualize {
        VisualizeMode::Bars => spectrum_renderer::render(f, spectrum_inner, app),
        VisualizeMode::Oscilloscope => oscilloscope_renderer::render(f, spectrum_inner, app),
    }
}


fn current_two_lines(app: &AppState) -> (String, String) {
    let Some(lines) = app.player.track.lyrics.as_ref() else {
        return (String::new(), String::new());
    };
    if lines.is_empty() {
        return (String::new(), String::new());
    }

    let pos_ms = app.player.position.as_millis() as u64;
    let mut idx = 0;
    for (i, l) in lines.iter().enumerate() {
        if l.start_ms <= pos_ms {
            idx = i;
        } else {
            break;
        }
    }

    let l1 = lines.get(idx).map(|l| l.text.clone()).unwrap_or_default();
    let l2 = lines.get(idx + 1).map(|l| l.text.clone()).unwrap_or_default();
    (l1, l2)
}
