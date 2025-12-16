use crate::app::state::{AppState, Overlay};
use crate::ui::panels::{info_panel, playlist_panel, visual_panel};
use crate::ui::components::control_buttons;
use crate::utils::input::Action;
use anyhow::Result;
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{event, terminal};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;
use std::io::{self, Stdout};

#[derive(Debug, Default, Clone, Copy)]
pub struct UiLayout {
    pub full: Rect,
    pub left: Rect,
    pub right: Rect,
    pub left_width: u16,

    pub info_progress: Rect,
    pub info_volume: Rect,
    pub info_controls: Rect,

    pub playlist_rect: Rect,
    pub playlist_inner: Rect,
}

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    pub should_quit: bool,
}

impl Tui {
    pub fn new() -> Result<Self> {
        let stdout = io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal, should_quit: false })
    }

    pub fn enter(&mut self) -> Result<()> {
        execute!(io::stdout(), EnterAlternateScreen, event::EnableMouseCapture)?;
        terminal::enable_raw_mode()?;
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        terminal::disable_raw_mode()?;
        execute!(io::stdout(), event::DisableMouseCapture, LeaveAlternateScreen)?;
        Ok(())
    }

    pub fn draw(&mut self, app: &mut AppState) -> Result<UiLayout> {
        if app.toast.as_ref().map(|(m, _)| m.as_str()) == Some("Bye") {
            self.should_quit = true;
        }

        let mut layout_out = UiLayout::default();

        self.terminal.draw(|f| {
            let size = f.size();
            layout_out.full = size;

            // small terminal: keep stable, hide secondary panels
            if size.width < 50 || size.height < 12 {
                f.render_widget(ratatui::widgets::Clear, size);

                let mut base_style = Style::default().fg(app.theme.color_text());
                if !app.config.transparent_background {
                    base_style = base_style.bg(app.theme.color_base());
                }
                f.render_widget(
                    ratatui::widgets::Block::default()
                        .style(base_style),
                    size,
                );
                f.render_widget(
                    ratatui::widgets::Paragraph::new("Terminal too small")
                        .style(Style::default().fg(app.theme.color_subtext())),
                    size,
                );
                return;
            }

            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
                .split(size);
            layout_out.left = cols[0];
            layout_out.right = cols[1];
            layout_out.left_width = cols[0].width;

            // right: lyrics (10%) + spectrum (rest)
            let lyric_h = ((cols[1].height as f32) * 0.10).round() as u16;
            let lyric_h = lyric_h.clamp(3, cols[1].height.saturating_sub(6));
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(lyric_h), Constraint::Min(1)])
                .split(cols[1]);

            let info_l = info_panel::layout(cols[0]);
            layout_out.info_progress = info_l.progress;
            layout_out.info_volume = info_l.volume;
            layout_out.info_controls = info_l.controls;

            // base styling
            f.render_widget(ratatui::widgets::Clear, size);

            let mut base_style = Style::default().fg(app.theme.color_text());
            if !app.config.transparent_background {
                base_style = base_style.bg(app.theme.color_base());
            }
            f.render_widget(
                ratatui::widgets::Block::default()
                    .style(base_style),
                size,
            );

            info_panel::render(f, cols[0], app);
            visual_panel::render(f, rows[0], rows[1], app);

            // playlist overlay slides in/out over left
            if app.overlay == Overlay::Playlist || app.playlist_slide_x != app.playlist_slide_target_x {
                // advance animation
                let step: i16 = 4;
                if app.playlist_slide_x < app.playlist_slide_target_x {
                    app.playlist_slide_x = (app.playlist_slide_x + step).min(app.playlist_slide_target_x);
                } else if app.playlist_slide_x > app.playlist_slide_target_x {
                    app.playlist_slide_x = (app.playlist_slide_x - step).max(app.playlist_slide_target_x);
                }

                // Slide effect via visible width growth/shrink (x stays at left edge)
                let full_w = cols[0].width as i16;
                let visible_w = (full_w + app.playlist_slide_x).clamp(0, full_w) as u16;
                if visible_w > 0 {
                    let r = Rect {
                        x: cols[0].x,
                        y: cols[0].y,
                        width: visible_w,
                        height: cols[0].height,
                    };
                    layout_out.playlist_rect = r;
                    layout_out.playlist_inner = r.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
                    playlist_panel::render(f, r, app);
                }
            }

            // footer hint
            let footer = "Ctrl+K: Keys";
            let footer_area = Rect {
                x: size.x,
                y: size.y + size.height.saturating_sub(1),
                width: size.width,
                height: 1,
            };
            f.render_widget(
                ratatui::widgets::Paragraph::new(footer).style(Style::default().fg(app.theme.color_subtext())),
                footer_area,
            );

            // folder input overlay (simple one-line prompt)
            if app.overlay == Overlay::FolderInput {
                let prompt = format!("Folder: {}", app.folder_input.buf);
                let area = Rect {
                    x: size.x,
                    y: size.y + size.height.saturating_sub(2),
                    width: size.width,
                    height: 1,
                };
                f.render_widget(
                    ratatui::widgets::Paragraph::new(prompt)
                        .style(Style::default().fg(app.theme.color_text()).bg(app.theme.color_surface())),
                    area,
                );
            }

            // toast
            if let Some((msg, _)) = &app.toast {
                let area = Rect {
                    x: size.x,
                    y: size.y,
                    width: size.width,
                    height: 1,
                };
                f.render_widget(
                    ratatui::widgets::Paragraph::new(msg.as_str()).style(Style::default().fg(app.theme.color_accent3())),
                    area,
                );
            }

            // modals (top-most)
            match app.overlay {
                Overlay::SettingsModal => render_settings_modal(f, size, app),
                Overlay::HelpModal => render_help_modal(f, size, app),
                Overlay::EqModal => render_eq_modal(f, size, app),
                _ => {}
            }
        })?;

        Ok(layout_out)
    }
}

