//! Reusable presentational widgets.
//!
//! Everything here is stateless: callers hand in already-resolved data and the
//! widget decides how to draw it. Keeping them together avoids each panel
//! inventing its own bar/badge/KV conventions.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
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

/// Inline label: coloured bold text with one cell of breathing room.
///
/// There is deliberately no filled variant. A solid background can only be
/// rectangular, and this UI draws every badge as a rounded pill instead; see
/// [`pill`]. Table cells use [`Tag::cell`], which never carries a background.
#[derive(Debug, Clone)]
pub struct Tag {
    pub text: String,
    pub tone: Tone,
}

impl Tag {
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }

    pub fn span(&self) -> Span<'static> {
        Span::styled(
            format!(" {} ", self.text),
            Style::default()
                .fg(self.tone.color())
                .add_modifier(Modifier::BOLD),
        )
    }

    /// Same as `span` but without leading/trailing padding, for table cells.
    pub fn cell(&self) -> Span<'static> {
        Span::styled(self.text.clone(), Style::default().fg(self.tone.color()))
    }

    #[allow(dead_code)]
    pub fn width(&self) -> usize {
        self.text.chars().count() + 2
    }
}

// ---------------------------------------------------------------------- pills

/// Width a pill needs: two rounded borders plus one cell of padding each side.
pub fn pill_width(text: &str) -> u16 {
    text.chars().count() as u16 + 4
}

/// Total width of a row of pills, including the single-cell gaps.
pub fn pills_width(items: &[(String, Tone)]) -> u16 {
    let inner: u16 = items.iter().map(|(t, _)| pill_width(t)).sum();
    inner + items.len().saturating_sub(1) as u16
}

/// The styled content line a pill carries. This is the `Line`-returning
/// variant: [`pill`] wraps it in the rounded border, and callers that need to
/// compose a badge row by hand can reuse exactly the same content style.
#[allow(dead_code)] // documented Line-returning API for composing badge rows
pub fn pill_content(text: impl Into<String>, tone: Tone) -> Line<'static> {
    pill_content_colored(text, tone.color())
}

/// Colour-explicit variant of [`pill_content`], used by the status pill so its
/// border and text can fade smoothly instead of snapping between tones.
pub fn pill_content_colored(text: impl Into<String>, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {} ", text.into()),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

/// Draw a rounded badge ("pill") filling `area`.
///
/// A one-line rounded box needs three rows: top border, content, bottom border.
/// When `area` is shorter than that (pathological terminals), the same content
/// is drawn as plain coloured text so a badge can never degrade into the solid
/// rectangular block this style exists to remove.
pub fn pill(f: &mut Frame, area: Rect, text: impl Into<String>, tone: Tone) -> Rect {
    pill_colored(f, area, text, tone.color())
}

/// [`pill`] with an explicit colour, so animated borders (the status fade) can
/// interpolate instead of switching palette entries.
pub fn pill_colored(f: &mut Frame, area: Rect, text: impl Into<String>, color: Color) -> Rect {
    let text = text.into();
    let width = pill_width(&text).min(area.width);
    let rect = Rect {
        x: area.x,
        y: area.y,
        width,
        height: area.height.min(3),
    };
    if rect.width < 3 || rect.height == 0 {
        return rect;
    }
    if rect.height < 3 {
        f.render_widget(
            Paragraph::new(pill_content_colored(text, color)),
            Rect {
                height: 1,
                ..rect
            },
        );
        return rect;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));
    f.render_widget(
        Paragraph::new(pill_content_colored(text, color)).block(block),
        rect,
    );
    rect
}

/// Render pills left to right inside `area`, one cell apart. Returns the x
/// coordinate just past the last pill (or `area.right()` when clipped).
pub fn pills(f: &mut Frame, area: Rect, items: &[(String, Tone)]) -> u16 {
    let mut x = area.x;
    for (text, tone) in items {
        if x >= area.right() {
            break;
        }
        let slot = Rect {
            x,
            y: area.y,
            width: area.right().saturating_sub(x),
            height: area.height,
        };
        let used = pill(f, slot, text.clone(), *tone);
        x = used.right().saturating_add(1);
    }
    x
}

/// Right-align a row of pills within `area`.
pub fn pills_end(f: &mut Frame, area: Rect, items: &[(String, Tone)]) -> u16 {
    let total = pills_width(items);
    let start = area.right().saturating_sub(total).max(area.x);
    pills(
        f,
        Rect {
            x: start,
            width: area.right().saturating_sub(start),
            ..area
        },
        items,
    )
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
    render_bar_block(f, area, label, ratio, text, tone);
}

