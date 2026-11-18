//! Reusable presentational widgets.
//!
//! Everything here is stateless: callers hand in already-resolved data and the
//! widget decides how to draw it. Keeping them together avoids each panel
//! inventing its own bar/badge/KV conventions.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph, Wrap},
    Frame,
};

use super::theme as th;

// ------------------------------------------------------------------- chrome

pub fn panel_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
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
        .border_type(BorderType::Rounded)
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

/// Map an arbitrary status string onto a tone. `Tag` and the status dots share
/// this so a state can never be green in one panel and grey in another.
pub fn tone_for_status(s: &str) -> Tone {
    let lower = s.to_ascii_lowercase();
    if ["live", "ok", "done", "complete", "success", "healthy", "available"]
        .iter()
        .any(|k| lower.contains(k))
    {
        Tone::Ok
    } else if ["lost", "fail", "error", "dead", "unhealthy", "missing", "empty"]
        .iter()
        .any(|k| lower.contains(k))
    {
        Tone::Error
    } else if ["warn", "degrad", "under", "cancel", "stale", "pending", "partial"]
        .iter()
        .any(|k| lower.contains(k))
    {
        Tone::Warn
    } else if ["run", "active", "loading", "scan", "in_progress", "queued"]
        .iter()
        .any(|k| lower.contains(k))
    {
        Tone::Accent
    } else {
        Tone::Neutral
    }
}

// --------------------------------------------------------------- status dots

/// Filled circle — the health/status indicator used across every panel.
pub const DOT: &str = "●";
/// Hollow circle — a state that is expected but not present.
pub const DOT_HOLLOW: &str = "○";

/// A single coloured dot.
pub fn dot(tone: Tone) -> Span<'static> {
    Span::styled(DOT, Style::default().fg(tone.color()))
}

/// `● label`. Quieter than a boxed tag, which is why health states use this
/// shape and only categorical values keep the box.
pub fn dot_label(tone: Tone, label: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        dot(tone),
        Span::raw(" "),
        Span::styled(label.into(), Style::default().fg(tone.color())),
    ])
}

/// Dot plus label straight from a status string.
pub fn dot_status(label: &str) -> Line<'static> {
    dot_label(tone_for_status(label), label)
}

/// A dot where only the count matters, e.g. replica health.
pub fn dot_count(tone: Tone, count: impl std::fmt::Display) -> Line<'static> {
    Line::from(vec![
        dot(tone),
        Span::raw(" "),
        Span::styled(count.to_string(), Style::default().fg(th::TEXT)),
    ])
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

// ------------------------------------------------------------------- help