fn centered_rect(size: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(size.width.saturating_sub(4)).max(10);
    let h = height.min(size.height.saturating_sub(4)).max(6);
    Rect {
        x: size.x + (size.width.saturating_sub(w)) / 2,
        y: size.y + (size.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn render_settings_modal(f: &mut ratatui::Frame, size: Rect, app: &mut AppState) {
    let area = centered_rect(size, 44, 10);
    f.render_widget(ratatui::widgets::Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
            .border_set(crate::ui::borders::SOLID_BORDER)
        .title("Settings")
        .style(Style::default().fg(app.theme.color_subtext()).bg(app.theme.color_surface()));
    f.render_widget(block, area);

    let inner = area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(
        "Up/Down Select  Left/Right Change  Esc Close",
        Style::default().fg(app.theme.color_subtext()).bg(app.theme.color_surface()),
    ));
    lines.push(Line::styled("", Style::default().bg(app.theme.color_surface())));

    let items = [
        format!("Theme: {}", app.theme.name.as_label()),
        format!(
            "Transparent background: {}",
            if app.config.transparent_background { "On" } else { "Off" }
        ),
        format!("Album border: {}", if app.config.album_border { "On" } else { "Off" }),
        format!("UI FPS: {}", if app.config.ui_fps >= 60 { 60 } else { 30 }),
    ];

    for (idx, text) in items.iter().enumerate() {
        let style = if idx == app.settings_selected {
            Style::default()
                .fg(app.theme.color_base())
                .bg(app.theme.color_accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(app.theme.color_text())
                .bg(app.theme.color_surface())
        };
        lines.push(Line::styled(format!("  {}", text), style));
    }

    let p = Paragraph::new(lines)
        .style(Style::default().bg(app.theme.color_surface()))
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn render_help_modal(f: &mut ratatui::Frame, size: Rect, app: &mut AppState) {
    let area = centered_rect(size, 56, 13);
    f.render_widget(ratatui::widgets::Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
            .border_set(crate::ui::borders::SOLID_BORDER)
        .title("Keys")
        .style(Style::default().fg(app.theme.color_subtext()).bg(app.theme.color_surface()));
    f.render_widget(block, area);

    let inner = area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });

    let mut lines: Vec<Line> = Vec::new();
    let bg = Style::default().bg(app.theme.color_surface());
    let text = Style::default().fg(app.theme.color_text()).bg(app.theme.color_surface());
    let sub = Style::default().fg(app.theme.color_subtext()).bg(app.theme.color_surface());

    lines.push(Line::styled("Esc = Close", sub));
    lines.push(Line::styled("", bg));

    for l in [
        "Ctrl+F    Open folder",
        "P         Toggle playlist",
        "Space     Play/Pause",
        "Left/Right Prev/Next",
        "Up/Down   Volume",
        "M         Repeat mode (Local)",
        "E         Equalizer (Local)",
        "T         Settings",
        "Ctrl+K    This help",
        "Q         Quit",
    ] {
        lines.push(Line::styled(l, text));
    }

    let p = Paragraph::new(lines)
        .style(bg)
        .wrap(Wrap { trim: true });
    f.render_widget(p, inner);
}

fn render_eq_modal(f: &mut ratatui::Frame, size: Rect, app: &mut AppState) {
    // 需求：柱状条宽 2 格，高度 +12/-12（含 0 行共 25）
    // 额外预留：顶部提示 1 行 + 底部频率/数值 2 行
    let area = centered_rect(size, 44, 31);
    f.render_widget(ratatui::widgets::Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
            .border_set(crate::ui::borders::SOLID_BORDER)
        .title("Equalizer (Local)")
        .style(Style::default().fg(app.theme.color_subtext()).bg(app.theme.color_surface()));
    f.render_widget(block, area);

    let inner = area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });

    let bg = Style::default().bg(app.theme.color_surface());
    let sub = Style::default().fg(app.theme.color_subtext()).bg(app.theme.color_surface());
    let text = Style::default().fg(app.theme.color_text()).bg(app.theme.color_surface());
    let selected_bg = Style::default()
        .fg(app.theme.color_base())
        .bg(app.theme.color_accent())
        .add_modifier(Modifier::BOLD);

    // layout inside modal
    if inner.height < 3 {
        return;
    }
    let hint_rect = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let freq_label_rect = Rect { x: inner.x, y: inner.y + inner.height - 2, width: inner.width, height: 1 };
    let gain_label_rect = Rect { x: inner.x, y: inner.y + inner.height - 1, width: inner.width, height: 1 };
    let bars_rect = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };

    f.render_widget(
        Paragraph::new("Click/Up/Down adjust (auto)  Alt+R reset  Esc close")
            .style(sub)
            .wrap(Wrap { trim: true }),
        hint_rect,
    );

    // compute band geometry
    const BANDS: usize = crate::app::state::EQ_BANDS;
    const BAR_W: u16 = 2;
    const GAP: u16 = 1;

    fn fmt_db2(v: f32) -> String {
        let i = v.clamp(-12.0, 12.0).round() as i32;
        format!("{:+03}", i)
    }

    fn fmt_freq(freq_hz: f32) -> String {
        let f = freq_hz.round() as i32;
        if f >= 1000 {
            format!("{}k", f / 1000)
        } else {
            format!("{f}")
        }
    }

    let gains = app.eq.bands_db;
    let freq_labels: Vec<String> = crate::app::state::EQ_FREQS_HZ
        .iter()
        .map(|&f| fmt_freq(f))
        .collect();
    let gain_labels: Vec<String> = gains.iter().map(|&g| fmt_db2(g)).collect();

    // Fit columns to available width (10 bands should still render on typical terminals).
    let gaps_w = GAP.saturating_mul((BANDS as u16).saturating_sub(1));
    let mut cw = if bars_rect.width > gaps_w {
        (bars_rect.width - gaps_w) / (BANDS as u16)
    } else {
        BAR_W
    };
    cw = cw.clamp(BAR_W, 10);
    let total_w: u16 = cw.saturating_mul(BANDS as u16) + gaps_w;
    let x0 = bars_rect.x + (bars_rect.width.saturating_sub(total_w)) / 2;
    let gap = GAP;

    // fixed height: 25 rows => +12..0..-12
    let want_h: u16 = 25;
    let bars_h = if bars_rect.height >= want_h { want_h } else { bars_rect.height.max(3) };
    let y0 = bars_rect.y + (bars_rect.height.saturating_sub(bars_h)) / 2;

    // helper: map row index to db
    let row_to_db = |r: i32| -> i32 {
        if bars_h == want_h {
            // r: 0..24 => +12..-12
            12 - r
        } else {
            // fallback scale to +/-12
            let mid = (bars_h as i32) / 2;
            if r == mid {
                0
            } else if r < mid {
                let level = (mid - r) as f32;
                let max = mid.max(1) as f32;
                ((12.0 * (level / max)).round() as i32).clamp(0, 12)
            } else {
                let level = (r - mid) as f32;
                let max = (bars_h as i32 - 1 - mid).max(1) as f32;
                (-(12.0 * (level / max)).round() as i32).clamp(-12, 0)
            }
        }
    };

    let mut lines: Vec<Line> = Vec::with_capacity(bars_h as usize);
    for r in 0..bars_h {
        let rr = r as i32;
        let db_row = row_to_db(rr);

        let mut spans: Vec<ratatui::text::Span> = Vec::new();

        // left padding
        if x0 > bars_rect.x {
            spans.push(ratatui::text::Span::styled(
                " ".repeat((x0 - bars_rect.x) as usize),
                bg,
            ));
        }

        for b in 0..BANDS {
            let gain = gains[b].clamp(-12.0, 12.0).round() as i32;
            let filled = if db_row == 0 {
                false
            } else if db_row > 0 {
                // +1..+12: fill when row <= gain (e.g. gain=3 fills +1..+3)
                gain > 0 && db_row <= gain
            } else {
                // -1..-12: fill when row >= gain (e.g. gain=-5 fills -1..-5)
                gain < 0 && db_row >= gain
            };

            // Each column: center the 2-cell bar within fixed column width.
            let left_pad = cw.saturating_sub(BAR_W) / 2;
            let right_pad = cw.saturating_sub(BAR_W) - left_pad;
            let mut cell = String::new();
            cell.push_str(&" ".repeat(left_pad as usize));
            // 需求：零点(0dB)使用“▓▓”标识。
            if db_row == 0 {
                cell.push_str("▓▓");
            } else {
                cell.push_str(if filled { "██" } else { "░░" });
            }
            cell.push_str(&" ".repeat(right_pad as usize));
            if b + 1 < BANDS {
                cell.push_str(&" ".repeat(gap as usize));
            }

            // 需求：仅去除柱的选中效果（柱体不高亮）
            spans.push(ratatui::text::Span::styled(cell, text));
        }

        // right padding
        let drawn = (cw.saturating_mul(BANDS as u16) + gap.saturating_mul((BANDS as u16).saturating_sub(1)))
            + (x0 - bars_rect.x);
        if drawn < bars_rect.width {
            spans.push(ratatui::text::Span::styled(
                " ".repeat((bars_rect.width - drawn) as usize),
                bg,
            ));
        }

        lines.push(Line::from(spans));
    }

    let draw_rect = Rect {
        x: bars_rect.x,
        y: y0,
        width: bars_rect.width,
        height: bars_h,
    };
    f.render_widget(Paragraph::new(lines).style(bg).wrap(Wrap { trim: false }), draw_rect);

    // bottom labels (two lines): keep frequency + always show numeric gain.
    let mut freq_spans: Vec<ratatui::text::Span> = Vec::new();
    let mut gain_spans: Vec<ratatui::text::Span> = Vec::new();
    if x0 > bars_rect.x {
        let pad = " ".repeat((x0 - bars_rect.x) as usize);
        freq_spans.push(ratatui::text::Span::styled(pad.clone(), bg));
        gain_spans.push(ratatui::text::Span::styled(pad, bg));
    }
    for b in 0..BANDS {
        let style = if b == app.eq_selected { selected_bg } else { sub };

        let mut ftxt = freq_labels[b].clone();
        if unicode_width::UnicodeWidthStr::width(ftxt.as_str()) as u16 > cw {
            ftxt = ftxt.chars().take(cw as usize).collect();
        }
        let fpad = cw.saturating_sub(unicode_width::UnicodeWidthStr::width(ftxt.as_str()) as u16);
        let fleft = fpad / 2;
        let fright = fpad - fleft;
        let mut fcell = format!("{}{}{}", " ".repeat(fleft as usize), ftxt, " ".repeat(fright as usize));
        if b + 1 < BANDS {
            fcell.push_str(&" ".repeat(gap as usize));
        }
        freq_spans.push(ratatui::text::Span::styled(fcell, style));

        let mut gtxt = gain_labels[b].clone();
        if unicode_width::UnicodeWidthStr::width(gtxt.as_str()) as u16 > cw {
            gtxt = gtxt.chars().take(cw as usize).collect();
        }
        let gpad = cw.saturating_sub(unicode_width::UnicodeWidthStr::width(gtxt.as_str()) as u16);
        let gleft = gpad / 2;
        let gright = gpad - gleft;
        let mut gcell = format!("{}{}{}", " ".repeat(gleft as usize), gtxt, " ".repeat(gright as usize));
        if b + 1 < BANDS {
            gcell.push_str(&" ".repeat(gap as usize));
        }
        gain_spans.push(ratatui::text::Span::styled(gcell, style));
    }
    f.render_widget(Paragraph::new(Line::from(freq_spans)).style(bg), freq_label_rect);
    f.render_widget(Paragraph::new(Line::from(gain_spans)).style(bg), gain_label_rect);
}

pub fn hit_test(layout: &UiLayout, app: &AppState, col: u16, row: u16) -> Option<Action> {
    // Eq modal consumes clicks first
    if app.overlay == Overlay::EqModal {
        let area = centered_rect(layout.full, 44, 31);
        let inner = area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        if inner.height >= 3 {
            let bars_rect = Rect {
                x: inner.x,
                y: inner.y + 1,
                width: inner.width,
                height: inner.height.saturating_sub(3),
            };

            if contains(bars_rect, col, row) {
                const BANDS: usize = crate::app::state::EQ_BANDS;
                const BAR_W: u16 = 2;
                const GAP: u16 = 1;

                let gaps_w = GAP.saturating_mul((BANDS as u16).saturating_sub(1));
                let mut cw = if bars_rect.width > gaps_w {
                    (bars_rect.width - gaps_w) / (BANDS as u16)
                } else {
                    BAR_W
                };
                cw = cw.clamp(BAR_W, 10);
                let total_w: u16 = cw.saturating_mul(BANDS as u16) + gaps_w;
                let x0 = bars_rect.x + (bars_rect.width.saturating_sub(total_w)) / 2;
                if col < x0 || col >= x0 + total_w {
                    return None;
                }

                // Find band by fixed widths; then require click within the centered BAR_W region.
                let mut band: Option<usize> = None;
                for b in 0..BANDS {
                    let col_start = x0 + (b as u16) * (cw + GAP);
                    let col_end = col_start + cw;
                    if col >= col_start && col < col_end {
                        let left_pad = cw.saturating_sub(BAR_W) / 2;
                        let bar_start = col_start + left_pad;
                        let bar_end = bar_start + BAR_W;
                        if col < bar_start || col >= bar_end {
                            return None;
                        }
                        band = Some(b);
                        break;
                    }
                }

                let Some(band) = band else { return None; };

                // fixed height mapping: prefer 25 rows (12..0..-12)
                let want_h: u16 = 25;
                let bars_h = if bars_rect.height >= want_h { want_h } else { bars_rect.height.max(3) };
                let y0 = bars_rect.y + (bars_rect.height.saturating_sub(bars_h)) / 2;
                if row < y0 || row >= y0 + bars_h {
                    return None;
                }
                let rr = (row - y0) as i32;

                let db_i = if bars_h == want_h {
                    (12 - rr).clamp(-12, 12)
                } else {
                    let mid = (bars_h as i32) / 2;
                    if rr == mid {
                        0
                    } else if rr < mid {
                        let level = (mid - rr) as f32;
                        let max = mid.max(1) as f32;
                        ((12.0 * (level / max)).round() as i32).clamp(0, 12)
                    } else {
                        let level = (rr - mid) as f32;
                        let max = (bars_h as i32 - 1 - mid).max(1) as f32;
                        (-(12.0 * (level / max)).round() as i32).clamp(-12, 0)
                    }
                };

                return Some(Action::EqSetBandDb {
                    band,
                    db: db_i as f32,
                });
            }
        }
    }

    if contains(layout.info_controls, col, row) {
        return control_buttons::hit_test(layout.info_controls, app, col, row);
    }

    if contains(layout.info_volume, col, row) {
        return Some(Action::SetVolume(ratio_in_bar(layout.info_volume, col)));
    }

    if contains(layout.info_progress, col, row) {
        return Some(Action::SeekToFraction(ratio_in_track(layout.info_progress, col)));
    }

    if contains(layout.playlist_inner, col, row) {
        let idx = row.saturating_sub(layout.playlist_inner.y) as usize;
        return Some(Action::PlaylistSelect(idx));
    }

    None
}

fn contains(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

fn ratio_in_bar(r: Rect, col: u16) -> f32 {
    if r.width <= 2 {
        return 0.0;
    }
    let inner = (r.width - 2) as f32;
    let x = col.saturating_sub(r.x + 1) as f32;
    (x / inner).clamp(0.0, 1.0)
}

fn ratio_in_track(r: Rect, col: u16) -> f32 {
    if r.width <= 1 {
        return 0.0;
    }
    let denom = (r.width - 1) as f32;
    let x = col.saturating_sub(r.x) as f32;
    (x / denom).clamp(0.0, 1.0)
}
