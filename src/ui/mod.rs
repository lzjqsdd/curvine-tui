//! Rendering. `render` is the single entry point called from the main loop.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, InputMode, Panel};

pub mod components;
pub mod dashboard;
pub mod files;
pub mod jobs;
pub mod nodes;
pub mod theme;

use theme as th;

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header / tabs
            Constraint::Min(3),    // main area
            Constraint::Length(1), // status
            Constraint::Length(1), // key hints
        ])
        .split(f.area());

    render_header(f, app, chunks[0]);

    match app.panel {
        Panel::Dashboard => dashboard::render(f, app, chunks[1]),
        Panel::Files => files::render(f, app, chunks[1]),
        Panel::Nodes => nodes::render(f, app, chunks[1]),
        Panel::Mounts => jobs::render_mounts(f, app, chunks[1]),
        Panel::Jobs => jobs::render(f, app, chunks[1]),
        Panel::Config => jobs::render_config(f, app, chunks[1]),
    }

    render_status(f, app, chunks[2]);
    render_hints(f, app, chunks[3]);

    if app.help_visible {
        components::render_help_popup(f, f.area());
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Panel::ALL
        .iter()
        .enumerate()
        .map(|(i, p)| Line::from(format!(" {} {} ", i + 1, p.title())))
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(th::BORDER))
                .title(Span::styled(
                    " curvine ",
                    Style::default()
                        .fg(th::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ))
                .title_alignment(ratatui::layout::Alignment::Left),
        )
        .select(app.panel.index())
        .highlight_style(
            Style::default()
                .fg(th::ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
        .divider("");

    f.render_widget(tabs, area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let (text, style) = if app.status_is_error {
        (app.status.clone(), Style::default().fg(th::ERROR))
    } else if app.status.is_empty() {
        (
            format!("masters: {}", app.cfg.master_addrs.join(", ")),
            Style::default().fg(th::MUTED),
        )
    } else {
        (app.status.clone(), Style::default().fg(th::OK))
    };

    f.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), area);
}

fn render_hints(f: &mut Frame, app: &App, area: Rect) {
    if app.mode == InputMode::Command {
        let line = Line::from(vec![
            Span::styled(" :", Style::default().fg(th::ACCENT)),
            Span::raw(app.command.clone()),
            Span::styled("▏", Style::default().fg(th::ACCENT)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    if app.mode == InputMode::Filter {
        let line = Line::from(vec![
            Span::styled(" /", Style::default().fg(th::ACCENT)),
            Span::raw(app.files.filter.clone()),
            Span::styled("▏", Style::default().fg(th::ACCENT)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    let hints = match app.panel {
        Panel::Files => "↑↓ move  ⏎ enter  ⌫ up  / filter  m mkdir  d rm  D rm -r  r refresh  ? help  q quit",
        _ => "tab next panel  1-6 jump  r refresh  : command  ? help  q quit",
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hints,
            Style::default().fg(th::MUTED),
        ))),
        area,
    );
}

// Small shared formatting helpers used by every panel.

pub fn human_bytes(n: i64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n <= 0 {
        return "0 B".to_string();
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

pub fn format_mtime(ms: i64) -> String {
    if ms <= 0 {
        return "-".to_string();
    }
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}

pub fn format_mode(mode: u32, is_dir: bool) -> String {
    let ty = if is_dir { 'd' } else { '-' };
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    let mut s = String::with_capacity(10);
    s.push(ty);
    for (mask, ch) in bits {
        s.push(if mode & mask != 0 { ch } else { '-' });
    }
    s
}

pub fn ratio(used: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        (used as f64 / total as f64).clamp(0.0, 1.0)
    }
}