pub fn render_help_popup(f: &mut Frame, area: Rect) {
    let rect = centered_rect(66, 88, area);
    f.render_widget(Clear, rect);

    let rows: Vec<(&str, &str)> = vec![
        ("x", "open the contextual action menu"),
        ("j / k  ↑ ↓", "move selection"),
        ("tab / shift-tab", "switch panel"),
        ("1 … 8", "jump to panel"),
        ("enter / l", "enter directory / act"),
        ("backspace / h", "parent directory"),
        ("d", "full-screen detail (files, workers)"),
        ("space", "mark entry · a marks all · u clears marks"),
        ("/ / .", "filter listing / toggle hidden files"),
        ("n / u", "new mount / unmount (Mounts)"),
        ("m", "create directory"),
        ("Del / D", "delete / delete recursively (asks first)"),
        ("F", "free cached blocks"),
        ("p / g", "upload / download"),
        ("Del", "cancel selected transfer"),
        ("c", "clear finished transfers · cancel job"),
        ("s / f", "fsck scan / target browsed directory"),
        ("b p t d", "bench run / profile / threads / duration"),
        ("r", "refresh panel"),
        (":", "command line · tab completes · ↑↓ history"),
        ("esc", "close detail/menu · clear marks"),
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
            Tag::solid(
                format!("{} panels", crate::app::Panel::ALL.len()),
                Tone::Accent,
            )
            .span(),
        ]),
        Line::from(""),
        section("status dots"),
    ];

    // Legend: the dot colours are the health vocabulary, so spell them out once.
    let legend = [
        (Tone::Ok, "healthy / live / complete"),
        (Tone::Accent, "in progress"),
        (Tone::Warn, "degraded / pending"),
        (Tone::Error, "failed / lost / under-replicated"),
        (Tone::Neutral, "idle / unknown / not applicable"),
    ];
    for (tone, meaning) in legend {
        lines.push(Line::from(vec![
            Span::raw("  "),
            dot(tone),
            Span::styled(
                format!("  {meaning}"),
                Style::default().fg(th::MUTED),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(section("keys"));

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


// ------------------------------------------------------------------ dialogs

/// A generic blocking dialog. Used for both confirmations and describe views so
/// the app only needs one popup mechanism.
#[derive(Debug, Clone)]
pub struct Dialog {
    pub title: String,
    /// Bold headline line, drawn first.
    pub headline: Option<(String, Tone)>,
    /// `label: value` rows rendered as a two-column grid.
    pub rows: Vec<(String, String)>,
    /// Titled blocks of pre-formatted lines, for structured detail like a
    /// per-worker block distribution.
    pub sections: Vec<(String, Vec<Line<'static>>)>,
    /// Secondary text under the grid (hints, errors, footnotes).
    pub notes: Vec<(String, Tone)>,
    /// Shows the red confirm footer instead of a dismiss footer.
    pub destructive: bool,
}

impl Dialog {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            headline: None,
            rows: Vec::new(),
            sections: Vec::new(),
            notes: Vec::new(),
            destructive: false,
        }
    }

    /// Append a titled block of pre-rendered lines.
    pub fn section(mut self, title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        self.sections.push((title.into(), lines));
        self
    }

    pub fn headline(mut self, text: impl Into<String>, tone: Tone) -> Self {
        self.headline = Some((text.into(), tone));
        self
    }

    pub fn row(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.rows.push((label.into(), value.into()));
        self
    }

    pub fn note(mut self, text: impl Into<String>, tone: Tone) -> Self {
        self.notes.push((text.into(), tone));
        self
    }

    pub fn destructive(mut self, yes: bool) -> Self {
        self.destructive = yes;
        self
    }

    /// Width the dialog needs, so callers can size the popup to content.
    pub fn preferred_width(&self) -> u16 {
        let label_w = self.rows.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        let value_w = self.rows.iter().map(|(_, v)| v.chars().count()).max().unwrap_or(0);
        let headline_w = self
            .headline
            .as_ref()
            .map(|(h, _)| h.chars().count())
            .unwrap_or(0);
        let note_w = self.notes.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(0);
        let section_w = self
            .sections
            .iter()
            .flat_map(|(_, lines)| lines.iter())
            .map(|l| l.width())
            .max()
            .unwrap_or(0);
        let content = (label_w + value_w + 6)
            .max(headline_w + 4)
            .max(note_w + 4)
            .max(section_w + 4)
            .max(self.title.chars().count() + 8)
            .max(40);
        (content as u16).min(110)
    }

    /// Exact height the body needs. Counted line by line, because an off-by-one
    /// here silently clips the confirm footer — which is the one line the user
    /// has to read before a destructive action.
    pub fn preferred_height(&self) -> u16 {
        let mut h = 2; // top + bottom border
        h += 1; // leading blank
        if self.headline.is_some() {
            h += 2; // headline + trailing blank
        }
        h += self.rows.len() as u16;
        for (_, lines) in &self.sections {
            h += 2; // blank + section title
            h += lines.len() as u16;
        }
        h += self.notes.len() as u16 * 2; // blank + note
        h += 2; // blank + footer
        h.max(8)
    }
}

/// Render a dialog centred in `area`.
///
/// Dialogs appear at full size in one frame. A modal sits on the critical path
/// to a decision, so a box that is still growing — with its footer clipped —
/// reads as lag rather than as motion.
pub fn render_dialog(f: &mut Frame, area: Rect, dlg: &Dialog) -> Rect {
    let width = dlg.preferred_width().min(area.width.saturating_sub(2)).max(20);
    let height = dlg
        .preferred_height()
        .min(area.height.saturating_sub(2))
        .max(6);
    let rect = centre(width, height, area);
    f.render_widget(Clear, rect);

    let mut lines: Vec<Line> = vec![Line::from("")];

    if let Some((text, tone)) = &dlg.headline {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Tag::solid(text.clone(), *tone).span(),
        ]));
        lines.push(Line::from(""));
    }

    let label_w = dlg.rows.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
    for (label, value) in &dlg.rows {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {label:<label_w$}  "),
                Style::default().fg(th::MUTED),
            ),
            Span::styled(value.clone(), Style::default().fg(th::TEXT)),
        ]));
    }

    for (title, body) in &dlg.sections {
        lines.push(Line::from(""));
        lines.push(section(title));
        lines.extend(body.iter().cloned());
    }

    for (note, tone) in &dlg.notes {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(note.clone(), Style::default().fg(tone.color())),
        ]));
    }

    lines.push(Line::from(""));
    if dlg.destructive {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Tag::solid("enter", Tone::Error).span(),
            Span::styled(" confirm   ", Style::default().fg(th::MUTED)),
            Tag::new("esc", Tone::Neutral).span(),
            Span::styled(" cancel", Style::default().fg(th::MUTED)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Tag::new("esc", Tone::Neutral).span(),
            Span::styled(" close", Style::default().fg(th::MUTED)),
        ]));
    }

    let accent = if dlg.destructive { th::ERROR } else { th::ACCENT };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            format!(" {} ", dlg.title),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));

    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Left),
        rect,
    );
    rect
}

/// Exact-size rect centred in `area`, clamped to the available space.
pub fn centre(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}
