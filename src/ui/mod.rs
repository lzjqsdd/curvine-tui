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
pub mod detail;
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
            Constraint::Length(1), // header: panel menu + cluster badge
            Constraint::Min(3),    // main area
            Constraint::Length(command_hint), // command completions
            Constraint::Length(1), // footer: status badge + hints
        ])
        .split(f.area());

    render_header(f, app, chunks[0]);

    if let (InputMode::Detail, Some(view)) = (app.mode, &app.detail) {
        detail::render(f, view, chunks[1]);
    } else {
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
    }

    if app.mode == InputMode::Command {
        render_command_hint(f, app, chunks[2]);
    }
    render_footer(f, app, chunks[3]);

    // Error toast: bottom-centred, above the footer, floating over panel
    // content. Drawn before dialogs so a blocking decision stays on top.
    if let Some(toast) = &app.toast {
        let height = 3.min(chunks[3].y);
        if height > 0 {
            components::render_toast(
                f,
                Rect {
                    x: f.area().x,
                    y: chunks[3].y.saturating_sub(height),
                    width: f.area().width,
                    height,
                },
                &toast.text,
                toast.count,
                app.anim.toast_alpha(),
            );
        }
    }

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

/// How much of each tab the header draws. The menu degrades one step at a
/// time so the panel number is the last thing to go, never the cluster pill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabDetail {
    Label,
    Icon,
    Number,
}

impl TabDetail {
    /// Richest first: short label, then icon, then the bare number.
    const TIERS: [TabDetail; 3] = [TabDetail::Label, TabDetail::Icon, TabDetail::Number];

    fn text(self, panel: Panel, number: usize) -> String {
        match self {
            TabDetail::Label => format!(" {number} {} {} ", panel.icon(), panel.short_title()),
            TabDetail::Icon => format!(" {number} {} ", panel.icon()),
            TabDetail::Number => format!(" {number} "),
        }
    }

    fn width(self) -> usize {
        Panel::ALL
            .iter()
            .enumerate()
            .map(|(i, p)| self.text(*p, i + 1).chars().count())
            .sum()
    }

    /// Widest tier that fits in `width`. If even the numbers are too wide the
    /// caller clips them: losing a panel from the menu is worse than clipping.
    fn fit(width: u16) -> TabDetail {
        Self::TIERS
            .into_iter()
            .find(|tier| tier.width() <= width as usize)
            .unwrap_or(TabDetail::Number)
    }
}

/// Active tab: accent ink on a subtle fill. Inactive tabs are muted.
fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(th::ACCENT)
            .bg(th::TAB_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th::MUTED)
    }
}

/// Text and tone for the cluster pill in the header's right corner.
fn cluster_badge(app: &App) -> (String, Tone) {
    let (dot_glyph, cluster_text, tone) = match (&app.cluster.data, &app.cluster.error) {
        (Some(c), _) => {
            let tone = if c.workers.is_empty() {
                Tone::Warn
            } else {
                Tone::Ok
            };
            let master = if c.active_master.is_empty() {
                "connected".to_string()
            } else {
                c.active_master.clone()
            };
            (components::DOT, master, tone)
        }
        (None, Some(_)) => (components::DOT_HOLLOW, "disconnected".to_string(), Tone::Error),
        (None, None) if app.cluster.loading => (
            components::DOT_HOLLOW,
            "connecting…".to_string(),
            Tone::Accent,
        ),
        (None, None) => (
            components::DOT_HOLLOW,
            "disconnected".to_string(),
            Tone::Neutral,
        ),
    };
    let workers = app
        .cluster
        .data
        .as_ref()
        .map(|c| c.workers.len())
        .unwrap_or(0);
    let worker_word = if workers == 1 { "worker" } else { "workers" };
    (
        format!(
            "{dot_glyph} {cluster_text} · {workers} {worker_word} · ↻{}s",
            app.refresh_countdown_secs()
        ),
        tone,
    )
}

/// One-line, borderless menu header: a tab per panel on the left and the
/// cluster pill on the right.
///
/// The tab group degrades (`n icon short` -> `n icon` -> `n`) against the width
/// left over after the pill is placed, never the other way round: the pill
/// carries cluster health, so it always keeps its cells.
fn render_header(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let row = Rect {
        y: area.y + area.height.saturating_sub(1) / 2,
        height: 1,
        ..area
    };

    let (badge, tone) = cluster_badge(app);
    let badge_w = components::pill_width(&badge).min(area.width);
    let badge_x = area.right().saturating_sub(badge_w);

    // Keep one empty cell between the menu and the pill whenever there is room.
    let gap = u16::from(badge_x > area.x);
    let tabs_w = badge_x.saturating_sub(area.x).saturating_sub(gap);
    let detail = TabDetail::fit(tabs_w);
    let tabs: Vec<Span> = Panel::ALL
        .iter()
        .enumerate()
        .map(|(i, panel)| {
            Span::styled(
                detail.text(*panel, i + 1),
                tab_style(*panel == app.panel),
            )
        })
        .collect();

    f.render_widget(
        Paragraph::new(Line::from(tabs)),
        Rect {
            width: tabs_w,
            ..row
        },
    );
    f.render_widget(
        Paragraph::new(components::badge(badge, tone)),
        Rect {
            x: badge_x,
            width: badge_w,
            ..row
        },
    );
}

