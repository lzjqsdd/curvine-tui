//! Mount table plus the mount/unmount forms.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};

use super::{
    components::{self, input_line, popup, section, choice_line, toggle_line, Tag, Tone},
    human_bytes, theme as th,
};
use crate::app::{App, MountForm};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);

    render_table(f, app, rows[0]);
    render_selected(f, app, rows[1]);

    if let Some(form) = &app.mount_form {
        render_form(f, form, area);
    }
    if let Some(path) = &app.umount_confirm {
        render_confirm(f, path, area);
    }
}

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
    let line = match app.mounts.data.as_ref().and_then(|l| l.get(app.mount_selected)) {
        Some(m) => Line::from(vec![
            Span::styled(" selected ", Style::default().fg(th::MUTED)),
            Tag::new(m.cv_path.clone(), Tone::Info).span(),
            Span::styled(" ← ", Style::default().fg(th::BORDER)),
            Span::styled(m.ufs_path.clone(), Style::default().fg(th::TEXT)),
            Span::raw("  "),
            Tag::new(format!("id {}", m.mount_id), Tone::Neutral).span(),
            Span::raw(" "),
            Tag::new(format!("ttl {}", if m.ttl_ms > 0 { m.ttl_ms / 1000 } else { 0 }), Tone::Neutral)
                .span(),
            if m.storage_type.is_empty() {
                Span::raw("")
            } else {
                Tag::new(m.storage_type.clone(), Tone::Accent).span()
            },
        ]),
        None => Line::from(Span::styled(
            " n new mount   u unmount   r refresh — no mount selected",
            Style::default().fg(th::MUTED),
        )),
    };

    f.render_widget(
        Paragraph::new(line).block(components::panel_block("selected")),
        area,
    );
}

fn render_form(f: &mut Frame, form: &MountForm, area: Rect) {
    let inner = popup(f, area, "new mount");
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
        lines.push(Line::from(vec![
            Tag::solid("error", Tone::Error).span(),
            Span::styled(format!(" {err}"), Style::default().fg(th::ERROR)),
        ]));
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

fn render_confirm(f: &mut Frame, path: &str, area: Rect) {
    let inner = components::centered_rect(52, 24, area);
    f.render_widget(ratatui::widgets::Clear, inner);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  unmount ", Style::default().fg(th::TEXT)),
            Tag::new(path.to_string(), Tone::Warn).span(),
            Span::styled(" ?", Style::default().fg(th::TEXT)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  cached data on this mount becomes unreachable.",
            Style::default().fg(th::MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  enter ", Style::default().fg(th::ERROR)),
            Span::styled("confirm   ", Style::default().fg(th::MUTED)),
            Span::styled("esc ", Style::default().fg(th::ACCENT_DIM)),
            Span::styled("cancel", Style::default().fg(th::MUTED)),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines).block(components::panel_block("confirm")),
        inner,
    );
}

/// Shared helper for the free-space column in other panels.
#[allow(dead_code)]
pub fn free_cell(capacity: i64, used: i64) -> Cell<'static> {
    Cell::from(human_bytes((capacity - used).max(0)))
}
