use crate::app::state::{AppState, CoverSnapshot, PlayMode};
use crate::render::cover_cache::CoverKey;
use crate::ui::components::{control_buttons, progress_bar, volume_bar};
use crate::ui::borders::SOLID_BORDER;
use crate::utils::timefmt;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Default, Clone, Copy)]
pub struct InfoPanelLayout {
    pub inner: Rect,
    pub cover: Rect,
    pub progress: Rect,
    pub volume: Rect,
    pub controls: Rect,
    pub volume_label: Rect,
    pub time_line: Rect,
    pub sr_hint: Rect,
}

pub fn layout(area: Rect) -> InfoPanelLayout {
    // padding +1 char compared to previous (keep borders outside)
    let inner = area.inner(&ratatui::layout::Margin { horizontal: 2, vertical: 2 });

    // cover box: approximate visual square for terminal glyph aspect.
    // Baseline is height ≈ width/2; user request: expand vertically by +4 chars.
    let max_cover_h = inner.height.saturating_sub(10).max(3);
    let max_square_h_by_width = (inner.width / 2).max(3);
    let mut cover_h = max_square_h_by_width.min(max_cover_h);
    cover_h = cover_h.saturating_add(4).min(max_square_h_by_width).min(max_cover_h);
    let cover_w = (cover_h.saturating_mul(2)).min(inner.width).max(6);

    // stack height (using the fixed offsets below)
    let stack_h = cover_h
        .saturating_add(1) // gap
        .saturating_add(3) // title/artist/album
        .saturating_add(1) // gap
        .saturating_add(1) // time
        .saturating_add(1) // gap
        .saturating_add(1) // progress
        .saturating_add(1) // gap
        .saturating_add(1) // volume
        .saturating_add(1) // vol label
        .saturating_add(1) // gap
        .saturating_add(1) // controls
        .saturating_add(1); // S/R hint

    let top_pad = inner.height.saturating_sub(stack_h) / 2;
    let cover_y = inner.y.saturating_add(top_pad);
    let cover = Rect {
        x: inner.x + (inner.width.saturating_sub(cover_w)) / 2,
        y: cover_y,
        width: cover_w,
        height: cover_h,
    };

    let title_y = cover.y + cover.height + 1;

    let time_line = Rect { x: inner.x, y: title_y + 4, width: inner.width, height: 1 };
    let progress = Rect { x: inner.x, y: title_y + 6, width: inner.width, height: 1 };
    let volume = Rect { x: inner.x, y: title_y + 8, width: inner.width, height: 1 };
    let volume_label = Rect { x: inner.x, y: title_y + 9, width: inner.width, height: 1 };
    let controls = Rect { x: inner.x, y: title_y + 11, width: inner.width, height: 1 };
    let sr_hint = Rect { x: inner.x, y: title_y + 12, width: inner.width, height: 1 };

    InfoPanelLayout {
        inner,
        cover,
        progress,
        volume,
        controls,
        volume_label,
        time_line,
        sr_hint,
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &mut AppState) {
    let b = Block::default()
        .borders(Borders::ALL)
        .border_set(SOLID_BORDER)
        .title(" ")
        .style(Style::default().fg(app.theme.color_subtext()));
    f.render_widget(b, area);

    let l = layout(area);

    // cover (animated as a whole: content + border)
    if l.cover.width > 0 && l.cover.height > 0 {
        let show_border = app.config.album_border;

        if let Some(anim) = app.cover_anim.take() {
            let p = (app
                .last_frame
                .duration_since(anim.started_at)
                .as_secs_f32()
                / anim.duration.as_secs_f32())
            .clamp(0.0, 1.0);
            let offset = (p * l.cover.width as f32).round() as i16;

            let (from_box, from_fg) = cover_box_ascii_for_snapshot(
                &anim.from,
                l.cover.width,
                l.cover.height,
                show_border,
                app,
            );
            let (to_box, to_fg) = cover_box_ascii_for_snapshot(
                &anim.to,
                l.cover.width,
                l.cover.height,
                show_border,
                app,
            );

            let composed = compose_slide_cover(l.cover.width, l.cover.height, &from_box, &to_box, anim.dir, offset);
            let fg = if to_fg == app.theme.color_text() { to_fg } else { from_fg };
            f.render_widget(Paragraph::new(composed).style(Style::default().fg(fg)), l.cover);

            // restore animation (lifetime managed in tick)
            app.cover_anim = Some(anim);
        } else {
            let snap = CoverSnapshot::from(&app.player.track);
            let (box_ascii, fg) = cover_box_ascii_for_snapshot(
                &snap,
                l.cover.width,
                l.cover.height,
                show_border,
                app,
            );
            f.render_widget(Paragraph::new(box_ascii).style(Style::default().fg(fg)), l.cover);
        }
    }

    // metadata lines
    let title_y = l.cover.y + l.cover.height + 1;
    let inner_bottom = l.inner.y.saturating_add(l.inner.height);
    let can_render_meta = l.controls.y.saturating_add(l.controls.height) <= inner_bottom;

    if can_render_meta {
        let title = app.player.track.title.as_str();
        let artist = app.player.track.artist.as_str();
        let album = app.player.track.album.as_str();

        let text_style = Style::default().fg(app.theme.color_text());
        let sub_style = Style::default().fg(app.theme.color_subtext());

        let t = Paragraph::new(title).style(text_style).alignment(Alignment::Center);
        f.render_widget(t, Rect { x: l.inner.x, y: title_y, width: l.inner.width, height: 1 });
        let a = Paragraph::new(artist).style(sub_style).alignment(Alignment::Center);
        f.render_widget(a, Rect { x: l.inner.x, y: title_y + 1, width: l.inner.width, height: 1 });
        let al = Paragraph::new(album).style(sub_style).alignment(Alignment::Center);
        f.render_widget(al, Rect { x: l.inner.x, y: title_y + 2, width: l.inner.width, height: 1 });

        // time + progress
        let pos = app.player.position;
        let dur = app.player.track.duration;
        let left = timefmt::mmss(pos);
        let right = timefmt::mmss(dur);
        let time_line = format!(
            "{}{:>width$}",
            left,
            right,
            width = (l.inner.width as usize).saturating_sub(left.len())
        );
        f.render_widget(
            Paragraph::new(Line::from(time_line)).style(sub_style).alignment(Alignment::Center),
            l.time_line,
        );

        progress_bar::render(f, l.progress, app, pos, dur);
        volume_bar::render(f, l.volume, app, app.player.volume);

        let v_label = format!("Vol {}%", (app.player.volume * 100.0).round() as i32);
        f.render_widget(
            Paragraph::new(v_label).style(sub_style).alignment(Alignment::Left),
            l.volume_label,
        );

        control_buttons::render(f, l.controls, app);

        // (Removed S/R hint)
    }

    // header right status (theme + mode)
    let header = format!("[{}]  [Mode: {}]", app.theme.name.as_label(), mode_label(app.player.mode));
    let header_area = Rect { x: area.x + 2, y: area.y, width: area.width.saturating_sub(4), height: 1 };
    f.render_widget(
        Paragraph::new(header)
            .style(Style::default().fg(app.theme.color_subtext()))
            .alignment(Alignment::Right)
            .wrap(Wrap { trim: true }),
        header_area,
    );
}

fn cover_box_ascii_for_snapshot(
    snap: &CoverSnapshot,
    width: u16,
    height: u16,
    show_border: bool,
    app: &mut AppState,
) -> (String, ratatui::style::Color) {
    if width == 0 || height == 0 {
        return (String::new(), app.theme.color_subtext());
    }

    let mut grid: Vec<Vec<char>> = vec![vec![' '; width as usize]; height as usize];

    let (inner_x, inner_y, inner_w, inner_h) = if width >= 3 && height >= 3 {
        if show_border {
            // Border
            let tl = SOLID_BORDER.top_left.chars().next().unwrap_or(' ');
            let tr = SOLID_BORDER.top_right.chars().next().unwrap_or(' ');
            let bl = SOLID_BORDER.bottom_left.chars().next().unwrap_or(' ');
            let br = SOLID_BORDER.bottom_right.chars().next().unwrap_or(' ');
            let hch = SOLID_BORDER.horizontal_top.chars().next().unwrap_or(' ');
            let vl = SOLID_BORDER.vertical_left.chars().next().unwrap_or(' ');
            let vr = SOLID_BORDER.vertical_right.chars().next().unwrap_or(' ');

            grid[0][0] = tl;
            grid[0][(width - 1) as usize] = tr;
            grid[(height - 1) as usize][0] = bl;
            grid[(height - 1) as usize][(width - 1) as usize] = br;

            for x in 1..(width - 1) {
                grid[0][x as usize] = hch;
                grid[(height - 1) as usize][x as usize] = hch;
            }
            for y in 1..(height - 1) {
                grid[y as usize][0] = vl;
                grid[y as usize][(width - 1) as usize] = vr;
            }
        }

        // Always reserve the same inner content area, even when border is hidden.
        (1usize, 1usize, (width - 2) as usize, (height - 2) as usize)
    } else {
        // Too small to reserve padding; render full area.
        (0usize, 0usize, width as usize, height as usize)
    };

    let (inner_ascii, fg) = cover_ascii_for_snapshot(snap, inner_w as u16, inner_h as u16, app);
    let inner_lines = split_lines(&inner_ascii, inner_h);
    blit_xy(&mut grid, &inner_lines, inner_x as i16, inner_y as i16);

    let mut out = String::with_capacity((width as usize + 1) * height as usize);
    for row in grid {
        out.extend(row);
        out.push('\n');
    }
    (out, fg)
}

fn hash_track_seed(app: &AppState) -> u64 {
    let mut h = DefaultHasher::new();
    app.player.track.title.hash(&mut h);
    app.player.track.artist.hash(&mut h);
    app.player.track.album.hash(&mut h);
    h.finish()
}

fn hash_snapshot_seed(s: &CoverSnapshot) -> u64 {
    let mut h = DefaultHasher::new();
    s.title.hash(&mut h);
    s.artist.hash(&mut h);
    s.album.hash(&mut h);
    h.finish()
}

fn cover_ascii_for_snapshot(
    snap: &CoverSnapshot,
    width: u16,
    height: u16,
    app: &mut AppState,
) -> (String, ratatui::style::Color) {
    if let (Some(bytes), Some(hash)) = (snap.cover.as_deref(), snap.cover_hash) {
        let key = CoverKey { hash, width, height };
        let cached = { app.cover_cache.borrow_mut().get(key) };
        let ascii = match cached {
            Some(s) => s,
            None => {
                // Avoid heavy render on UI thread; enqueue background render and
                // return a cheap placeholder for this frame.
                app.queue_cover_ascii_render(key, bytes, '░');
                fill_ascii(width, height, '░')
            }
        };
        (ascii, app.theme.color_text())
    } else {
        let seed = hash_snapshot_seed(snap);
        let key = CoverKey { hash: seed, width, height };
        let cached = { app.cover_cache.borrow_mut().get(key) };
        let ascii = match cached {
            Some(s) => s,
            None => {
                let s = generate_random_cover_ascii(width, height, seed);
                {
                    let mut cache = app.cover_cache.borrow_mut();
                    cache.put(key, s);
                    cache.get(key).unwrap_or_default()
                }
            }
        };
        (ascii, app.theme.color_subtext())
    }
}

fn fill_ascii(width: u16, height: u16, ch: char) -> String {
    let row = ch.to_string().repeat(width as usize);
    let mut s = String::new();
    for _ in 0..height {
        s.push_str(&row);
        s.push('\n');
    }
    s
}

fn compose_slide_cover(
    width: u16,
    height: u16,
    from_ascii: &str,
    to_ascii: &str,
    dir: i8,
    offset: i16,
) -> String {
    let w = width as i16;
    let h = height as usize;

    let mut grid: Vec<Vec<char>> = vec![vec![' '; width as usize]; h];
    let from_lines = split_lines(from_ascii, h);
    let to_lines = split_lines(to_ascii, h);

    // Next: dir=-1, both move left. Prev: dir=+1, both move right.
    let (from_dx, to_dx) = if dir < 0 {
        (-offset, w - offset)
    } else {
        (offset, -w + offset)
    };

    blit(&mut grid, &from_lines, from_dx);
    blit(&mut grid, &to_lines, to_dx);

    let mut out = String::with_capacity((width as usize + 1) * h);
    for row in grid {
        out.extend(row);
        out.push('\n');
    }
    out
}

fn split_lines(s: &str, expected: usize) -> Vec<Vec<char>> {
    let mut out: Vec<Vec<char>> = Vec::with_capacity(expected);
    for line in s.lines() {
        out.push(line.chars().collect());
        if out.len() == expected {
            break;
        }
    }
    while out.len() < expected {
        out.push(Vec::new());
    }
    out
}

fn blit(dst: &mut [Vec<char>], src: &[Vec<char>], dx: i16) {
    let h = dst.len().min(src.len());
    if h == 0 {
        return;
    }
    let w = dst[0].len() as i16;
    for y in 0..h {
        for (x_src, ch) in src[y].iter().enumerate() {
            let x = x_src as i16 + dx;
            if x >= 0 && x < w {
                dst[y][x as usize] = *ch;
            }
        }
    }
}

fn blit_xy(dst: &mut [Vec<char>], src: &[Vec<char>], dx: i16, dy: i16) {
    let dst_h = dst.len() as i16;
    if dst_h == 0 {
        return;
    }
    let dst_w = dst[0].len() as i16;
    if dst_w == 0 {
        return;
    }

    for (y_src, row) in src.iter().enumerate() {
        let y = y_src as i16 + dy;
        if y < 0 || y >= dst_h {
            continue;
        }
        for (x_src, ch) in row.iter().enumerate() {
            let x = x_src as i16 + dx;
            if x >= 0 && x < dst_w {
                dst[y as usize][x as usize] = *ch;
            }
        }
    }
}

fn generate_random_cover_ascii(width: u16, height: u16, seed: u64) -> String {
    // Requirement: when the app has no response / no album cover available,
    // use a consistent solid fill instead of random characters.
    let _ = seed;
    let w = width as usize;
    let h = height as usize;
    let mut out = String::with_capacity((w + 1) * h);
    for _y in 0..h {
        for _x in 0..w {
            out.push('░');
        }
        out.push('\n');
    }
    out
}

fn mode_label(m: PlayMode) -> &'static str {
    match m {
        PlayMode::Idle => "Idle",
        PlayMode::LocalPlayback => "Local",
        PlayMode::SystemMonitor => "System",
    }
}

