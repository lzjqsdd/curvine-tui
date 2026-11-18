//! Rendering. `render` is the single entry point called from the main loop.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, InputMode, Panel};

pub mod components;
pub mod dashboard;
pub mod files;
pub mod icons;
pub mod jobs;
pub mod menu;
pub mod mounts;
pub mod bench;
pub mod fsck;
pub mod nodes;
pub mod transfers;
pub mod theme;

use components::Tone;
use theme as th;

pub fn render(f: &mut Frame, app: &App) {
    // The command line needs one extra row above the footer while it is open;
    // every other mode gets the same single-line chrome.
    let command_hint = u16::from(app.mode == InputMode::Command);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(3),    // main area
            Constraint::Length(command_hint), // command completions
            Constraint::Length(1), // footer
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
    }

    if app.mode == InputMode::Command {
        render_command_hint(f, app, chunks[2]);
    }
    render_footer(f, app, chunks[3]);

    // Blocking dialogs sit above panel content and below help. The full-screen
    // detail view is a panel mode, not a dialog, and is rendered by its panel.
    if let Some(pending) = &app.confirm {
        components::render_dialog(f, f.area(), &confirm_to_dialog(pending));
    } else if let Some(view) = &app.describe {
        components::render_dialog(f, f.area(), &mounts::describe_to_dialog(view));
    } else if let Some(menu) = &app.menu {
        menu::render(f, f.area(), menu);
    }

    if app.help_visible {
        components::render_help_popup(f, f.area());
    }
}

