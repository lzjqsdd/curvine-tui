//! Reusable presentational widgets.
//!
//! Everything here is stateless: callers hand in already-resolved data and the
//! widget decides how to draw it. Keeping them together avoids each panel
//! inventing its own bar/badge/KV conventions.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
    Frame,
};

use super::theme as th;

// ------------------------------------------------------------------- chrome

pub fn panel_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th::BORDER))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(th::ACCENT_DIM)
                .add_modifier(Modifier::BOLD),
        ))
}

/// Like `panel_block`, but the title carries a trailing badge.
pub fn panel_block_badged(title: &str, badge: Tag) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th::BORDER))
        .title(Line::from(vec![
            Span::styled(
                format!(" {title} "),
                Style::default()
                    .fg(th::ACCENT_DIM)
                    .add_modifier(Modifier::BOLD),
            ),
            badge.span(),
            Span::raw(" "),
        ]))
}

// ---------------------------------------------------------------------- tags

/// A small coloured badge: ` mount `, ` LIVE `, ` 82% `.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Ok,
    Warn,
    Error,
    Info,
}

impl Tone {
    pub fn color(self) -> Color {
        match self {
            Tone::Neutral => th::MUTED,
            Tone::Accent => th::ACCENT,
            Tone::Ok => th::OK,
            Tone::Warn => th::WARN,
            Tone::Error => th::ERROR,
            Tone::Info => th::DIR,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub text: String,
    pub tone: Tone,
    /// Render with a filled background instead of just coloured text.
    pub solid: bool,
}

impl Tag {
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
            solid: false,
        }
    }

    pub fn solid(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
            solid: true,
        }
    }

    /// Build a tag from an arbitrary status string (e.g. a Debug-formatted enum).
    pub fn from_status(s: &str) -> Self {
        let lower = s.to_ascii_lowercase();
        let tone = if ["live", "ok", "done", "complete", "success", "healthy"]
            .iter()
            .any(|k| lower.contains(k))
        {
            Tone::Ok
        } else if ["lost", "fail", "error", "dead", "unhealthy"]
            .iter()
            .any(|k| lower.contains(k))
        {
            Tone::Error
        } else if ["warn", "degrad", "under", "cancel", "stale", "pending"]
            .iter()
            .any(|k| lower.contains(k))
        {
            Tone::Warn
        } else if ["run", "active", "loading", "scan", "in_progress"]
            .iter()
            .any(|k| lower.contains(k))
        {
            Tone::Accent
        } else {
            Tone::Neutral
        };
        Self::new(s, tone)
    }

    pub fn span(&self) -> Span<'static> {
        let color = self.tone.color();
        let style = if self.solid {
            Style::default()
                .fg(Color::Rgb(16, 18, 22))
                .bg(color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        };
        Span::styled(format!(" {} ", self.text), style)
    }

    /// Same as `span` but without leading/trailing padding, for table cells.
    pub fn cell(&self) -> Span<'static> {
        let color = self.tone.color();
        let style = if self.solid {
            Style::default().fg(Color::Rgb(16, 18, 22)).bg(color)
        } else {
            Style::default().fg(color)
        };
        Span::styled(self.text.clone(), style)
    }

    #[allow(dead_code)]
    pub fn width(&self) -> usize {
        self.text.chars().count() + if self.solid { 0 } else { 2 }
    }
}

// ------------------------------------------------------------------ progress

/// Horizontal bar using eighth-block characters, so short bars still convey
/// sub-cell precision. Returns the bar and its display width.
pub fn bar(ratio: f64, width: usize) -> String {
    const PARTS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let ratio = ratio.clamp(0.0, 1.0);
    if width == 0 {
        return String::new();
    }
    let filled = ratio * width as f64;
    let full = filled.floor() as usize;
    let mut out = String::with_capacity(width * 3);
    for _ in 0..full.min(width) {
        out.push('█');
    }
    let mut remaining = width - full.min(width);
    if remaining > 0 {
        let frac = (filled - full as f64) * 8.0;
        let idx = (frac.round() as usize).min(7);
        if idx > 0 {
            out.push(PARTS[idx]);
            remaining -= 1;
        }
    }
    for _ in 0..remaining {
        out.push('░');
    }
    out
}

/// A bar plus a right-aligned percentage, sized to `width`.
pub fn bar_with_pct(ratio: f64, width: usize) -> String {
    let width = width.max(6);
    let pct = format!("{:>4.0}% ", ratio.clamp(0.0, 1.0) * 100.0);
    let bar_width = width.saturating_sub(pct.chars().count()).max(1);
    format!("{}{}", bar(ratio, bar_width), pct)
}

/// A labelled usage gauge, used for capacity and health.
pub fn render_usage(
    f: &mut Frame,
    area: Rect,
    label: &str,
    ratio: f64,
    text: String,
    tone: Tone,
) {
    let g = Gauge::default()
        .block(panel_block(label))
        .gauge_style(
            Style::default()
                .fg(tone.color())
                .bg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        )
        .ratio(ratio.clamp(0.0, 1.0))
        .label(Span::styled(text, Style::default().fg(th::TEXT)));
    f.render_widget(g, area);
}

