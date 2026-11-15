//! Transfer panel: per-file progress bars plus a guided put/get form.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Clear, Gauge, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{components, human_bytes, theme as th};
use crate::app::{App, TransferForm};
use crate::event::{TransferKind, TransferState};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(area);

    render_table(f, app, chunks[0]);
    render_active_gauge(f, app, chunks[1]);

    if let Some(form) = &app.form {
        render_form(f, form, area);
    }
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("id"),
        Cell::from("dir"),
        Cell::from("source"),
        Cell::from("target"),
        Cell::from("progress"),
        Cell::from("size"),
        Cell::from("speed"),
        Cell::from("state"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .transfers
        .iter()
        .map(|t| {
            let pct = t.ratio();
            let state_style = match t.state {
                TransferState::Done => Style::default().fg(th::OK),
                TransferState::Failed => Style::default().fg(th::ERROR),
                TransferState::Cancelled => Style::default().fg(th::MUTED),
                TransferState::Running => Style::default().fg(th::ACCENT),
                TransferState::Queued => Style::default().fg(th::MUTED),
            };
            // 10-cell textual bar keeps alignment exact inside a table cell.
            let filled = (pct * 10.0).round() as usize;
            let bar = format!(
                "{}{} {:>3.0}%",
                "█".repeat(filled),
                "░".repeat(10 - filled),
                pct * 100.0
            );
            let (source, target) = match t.kind {
                TransferKind::Upload => (t.local.clone(), t.remote.clone()),
                TransferKind::Download => (t.remote.clone(), t.local.clone()),
            };
            Row::new(vec![
                Cell::from(format!("#{}", t.id)),
                Cell::from(t.kind.arrow()).style(Style::default().fg(th::DIR)),
                Cell::from(source),
                Cell::from(target),
                Cell::from(bar).style(Style::default().fg(th::usage_color(pct))),
                Cell::from(if t.total > 0 {
                    human_bytes(t.total as i64)
                } else {
                    "-".to_string()
                }),
                Cell::from(if t.speed > 0 {
                    format!("{}/s", human_bytes(t.speed as i64))
                } else {
                    "-".to_string()
                }),
                Cell::from(t.state.label()).style(state_style),
            ])
        })
        .collect();

    let title = if app.transfers.is_empty() {
        "transfers — press p (upload) or g (download)".to_string()
    } else {
        let running = app
            .transfers
            .iter()
            .filter(|t| t.state == TransferState::Running)
            .count();
        format!("transfers — {} total, {} running", app.transfers.len(), running)
    };

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.transfer_selected.min(rows.len() - 1)));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Percentage(24),
            Constraint::Percentage(24),
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Min(10),
        ],
    )
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

/// Aggregate gauge for the currently selected transfer, so a long upload is
/// readable at a glance without scanning the table.
fn render_active_gauge(f: &mut Frame, app: &App, area: Rect) {
    let Some(t) = app.transfers.get(app.transfer_selected) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " no active transfer ",
                Style::default().fg(th::MUTED),
            )))
            .block(components::panel_block("current")),
            area,
        );
        return;
    };

    if t.state.is_terminal() {
        let text = match (&t.error, t.state) {
            (Some(err), _) => format!(" #{} {} — {err}", t.id, t.state.label()),
            (None, s) => format!(
                " #{} {} — {}",
                t.id,
                s.label(),
                human_bytes(t.transferred as i64)
            ),
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(if t.state == TransferState::Failed {
                    th::ERROR
                } else {
                    th::OK
                }),
            )))
            .block(components::panel_block("current")),
            area,
        );
        return;
    }

    let label = format!(
        "{}  {} / {}   {}",
        t.state.label(),
        human_bytes(t.transferred as i64),
        if t.total > 0 {
            human_bytes(t.total as i64)
        } else {
            "?".to_string()
        },
        if t.speed > 0 {
            format!("{}/s", human_bytes(t.speed as i64))
        } else {
            String::new()
        }
    );

    let g = Gauge::default()
        .block(components::panel_block("current"))
        .gauge_style(Style::default().fg(th::ACCENT).add_modifier(Modifier::BOLD))
        .ratio(t.ratio())
        .label(Span::styled(label, Style::default().fg(th::TEXT)));
    f.render_widget(g, area);
}

fn render_form(f: &mut Frame, form: &TransferForm, area: Rect) {
    let rect = components::centered_rect(64, 40, area);
    f.render_widget(Clear, rect);

    let title = match form.kind {
        TransferKind::Upload => "upload  local -> curvine",
        TransferKind::Download => "download  curvine -> local",
    };

    let field = |label: &str, value: &str, focused: bool| {
        let cursor = if focused { "▏" } else { "" };
        let style = if focused {
            Style::default().fg(th::TEXT)
        } else {
            Style::default().fg(th::MUTED)
        };
        Line::from(vec![
            Span::styled(
                format!("  {label:<8}"),
                Style::default().fg(th::ACCENT_DIM),
            ),
            Span::styled(value.to_string(), style),
            Span::styled(cursor, Style::default().fg(th::ACCENT)),
        ])
    };

    let local_label = match form.kind {
        TransferKind::Upload => "local",
        TransferKind::Download => "dest",
    };

    let mut lines = vec![
        Line::from(""),
        field(local_label, &form.local, form.focus == 0),
        Line::from(""),
        field("remote", &form.remote, form.focus == 1),
        Line::from(""),
    ];

    if let Some(err) = &form.error {
        lines.push(Line::from(Span::styled(
            format!("  {err}"),
            Style::default().fg(th::ERROR),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  tab switch field   enter transfer   esc cancel",
            Style::default().fg(th::MUTED),
        )));
    }

    f.render_widget(
        Paragraph::new(lines)
            .block(components::panel_block(title))
            .alignment(Alignment::Left),
        rect,
    );
}