/// The footer merges the status row and the key hints: a success/info status is
/// an inline badge on the left (its tone carries the fade), the contextual
/// hints stay plain on the right so a long hint list never turns into a giant
/// box. Errors never reach this row — they become a toast.
fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let row = Rect {
        y: area.y + area.height.saturating_sub(1) / 2,
        height: 1,
        ..area
    };
    // Prefer the panel's hints, but never let a long list hide the one global
    // action the header cannot show once it has degraded: switching panels.
    let hints = {
        let full = footer_hints(app);
        if area.width as usize > full.chars().count() + 8 {
            full
        } else if app.mode == InputMode::Normal {
            GENERIC_HINTS.to_string()
        } else {
            full
        }
    };
    let hints_w = hints.chars().count() as u16;
    let hints_fit = area.width > hints_w + 8;
    if hints_fit {
        f.render_widget(
            Paragraph::new(Span::styled(hints, Style::default().fg(th::MUTED))),
            Rect {
                x: area.right() - hints_w,
                width: hints_w,
                ..row
            },
        );
    }

    if app.mode == InputMode::Command {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" :", Style::default().fg(th::ACCENT)),
                Span::raw(app.command.clone()),
                Span::styled("▏", Style::default().fg(th::ACCENT)),
            ])),
            Rect {
                width: if hints_fit {
                    area.right().saturating_sub(hints_w + 2)
                } else {
                    area.width
                },
                ..row
            },
        );
        return;
    }
    if app.mode == InputMode::Filter {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" /", Style::default().fg(th::ACCENT)),
                Span::raw(app.files.filter.clone()),
                Span::styled("▏", Style::default().fg(th::ACCENT)),
            ])),
            Rect {
                width: if hints_fit {
                    area.right().saturating_sub(hints_w + 2)
                } else {
                    area.width
                },
                ..row
            },
        );
        return;
    }

    let (text, color) = status_badge(app);
    let available = if hints_fit {
        area.right().saturating_sub(hints_w + 2)
    } else {
        area.width
    };
    // A long status must never push the hints off-screen: the badge is capped
    // at 45% of the band, and the message is clipped with an ellipsis.
    let cap = ((area.width as u32 * 45) / 100) as u16;
    let max_w = available.min(cap.max(16));
    let max_chars = max_w.saturating_sub(4) as usize;
    let text: String = if text.chars().count() > max_chars {
        let mut clipped: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        clipped.push('…');
        clipped
    } else {
        text
    };
    f.render_widget(
        Paragraph::new(components::badge_colored(text.clone(), color)),
        Rect {
            x: area.x,
            y: row.y,
            width: components::pill_width(&text).min(max_w),
            height: 1,
        },
    );
}

/// Status text and the colour that carries its fade. Only success/info
/// messages reach the footer; errors are rendered as a toast instead.
fn status_badge(app: &App) -> (String, ratatui::style::Color) {
    let alpha = app.anim.status_alpha();
    if app.status.is_empty() {
        let masters = if app.cfg.master_addrs.is_empty() {
            "no masters configured — pass --master-addrs or --conf".to_string()
        } else {
            format!("masters: {}", app.cfg.master_addrs.join(", "))
        };
        (masters, th::MUTED)
    } else {
        (
            app.status.clone(),
            crate::anim::lerp_color(th::MUTED, th::OK, alpha),
        )
    }
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
            "match: ",
            Style::default().fg(th::MUTED),
        ));
        for (i, cmd) in matches.iter().take(3).enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::default().fg(th::BORDER)));
            }
            spans.push(Span::styled(
                (*cmd).to_string(),
                Style::default().fg(th::ACCENT),
            ));
        }
        if matches.len() > 3 {
            spans.push(Span::styled(" …", Style::default().fg(th::BORDER)));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The shortest footer that still names the global escape hatches. It is the
/// fallback when a panel's own hints do not fit, so panel switching can never
/// disappear from the footer just because a panel has many keys.
const GENERIC_HINTS: &str = "tab/1-8 panels · : cmd · ? help · q quit";

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
        InputMode::Detail => "esc/q close  tab/1-8 panels".to_string(),
        InputMode::Menu => "↑↓ move  ⏎ run  esc close".to_string(),
        InputMode::Normal => {
            let panel_hints = match app.panel {
                Panel::Files => {
                    "space mark  a all  u unmark  x menu  ↑↓ move  ⏎ enter  d detail  / filter"
                }
                Panel::Dashboard => "↑↓ select  d detail  x menu  r refresh  ? help",
                Panel::Nodes => "↑↓ select  d detail  x menu  r refresh  ? help",
                Panel::Mounts => "↑↓ select  n new  u unmount  x menu  r refresh  ? help",
                Panel::Jobs => "↑↓ select  c cancel  x menu  r refresh  ? help",
                Panel::Transfers => "↑↓ select  Del cancel  c clear  p put  g get  x menu",
                Panel::Fsck => "↑↓ select  ⏎ detail  s scan  f target  x menu",
                Panel::Bench => "b run  p profile  t threads  d duration  x menu",
            };
            format!("{panel_hints}  ·  tab/1-8 panels")
        }
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

/// Owner column value: an empty owner renders as a dash, never as a gap.
pub fn owner_dash(owner: &str) -> String {
    let owner = owner.trim();
    if owner.is_empty() {
        "-".to_string()
    } else {
        owner.to_string()
    }
}

/// `owner:group` with dashes for missing halves, so a bare ` : ` (or a
/// dangling `:`) can never reach the screen.
pub fn owner_group(owner: &str, group: &str) -> String {
    let owner = owner.trim();
    let group = group.trim();
    if owner.is_empty() && group.is_empty() {
        return "-".to_string();
    }
    format!(
        "{}:{}",
        if owner.is_empty() { "-" } else { owner },
        if group.is_empty() { "-" } else { group }
    )
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