/// A bordered bar with a centred label drawn over it.
///
/// Deliberately not `Gauge`: ratatui inverts a gauge label into a filled block,
/// and a filled block is exactly the solid rectangle this design removes. The
/// bar is drawn with block characters, so the label sits on plain background.
pub fn render_bar_block(
    f: &mut Frame,
    area: Rect,
    title: &str,
    ratio: f64,
    text: String,
    tone: Tone,
) {
    let block = panel_block(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let mid = Rect {
        y: inner.y + inner.height / 2,
        height: 1,
        ..inner
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            bar(ratio.clamp(0.0, 1.0), inner.width as usize),
            Style::default().fg(tone.color()),
        )),
        mid,
    );
    // Pad so the label never runs straight into the bar glyphs.
    let padded = format!(" {text} ");
    let text: String = padded.chars().take(inner.width as usize).collect();
    f.render_widget(
        Paragraph::new(Span::styled(text, Style::default().fg(th::TEXT)))
            .alignment(Alignment::Center),
        mid,
    );
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
    let block = panel_block("Help");
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    // Title and the panel-count pill share one three-row band; the pill is the
    // badge, the product name stays plain text beside it.
    let band_h = inner.height.min(3);
    let band = Rect {
        height: band_h,
        ..inner
    };
    let pill_rect = pill(
        f,
        Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(1),
            ..band
        },
        format!("{} panels", crate::app::Panel::ALL.len()),
        Tone::Accent,
    );
    if band_h >= 3 {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "❖ curvine-tui",
                    Style::default()
                        .fg(th::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  contextual actions", Style::default().fg(th::MUTED)),
            ])),
            Rect {
                x: pill_rect.right().saturating_add(1),
                y: band.y + 1,
                width: inner.right().saturating_sub(pill_rect.right() + 1),
                height: 1,
            },
        );
    }

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

    let mut lines: Vec<Line> = vec![section("status dots")];

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
            Span::styled(format!("  {meaning}"), Style::default().fg(th::MUTED)),
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

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Left),
        Rect {
            y: inner.y + band_h,
            height: inner.height.saturating_sub(band_h),
            ..inner
        },
    );
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
            .max(headline_w + 8)
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
        let mut body = 1; // leading blank
        body += self.rows.len() as u16;
        for (_, lines) in &self.sections {
            body += 2; // blank + section title
            body += lines.len() as u16;
        }
        body += self.notes.len() as u16 * 2; // blank + note
        let mut h = 2; // top + bottom border
        if self.headline.is_some() {
            h += 3; // rounded pill band
        }
        h += body;
        h += 1; // gap above the footer
        h += 3; // footer pills
        h.max(11)
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

    let accent = if dlg.destructive { th::ERROR } else { th::ACCENT };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            format!(" {} ", dlg.title),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let split = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(if dlg.headline.is_some() { 3 } else { 0 }),
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(3),
        ])
        .split(inner);

    // Headline badge: a rounded pill, never a filled rectangle.
    if let Some((text, tone)) = &dlg.headline {
        pill(
            f,
            Rect {
                x: inner.x + 1,
                width: inner.width.saturating_sub(2),
                ..split[0]
            },
            text.clone(),
            *tone,
        );
    }

    let mut lines: Vec<Line> = vec![Line::from("")];
    let label_w = dlg
        .rows
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0);
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

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Left),
        split[1],
    );

    // Footer keycaps are pills too; the labels between them stay plain so the
    // row reads as one sentence.
    let footer = split[3];
    let mid = Rect {
        y: footer.y + footer.height.saturating_sub(1) / 2,
        height: 1,
        ..footer
    };
    let mut x = inner.x + 2;
    let keycaps: &[(&str, Tone, &str)] = if dlg.destructive {
        &[("enter", Tone::Error, " confirm "), ("esc", Tone::Neutral, " cancel")]
    } else {
        &[("esc", Tone::Neutral, " close")]
    };
    for (key, tone, label) in keycaps {
        let slot = Rect {
            x,
            y: footer.y,
            width: footer.right().saturating_sub(x),
            height: footer.height,
        };
        let used = pill(f, slot, *key, *tone);
        x = used.right().saturating_add(1);
        f.render_widget(
            Paragraph::new(Span::styled(*label, Style::default().fg(th::MUTED))),
            Rect {
                x,
                width: mid.right().saturating_sub(x),
                ..mid
            },
        );
        x += label.chars().count() as u16;
    }

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
