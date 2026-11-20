//! Job list — table-driven with badges.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{
    components::{self, bar_with_pct, kv_line, Tone},
    human_bytes, theme as th,
};
use crate::app::App;

// ------------------------------------------------------------------- jobs

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(area);

    render_jobs_table(f, app, chunks[0]);
    render_jobs_summary(f, app, chunks[1]);
}

fn render_jobs_table(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 96;

    let mut header_cells = vec![
        Cell::from("job"),
        Cell::from("state"),
        Cell::from("progress"),
    ];
    if wide {
        header_cells.push(Cell::from("bytes"));
    }
    header_cells.push(Cell::from("source"));
    header_cells.push(Cell::from("target"));

    let header = Row::new(header_cells).style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .jobs
        .data
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|j| {
                    let mut cells = vec![
                        Cell::from(short_id(&j.job_id)).style(Style::default().fg(th::MUTED)),
                        Cell::from(components::dot_status(&j.state)),
                        Cell::from(bar_with_pct(j.progress, 13))
                            .style(Style::default().fg(th::usage_tone(1.0 - j.progress).color())),
                    ];
                    if wide {
                        cells.push(Cell::from(format!(
                            "{}/{}",
                            human_bytes(j.loaded_bytes as i64),
                            human_bytes(j.total_bytes as i64)
                        )));
                    }
                    cells.push(Cell::from(j.source.clone()));
                    cells.push(Cell::from(j.target.clone()));
                    Row::new(cells)
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.jobs.data, app.jobs.loading) {
        (Some(l), _) => format!("jobs — {} entries", l.len()),
        (None, true) => format!("jobs — {} loading", components::live_spinner()),
        (None, false) => format!("jobs — {}", app.jobs.error.as_deref().unwrap_or("no data")),
    };

    let mut widths = vec![
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Length(14),
    ];
    if wide {
        widths.push(Constraint::Length(12));
    }
    widths.push(Constraint::Percentage(28));
    widths.push(Constraint::Min(20));

    if rows.is_empty() {
        let block = components::panel_block(&title);
        let inner = block.inner(area);
        f.render_widget(block, area);
        if !app.jobs.loading && inner.height > 0 {
            let msg = app
                .jobs
                .error
                .as_deref()
                .unwrap_or("load jobs submitted in this session will appear here");
            f.render_widget(
                Paragraph::new(Span::styled(msg, Style::default().fg(th::MUTED)))
                    .alignment(Alignment::Center),
                Rect {
                    y: inner.y + inner.height / 2,
                    height: 1,
                    ..inner
                },
            );
        }
        return;
    }

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.job_selected.min(rows.len() - 1)));
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

fn render_jobs_summary(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ")];

    if let Some(list) = &app.jobs.data {
        let running = list
            .iter()
            .filter(|j| j.state.to_ascii_lowercase().contains("run"))
            .count();
        let done = list
            .iter()
            .filter(|j| j.state.to_ascii_lowercase().contains("complete"))
            .count();
        let failed = list
            .iter()
            .filter(|j| j.state.to_ascii_lowercase().contains("fail"))
            .count();

        let counter = |tone: Tone, n: usize, label: &str| -> Vec<Span<'static>> {
            vec![
                components::dot(if n > 0 { tone } else { Tone::Neutral }),
                Span::raw(" "),
                Span::styled(format!("{n} {label}"), Style::default().fg(th::MUTED)),
                Span::raw("   "),
            ]
        };
        spans.extend(counter(Tone::Accent, running, "running"));
        spans.extend(counter(Tone::Ok, done, "complete"));
        spans.extend(counter(Tone::Error, failed, "failed"));
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            "only jobs started in this session are listed",
            Style::default().fg(th::MUTED),
        ));
    } else {
        spans.push(Span::styled(
            "no job data",
            Style::default().fg(th::MUTED),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(components::panel_block("summary")),
        area,
    );
}

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..12])
    } else {
        id.to_string()
    }
}

/// Shared free/used cell helper.
#[allow(dead_code)]
fn space_cell(used: i64, total: i64) -> Cell<'static> {
    let r = if total <= 0 {
        0.0
    } else {
        used as f64 / total as f64
    };
    Cell::from(human_bytes(used)).style(Style::default().fg(th::usage_tone(r).color()))
}

#[allow(dead_code)]
fn demo_kv() -> Line<'static> {
    kv_line("key", "value", Tone::Neutral, 12)
}
