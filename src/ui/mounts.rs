//! Mount table plus the mount/unmount forms.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use super::{
    components::{self, choice_line, input_line, section, toggle_line, Tag, Tone},
    human_bytes, theme as th,
};
use crate::app::{App, MountForm};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(area);

    render_table(f, app, rows[0]);
    render_selected(f, app, rows[1]);

    if let Some(form) = &app.mount_form {
        render_form(f, form, area);
    }}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 90;

    let mut header_cells = vec![
        Cell::from("cv path"),
        Cell::from("ufs path"),
        Cell::from("mode"),
        Cell::from("storage"),
    ];
    if wide {
        header_cells.push(Cell::from("ttl"));
        header_cells.push(Cell::from("provider"));
    }
    let header = Row::new(header_cells).style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .mounts
        .data
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|m| {
                    let mode_tone = if m.write_type.to_ascii_lowercase().contains("fs") {
                        Tone::Info
                    } else {
                        Tone::Accent
                    };
                    let mut cells = vec![
                        Cell::from(m.cv_path.clone()).style(Style::default().fg(th::DIR)),
                        Cell::from(m.ufs_path.clone()),
                        Cell::from(Tag::new(m.write_type.clone(), mode_tone).cell()),
                        Cell::from(storage_tag(&m.storage_type).cell()),
                    ];
                    if wide {
                        cells.push(Cell::from(if m.ttl_ms > 0 {
                            format!("{}s", m.ttl_ms / 1000)
                        } else {
                            "∞".to_string()
                        }));
                        cells.push(Cell::from(if m.provider.is_empty() {
                            "—".to_string()
                        } else {
                            m.provider.clone()
                        }));
                    }
                    Row::new(cells)
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.mounts.data, app.mounts.loading) {
        (Some(l), _) => format!("mounts — {} entries", l.len()),
        (None, true) => format!("mounts — {} loading", components::live_spinner()),
        (None, false) => format!(
            "mounts — {}",
            app.mounts.error.as_deref().unwrap_or("no data")
        ),
    };

    let mut widths = vec![
        Constraint::Percentage(26),
        Constraint::Percentage(34),
        Constraint::Length(11),
        Constraint::Length(9),
    ];
    if wide {
        widths.push(Constraint::Length(8));
        widths.push(Constraint::Min(10));
    }

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.mount_selected.min(rows.len() - 1)));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(components::panel_block(&title))
        .row_highlight_style(
            Style::default()
                .bg(th::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ")
        .column_spacing(1);

    f.render_stateful_widget(table, area, &mut state);
}

fn storage_tag(s: &str) -> Tag {
    let lower = s.to_ascii_lowercase();
    let tone = if lower.contains("ssd") {
        Tone::Accent
    } else if lower.contains("hdd") {
        Tone::Info
    } else if lower.contains("mem") {
        Tone::Warn
    } else if lower.contains("ufs") {
        Tone::Ok
    } else {
        Tone::Neutral
    };
    Tag::new(if s.is_empty() { "—".to_string() } else { s.to_string() }, tone)
}

fn render_selected(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let Some(m) = app.mounts.data.as_ref().and_then(|l| l.get(app.mount_selected)) else {
        f.render_widget(
            Paragraph::new(Span::styled(
                " n new mount   u unmount   r refresh — no mount selected",
                Style::default().fg(th::MUTED),
            )),
            Rect {
                y: area.y + area.height.saturating_sub(1) / 2,
                height: 1,
                ..area
            },
        );
        return;
    };

    // Badges for the mount identity, then the UFS side as plain text.
    let mut items: Vec<(String, Tone)> = vec![
        (m.cv_path.clone(), Tone::Info),
        (format!("id {}", m.mount_id), Tone::Neutral),
        (
            format!(
                "ttl {}",
                if m.ttl_ms > 0 {
                    format!("{}s", m.ttl_ms / 1000)
                } else {
                    "∞".to_string()
                }
            ),
            Tone::Neutral,
        ),
    ];
    if !m.storage_type.is_empty() {
        items.push((m.storage_type.clone(), Tone::Accent));
    }
    let used = components::badges(f, area, &items);
    let row = Rect {
        y: area.y + area.height.saturating_sub(1) / 2,
        height: 1,
        x: used,
        width: area.right().saturating_sub(used),
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("← ", Style::default().fg(th::BORDER)),
            Span::styled(m.ufs_path.clone(), Style::default().fg(th::TEXT)),
            Span::raw("  "),
            components::dot(Tone::Ok),
            Span::styled(" mounted", Style::default().fg(th::MUTED)),
        ])),
        row,
    );
}

