use crate::app::state::{AppState, Overlay};
use crate::ui::panels::{info_panel, playlist_panel, visual_panel};
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
    let area = centered_rect(size, 44, 9);
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
        "M         Repeat mode",
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

pub fn hit_test(layout: &UiLayout, col: u16, row: u16) -> Option<Action> {
    if contains(layout.info_controls, col, row) {
        // 3 segments: prev, play/pause, next
        let w = layout.info_controls.width.max(1);
        let rel = col.saturating_sub(layout.info_controls.x);
        let seg = ((rel as u32) * 3 / (w as u32)) as u16;
        return match seg {
            0 => Some(Action::Prev),
            1 => Some(Action::TogglePlayPause),
            2 => Some(Action::Next),
            _ => None,
        };
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
