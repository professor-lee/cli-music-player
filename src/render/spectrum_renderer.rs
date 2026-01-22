use crate::app::state::AppState;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &AppState) {
    let h = area.height as usize;
    let w = area.width as usize;
    if h == 0 || w == 0 {
        return;
    }

    // Bottom hint line (leave at least 1 row above for bars when possible).
    let bars_h = h.saturating_sub(1);
    if bars_h == 0 {
        return;
    }

    // Stretch 64 bars across the full width by mapping each column to a bar index.
    // This avoids unused space when area.width > 64.
    let bars = &app.spectrum.bars;
    let bar_count = 64usize;
    let mut grid: Vec<Vec<char>> = vec![vec![' '; w]; bars_h];

    let gap = app.config.bars_gap;
    for x in 0..w {
        if gap && (x % 2 == 1) {
            continue;
        }
        let i = ((x as u32) * (bar_count as u32) / (w as u32)) as usize;
        let i = i.min(bar_count - 1);
        let val = bars[i].clamp(0.0, 1.0);
        if app.config.super_smooth_bar {
            let fill = val * bars_h as f32;
            let full = fill.floor().clamp(0.0, bars_h as f32) as usize;
            let frac = (fill - full as f32).clamp(0.0, 1.0);

            for y in 0..bars_h {
                let row = bars_h - 1 - y;
                let ch = if y < full {
                    '█'
                } else if y == full {
                    smooth_char(frac)
                } else {
                    ' '
                };
                if ch != ' ' {
                    grid[row][x] = ch;
                }
            }
        } else {
            let bar_h = (val * bars_h as f32).round() as usize;
            for y in 0..bar_h.min(bars_h) {
                let row = bars_h - 1 - y;
                let ch = density_char(y, bar_h.max(1));
                grid[row][x] = ch;
            }
        }
    }

    // Render per-line vertical gradient using theme colors.
    let mut lines: Vec<Line> = Vec::with_capacity(bars_h + 1);
    for (row_idx, row) in grid.into_iter().enumerate() {
        let t = if bars_h <= 1 { 1.0 } else { row_idx as f32 / (bars_h - 1) as f32 };
        let fg = vertical_gradient_color(app, t);
        let s = row.into_iter().collect::<String>();
        lines.push(Line::from(Span::styled(s, Style::default().fg(fg))));
    }

    // bottom hint bar (same color as bars)
    let hint = "─".repeat(w);
    let fg = vertical_gradient_color(app, 1.0);
    lines.push(Line::from(Span::styled(hint, Style::default().fg(fg))));

    f.render_widget(Paragraph::new(lines), area);
}

fn density_char(level: usize, height: usize) -> char {
    // bottom dense, top light
    if height == 0 {
        return ' ';
    }
    if height == 1 {
        return '░';
    }
    let ratio = level as f32 / height as f32;
    if ratio < 0.25 {
        '█'
    } else if ratio < 0.50 {
        '▓'
    } else if ratio < 0.75 {
        '▒'
    } else {
        '░'
    }
}

fn smooth_char(frac: f32) -> char {
    // Order: " ▂▃▄▅▆▇█" (low to high)
    if frac <= 0.0 {
        ' '
    } else if frac < 1.0 / 7.0 {
        '▂'
    } else if frac < 2.0 / 7.0 {
        '▃'
    } else if frac < 3.0 / 7.0 {
        '▄'
    } else if frac < 4.0 / 7.0 {
        '▅'
    } else if frac < 5.0 / 7.0 {
        '▆'
    } else if frac < 6.0 / 7.0 {
        '▇'
    } else {
        '█'
    }
}

fn vertical_gradient_color(app: &AppState, t: f32) -> Color {
    // Top -> bottom
    // Use the theme's accent range for a clear vertical gradient.
    let top = app.theme.color_accent2();
    let bottom = app.theme.color_accent3();
    mix(top, bottom, t)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let r = (ar as f32 + (br as f32 - ar as f32) * t) as u8;
            let g = (ag as f32 + (bg as f32 - ag as f32) * t) as u8;
            let b = (ab as f32 + (bb as f32 - ab as f32) * t) as u8;
            Color::Rgb(r, g, b)
        }
        _ => a,
    }
}