fn render_form(f: &mut Frame, form: &MountForm, area: Rect) {
    let rect = components::centered_rect(66, 56, area);
    f.render_widget(ratatui::widgets::Clear, rect);
    f.render_widget(components::panel_block("new mount"), rect);
    let inner = components::panel_block("new mount").inner(rect);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(2)])
        .split(inner);

    let mut lines = vec![
        Line::from(""),
        section("paths"),
        input_line("ufs", &form.ufs_path, form.focus == 0, false),
        input_line("curvine", &form.cv_path, form.focus == 1, false),
        Line::from(""),
        section("options"),
        choice_line("write mode", form.write_type.label(), form.focus == 2),
        choice_line("storage", form.storage_type.label(), form.focus == 3),
        input_line("ttl", &form.ttl, form.focus == 4, false),
        toggle_line("auto cache", form.auto_cache, form.focus == 5),
        toggle_line("read verify", form.read_verify_ufs, form.focus == 6),
    ];

    if let Some(err) = &form.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  error: {err}"),
            Style::default().fg(th::ERROR),
        )));
    }

    f.render_widget(
        Paragraph::new(lines)
            .block(components::panel_block(""))
            .wrap(Wrap { trim: false }),
        inner,
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" tab ", Style::default().fg(th::ACCENT_DIM)),
            Span::styled("next field  ", Style::default().fg(th::MUTED)),
            Span::styled("← → ", Style::default().fg(th::ACCENT_DIM)),
            Span::styled("change  ", Style::default().fg(th::MUTED)),
            Span::styled("space ", Style::default().fg(th::ACCENT_DIM)),
            Span::styled("toggle  ", Style::default().fg(th::MUTED)),
            Span::styled("enter ", Style::default().fg(th::ACCENT_DIM)),
            Span::styled("mount  ", Style::default().fg(th::MUTED)),
            Span::styled("esc ", Style::default().fg(th::ACCENT_DIM)),
            Span::styled("cancel", Style::default().fg(th::MUTED)),
        ])),
        rows[1],
    );
}

/// Convert a describe view into the generic dialog shape.
///
/// Blocks are shown twice on purpose: once aggregated per worker (where does
/// the data actually live?) and once flat by offset (which block is which?).
/// A single flat list makes the distribution unreadable past a handful of
/// blocks, and a single aggregate hides per-block replica damage.
pub fn describe_to_dialog(view: &crate::app::DescribeView) -> components::Dialog {
    use crate::ui::{human_bytes, theme};

    let mut dlg = components::Dialog::new(view.title.clone());
    if let Some((tag, subject)) = &view.headline {
        dlg = dlg.headline(tag.clone(), Tone::Info);
        dlg.notes.push((subject.clone(), Tone::Accent));
    }
    for (k, v) in &view.rows {
        dlg = dlg.row(k.clone(), v.clone());
    }

    if view.loading {
        return dlg;
    }

    // --- distribution by worker
    if !view.worker_blocks.is_empty() {
        let total_bytes: i64 = view.worker_blocks.iter().map(|w| w.bytes).sum();
        let mut lines: Vec<Line> = Vec::new();
        for w in &view.worker_blocks {
            let tone = if w.available == w.total {
                Tone::Ok
            } else if w.available == 0 {
                Tone::Error
            } else {
                Tone::Warn
            };
            let share = if total_bytes > 0 {
                w.bytes as f64 / total_bytes as f64
            } else {
                0.0
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                components::dot(tone),
                Span::raw(" "),
                Span::styled(
                    format!("{:<22}", w.worker),
                    Style::default().fg(theme::TEXT),
                ),
                Span::styled(
                    format!("{:>3} blocks  ", w.block_ids.len()),
                    Style::default().fg(theme::MUTED),
                ),
                Span::styled(
                    format!("{:>9}  ", human_bytes(w.bytes)),
                    Style::default().fg(theme::TEXT),
                ),
                Span::styled(
                    components::bar(share, 8),
                    Style::default().fg(theme::usage_tone(1.0 - share).color()),
                ),
                Span::styled(
                    format!(" {:>3.0}%", share * 100.0),
                    Style::default().fg(theme::MUTED),
                ),
            ]));
        }
        dlg = dlg.section("distribution by worker", lines);
    }

    // --- flat block list, aggregated per block id
    if !view.blocks.is_empty() {
        use std::collections::BTreeMap;
        // block id -> (offset, len, available, total)
        // block id -> (offset, len, available, total, distinct non-live states)
        let mut per_block: BTreeMap<&str, (i64, i64, usize, usize, Vec<String>)> =
            BTreeMap::new();
        for r in &view.blocks {
            let e = per_block
                .entry(r.block_id.as_str())
                .or_insert((r.offset, r.len, 0, 0, Vec::new()));
            e.3 += 1;
            if r.available {
                e.2 += 1;
            } else if !e.4.contains(&r.state) {
                e.4.push(r.state.clone());
            }
        }

        let mut lines: Vec<Line> = Vec::new();
        for (id, (offset, len, avail, total, bad_states)) in per_block {
            let tone = if avail == 0 {
                Tone::Error
            } else if avail < total {
                Tone::Warn
            } else {
                Tone::Ok
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{id:<20}"), Style::default().fg(theme::DIR)),
                Span::styled(
                    format!("{:>9}", human_bytes(len)),
                    Style::default().fg(theme::TEXT),
                ),
                Span::styled(
                    format!("  @{:<10}", human_bytes(offset)),
                    Style::default().fg(theme::MUTED),
                ),
                components::dot(tone),
                Span::styled(
                    format!(" {avail}/{total}"),
                    Style::default().fg(tone.color()),
                ),
                Span::styled(
                    if bad_states.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", bad_states.join(", "))
                    },
                    Style::default().fg(th::ERROR),
                ),
            ]));
        }
        dlg = dlg.section("blocks by offset", lines);
    }

    dlg
}

/// Shared helper for the free-space column in other panels.
#[allow(dead_code)]
pub fn free_cell(capacity: i64, used: i64) -> Cell<'static> {
    Cell::from(human_bytes((capacity - used).max(0)))
}