// ------------------------------------------------------------------- key/value

/// One line of `  label    value` inside a bordered panel, with the label in a
/// muted colour so values carry the visual weight.
pub fn kv_line(label: &str, value: impl Into<String>, tone: Tone, label_width: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label:<label_width$}"),
            Style::default().fg(th::MUTED),
        ),
        Span::styled(value.into(), Style::default().fg(tone.color())),
    ])
}

/// A kv line whose value is a tag.
#[allow(dead_code)]
pub fn kv_tagged(label: &str, tag: Tag, label_width: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label:<label_width$}"),
            Style::default().fg(th::MUTED),
        ),
        tag.span(),
    ])
}

/// Section heading inside a panel body.
pub fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {title}"),
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    ))
}

// --------------------------------------------------------------------- misc

pub fn spinner(frame: usize) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[frame % FRAMES.len()]
}

/// A tick-derived spinner that advances with wall time, so long operations
/// animate even when no events arrive.
pub fn live_spinner() -> &'static str {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as usize)
        .unwrap_or(0);
    spinner(ms / 80)
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
            ratatui::layout::Constraint::Percentage(percent_y.max(3)),
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
            ratatui::layout::Constraint::Percentage(percent_x.max(20)),
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

/// A single-line text input with a caret.
pub fn input_line(label: &str, value: &str, focused: bool, mask: bool) -> Line<'static> {
    let shown = if mask {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    let style = if focused {
        Style::default().fg(th::TEXT)
    } else {
        Style::default().fg(th::MUTED)
    };
    Line::from(vec![
        Span::styled(
            format!("  {label:<10}"),
            Style::default().fg(if focused {
                th::ACCENT
            } else {
                th::ACCENT_DIM
            }),
        ),
        Span::styled(shown, style),
        Span::styled(
            if focused { "▏" } else { "" },
            Style::default().fg(th::ACCENT),
        ),
    ])
}

/// A toggle row rendered as `[x] label` / `[ ] label`.
pub fn toggle_line(label: &str, on: bool, focused: bool) -> Line<'static> {
    let box_span = if on {
        Span::styled("[x] ", Style::default().fg(th::OK))
    } else {
        Span::styled("[ ] ", Style::default().fg(th::MUTED))
    };
    Line::from(vec![
        Span::raw("  "),
        box_span,
        Span::styled(
            label.to_string(),
            Style::default().fg(if focused { th::ACCENT } else { th::TEXT }),
        ),
    ])
}

/// A horizontally scrolled-choice row: `‹ value ›`.
pub fn choice_line(label: &str, value: &str, focused: bool) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label:<10}"),
            Style::default().fg(if focused {
                th::ACCENT
            } else {
                th::ACCENT_DIM
            }),
        ),
        Span::styled("‹ ", Style::default().fg(th::MUTED)),
        Span::styled(value.to_string(), Style::default().fg(th::TEXT)),
        Span::styled(" ›", Style::default().fg(th::MUTED)),
    ])
}

/// Centered popup frame with a title; returns the inner rect for body content.
pub fn popup(f: &mut Frame, area: Rect, title: &str) -> Rect {
    let rect = centered_rect(66, 56, area);
    f.render_widget(Clear, rect);
    let block = panel_block(title);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    inner
}

// ------------------------------------------------------------------- help

pub fn render_help_popup(f: &mut Frame, area: Rect) {
    let rect = centered_rect(64, 72, area);
    f.render_widget(Clear, rect);

    let rows: Vec<(&str, &str)> = vec![
        ("tab / shift-tab", "switch panel"),
        ("1 … 9", "jump to panel"),
        ("j / k  ↑ ↓", "move selection"),
        ("enter / l", "enter directory / act"),
        ("backspace / h", "parent directory"),
        ("/", "filter current listing"),
        (".", "toggle hidden files"),
        ("n", "new mount (Mounts panel)"),
        ("u", "unmount selection"),
        ("m", "create directory"),
        ("d / D", "delete / delete recursive"),
        ("p / g", "upload / download"),
        ("x", "cancel selected transfer / scan"),
        ("c", "clear finished transfers"),
        ("f", "fsck: target browsed directory"),
        ("r", "refresh / re-run panel"),
        (":", "command line (ls, cd, mkdir, rm, mv, put, get, mount…)"),
        ("?", "toggle this help"),
        ("q / ctrl-c", "quit"),
    ];

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                " curvine-tui ",
                Style::default()
                    .fg(th::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Tag::solid("9 panels", Tone::Accent).span(),
        ]),
        Line::from(""),
    ];
    for (k, d) in rows {
        lines.push(Line::from(vec![
            Span::styled(format!("  {k:<18}"), Style::default().fg(th::ACCENT_DIM)),
            Span::styled(d.to_string(), Style::default().fg(th::TEXT)),
        ]));
    }

    let p = Paragraph::new(lines)
        .block(panel_block("Help"))
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    f.render_widget(p, rect);
}
