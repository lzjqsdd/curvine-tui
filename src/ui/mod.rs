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
pub mod icons;
pub mod jobs;
pub mod mounts;
pub mod bench;
pub mod fsck;
pub mod nodes;
pub mod transfers;
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
        Panel::Mounts => mounts::render(f, app, chunks[1]),
        Panel::Jobs => jobs::render(f, app, chunks[1]),
        Panel::Transfers => transfers::render(f, app, chunks[1]),
        Panel::Fsck => fsck::render(f, app, chunks[1]),
        Panel::Bench => bench::render(f, app, chunks[1]),
        Panel::Config => jobs::render_config(f, app, chunks[1]),
    }

    render_status(f, app, chunks[2]);
    render_hints(f, app, chunks[3]);

    // Blocking dialogs sit above panel content and below help.
    let progress = app.anim.dialog_progress();
    if let Some(pending) = &app.confirm {
        components::render_dialog(f, f.area(), &confirm_to_dialog(pending), progress);
    } else if let Some(view) = &app.describe {
        components::render_dialog(f, f.area(), &mounts::describe_to_dialog(view), progress);
    }

    if app.help_visible {
        components::render_help_popup(f, f.area());
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    // The tab bar degrades gracefully: full titles, then short titles, then
    // bare numbers. Without this, panels past the width limit silently vanish.
    let inner = area.width.saturating_sub(2) as usize;
    let long: Vec<String> = Panel::ALL
        .iter()
        .enumerate()
        .map(|(i, p)| format!(" {} {} {} ", i + 1, p.icon(), p.title()))
        .collect();
    let short: Vec<String> = Panel::ALL
        .iter()
        .enumerate()
        .map(|(i, p)| format!(" {} {} {} ", i + 1, p.icon(), p.short_title()))
        .collect();
    let icons_only: Vec<String> = Panel::ALL
        .iter()
        .enumerate()
        .map(|(i, p)| format!(" {} {} ", i + 1, p.icon()))
        .collect();
    let minimal: Vec<String> = Panel::ALL
        .iter()
        .enumerate()
        .map(|(i, _)| format!(" {} ", i + 1))
        .collect();

    let width = |v: &[String]| v.iter().map(|s| s.chars().count()).sum::<usize>();
    let labels = if width(&long) <= inner {
        long
    } else if width(&short) <= inner {
        short
    } else if width(&icons_only) <= inner {
        icons_only
    } else {
        minimal
    };
    let titles: Vec<Line> = labels.into_iter().map(Line::from).collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(th::BORDER))
                // The active panel name lives in the border so it stays
                // readable even when the tab strip falls back to bare numbers.
                .title(Line::from(vec![
                    Span::styled(
                        " curvine ",
                        Style::default()
                            .fg(th::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("· ", Style::default().fg(th::BORDER)),
                    Span::styled(
                        app.panel.title(),
                        Style::default()
                            .fg(th::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" ", Style::default()),
                ]))
                .title_alignment(ratatui::layout::Alignment::Left),
        )
        // Labels already carry their own spacing; ratatui's default 1-cell
        // left/right padding would overflow the bar and hide trailing panels.
        .padding("", "")
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
    // Status messages fade out rather than vanishing, which makes a transient
    // error readable even if the user looked away for a second.
    let alpha = app.anim.status_alpha();
    let faded = |c: ratatui::style::Color| crate::anim::lerp_color(th::MUTED, c, alpha);
    let (text, style) = if app.status_is_error {
        (
            app.status.clone(),
            Style::default().fg(faded(th::ERROR)),
        )
    } else if app.status.is_empty() {
        let masters = if app.cfg.master_addrs.is_empty() {
            "no masters configured — pass --master-addrs or --conf".to_string()
        } else {
            format!("masters: {}", app.cfg.master_addrs.join(", "))
        };
        (masters, Style::default().fg(th::MUTED))
    } else {
        (app.status.clone(), Style::default().fg(faded(th::OK)))
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
        let buf = if app.panel == Panel::Config {
            &app.config_filter
        } else {
            &app.files.filter
        };
        let line = Line::from(vec![
            Span::styled(" /", Style::default().fg(th::ACCENT)),
            Span::raw(buf.clone()),
            Span::styled("▏", Style::default().fg(th::ACCENT)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    if app.mode == InputMode::Mount {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "tab next field   ←→ change   space toggle   enter mount   esc cancel",
                Style::default().fg(th::MUTED),
            ))),
            area,
        );
        return;
    }
    if matches!(app.mode, InputMode::Confirm | InputMode::Describe) {
        // The dialog itself renders its footers; a second copy here would just
        // be noise.
        return;
    }
    if app.mode == InputMode::Transfer {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "tab switch field   enter transfer   esc cancel",
                Style::default().fg(th::MUTED),
            ))),
            area,
        );
        return;
    }

    let hints = match app.panel {
        Panel::Files => "↑↓ move  ⏎ enter  ⌫ up  / filter  d describe  Del del  D del -r  p put  g get  ? help",
        Panel::Transfers => "↑↓ select x cancel c clear p put g get r refresh ? help q quit",
        Panel::Mounts => "↑↓ select  n new mount  u unmount  r refresh  ? help  q quit",
        Panel::Fsck => "enter scan  x cancel  f target=cwd  r rescan  ? help  q quit",
        Panel::Nodes => "↑↓ select  d describe  r refresh  ? help  q quit",
        Panel::Config => "/ filter keys  r reload  ? help  q quit",
        Panel::Bench => "enter run  p profile  t threads  d duration  r rerun  ? help  q quit",
        _ => "tab panels 1-9 jump p put g get r refresh : cmd ? help q quit",
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

/// Build the confirmation dialog for a pending destructive action.
pub fn confirm_to_dialog(p: &crate::app::PendingConfirm) -> components::Dialog {
    let mut dlg = components::Dialog::new(&p.title)
        .headline(p.target.clone(), components::Tone::Error)
        .destructive(true);
    for line in &p.lines {
        if line.is_empty() {
            continue;
        }
        // Lines already formatted as "label  value" are split so the value
        // aligns with the rest of the dialog grid.
        match line.split_once("  ") {
            Some((k, v)) if k.len() <= 12 && !k.contains(' ') => {
                dlg = dlg.row(k.trim().to_string(), v.trim().to_string());
            }
            _ => dlg = dlg.note(line.clone(), components::Tone::Warn),
        }
    }
    dlg
}

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