/// One-line, borderless context header: identity + panel on the left, cluster
/// state in the middle, navigation hint on the right.
fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let left = vec![
        Span::styled(
            " ❖ curvine-tui",
            Style::default()
                .fg(th::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(th::BORDER)),
        Span::styled(
            app.panel.title(),
            Style::default()
                .fg(th::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    let (dot_tone, cluster_text) = match (&app.cluster.data, &app.cluster.error) {
        (Some(c), _) => {
            let tone = if c.workers.is_empty() { Tone::Warn } else { Tone::Ok };
            let master = if c.active_master.is_empty() {
                "connected".to_string()
            } else {
                c.active_master.clone()
            };
            (tone, master)
        }
        (None, Some(_)) => (Tone::Error, "disconnected".to_string()),
        (None, None) if app.cluster.loading => (Tone::Accent, "connecting…".to_string()),
        (None, None) => (Tone::Neutral, "disconnected".to_string()),
    };

    let workers = app
        .cluster
        .data
        .as_ref()
        .map(|c| c.workers.len())
        .unwrap_or(0);
    let worker_word = if workers == 1 { "worker" } else { "workers" };
    let dot_span = if app.cluster.data.is_none() && app.cluster.error.is_none() {
        Span::styled(components::DOT_HOLLOW, Style::default().fg(Tone::Neutral.color()))
    } else {
        components::dot(dot_tone)
    };
    let full_middle = vec![
        dot_span.clone(),
        Span::styled(format!(" {cluster_text}"), Style::default().fg(th::MUTED)),
        Span::styled("   ·   ", Style::default().fg(th::BORDER)),
        Span::styled(
            format!("{workers} {worker_word}"),
            Style::default().fg(th::MUTED),
        ),
        Span::styled("   ·   ", Style::default().fg(th::BORDER)),
        Span::styled(
            format!("↻ {}s", app.refresh_countdown_secs()),
            Style::default().fg(th::MUTED),
        ),
    ];
    // Compact form when the full cluster line cannot fit next to the identity.
    let compact_middle = vec![
        dot_span,
        Span::styled(format!(" {cluster_text}"), Style::default().fg(th::MUTED)),
    ];

    let right = vec![
        Span::styled("1-8", Style::default().fg(th::ACCENT_DIM)),
        Span::styled(" switch ", Style::default().fg(th::MUTED)),
        Span::styled("·", Style::default().fg(th::BORDER)),
        Span::styled(" :", Style::default().fg(th::ACCENT_DIM)),
        Span::styled(" command ", Style::default().fg(th::MUTED)),
    ];

    let width_of = |spans: &[Span]| -> usize { spans.iter().map(|s| s.content.chars().count()).sum() };
    let left_w = width_of(&left);
    let full_w = width_of(&full_middle);
    let compact_w = width_of(&compact_middle);
    let right_w = width_of(&right);
    let inner = area.width as usize;

    let mut line: Vec<Span> = left;
    if inner >= left_w + 2 + full_w + 2 + right_w {
        line.push(Span::raw(" ".repeat(
            inner - left_w - full_w - right_w,
        )));
        line.extend(full_middle);
        line.extend(right);
    } else if inner >= left_w + 2 + compact_w {
        line.push(Span::raw("  "));
        line.extend(compact_middle);
    }

    f.render_widget(Paragraph::new(Line::from(line)), area);
}

/// Command-line suggestions: the input itself lives in the footer, so this row
/// stays dedicated to prefix matches (or the explicit no-match marker).
fn render_command_hint(f: &mut Frame, app: &App, area: Rect) {
    let matches = crate::app::matching_commands(&app.command);
    let mut spans = vec![Span::raw(" ")];
    if app.command.trim().is_empty() {
        spans.push(Span::styled(
            "commands  ",
            Style::default().fg(th::MUTED),
        ));
        spans.push(Span::styled(
            crate::app::COMMANDS.join("  "),
            Style::default().fg(th::ACCENT_DIM),
        ));
    } else if matches.is_empty() {
        spans.push(Span::styled(
            "✗ no matching command",
            Style::default().fg(th::ERROR),
        ));
    } else {
        spans.push(Span::styled(
            "match  ",
            Style::default().fg(th::MUTED),
        ));
        for (i, cmd) in matches.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(
                (*cmd).to_string(),
                Style::default().fg(th::ACCENT),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The footer merges the old status row and key-hint row: message on the left,
/// contextual keys on the right. It has no border, matching the header.
fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let (left_text, left_style) = if app.mode == InputMode::Command {
        (
            format!(" :{}▏", app.command),
            Style::default().fg(th::TEXT),
        )
    } else if app.mode == InputMode::Filter {
        (
            format!(" /{}▏", app.files.filter),
            Style::default().fg(th::TEXT),
        )
    } else {
        status_text(app)
    };
    let hints = footer_hints(app);

    let width = area.width as usize;
    let right_w = hints.chars().count();
    let show_right = right_w < width;
    let mut line = Vec::new();

    if show_right {
        let available = width.saturating_sub(right_w + 1);
        let text: String = left_text.chars().take(available).collect();
        line.push(Span::styled(text, left_style));
        let used = line.iter().map(|s: &Span| s.content.chars().count()).sum::<usize>();
        line.push(Span::raw(" ".repeat(width.saturating_sub(used + right_w))));
        line.push(Span::styled(hints, Style::default().fg(th::MUTED)));
    } else {
        let text: String = left_text.chars().take(width).collect();
        line.push(Span::styled(text, left_style));
    }

    f.render_widget(Paragraph::new(Line::from(line)), area);
}

/// Status message with the existing fade, or the configured masters when idle.
fn status_text(app: &App) -> (String, Style) {
    let alpha = app.anim.status_alpha();
    let faded = |c: ratatui::style::Color| crate::anim::lerp_color(th::MUTED, c, alpha);
    if app.status_is_error {
        (format!(" {}", app.status), Style::default().fg(faded(th::ERROR)))
    } else if app.status.is_empty() {
        let masters = if app.cfg.master_addrs.is_empty() {
            " no masters configured — pass --master-addrs or --conf".to_string()
        } else {
            format!(" masters: {}", app.cfg.master_addrs.join(", "))
        };
        (masters, Style::default().fg(th::MUTED))
    } else {
        (format!(" {}", app.status), Style::default().fg(faded(th::OK)))
    }
}

/// Contextual key hints for the footer's right edge.
fn footer_hints(app: &App) -> String {
    if app.help_visible {
        return "? close help  q quit".to_string();
    }
    match app.mode {
        InputMode::Command => "tab complete  ↑↓ history  ⏎ run  esc cancel".to_string(),
        InputMode::Filter => "⏎ apply  esc clear".to_string(),
        InputMode::Mount => "tab next  ←→ change  space toggle  ⏎ mount  esc cancel".to_string(),
        InputMode::Transfer => "tab switch  ⏎ transfer  esc cancel".to_string(),
        InputMode::Confirm => "⏎ confirm  esc cancel".to_string(),
        InputMode::Describe => "esc close".to_string(),
        InputMode::Detail => "esc/q close  1-8 switch".to_string(),
        InputMode::Menu => "↑↓ move  ⏎ run  esc close".to_string(),
        InputMode::Normal => match app.panel {
            Panel::Files => {
                "space mark  a all  u unmark  x menu  ↑↓ move  ⏎ enter  d detail  / filter".to_string()
            }
            Panel::Dashboard => "↑↓ select  d detail  x menu  r refresh  ? help".to_string(),
            Panel::Nodes => "↑↓ select  d detail  x menu  r refresh  ? help".to_string(),
            Panel::Mounts => "↑↓ select  n new  u unmount  x menu  r refresh  ? help".to_string(),
            Panel::Jobs => "↑↓ select  c cancel  x menu  r refresh  ? help".to_string(),
            Panel::Transfers => {
                "↑↓ select  Del cancel  c clear  p put  g get  x menu".to_string()
            }
            Panel::Fsck => "↑↓ select  ⏎ detail  s scan  f target  x menu".to_string(),
            Panel::Bench => "b run  p profile  t threads  d duration  x menu".to_string(),
        },
    }
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
