//! Cluster overview: capacity gauge, cluster stats, and per-worker load bars.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{
    components::{self, bar_with_pct, kv_line, Tag, Tone},
    human_bytes, ratio, theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // capacity gauge
            Constraint::Length(3), // inode / block stat strip
            Constraint::Min(0),    // stats + workers
        ])
        .split(area);

    render_capacity(f, app, rows[0]);
    render_strip(f, app, rows[1]);
    render_body(f, app, rows[2]);
}

fn render_capacity(f: &mut Frame, app: &App, area: Rect) {
    match (&app.cluster.data, app.cluster.loading) {
        (Some(c), _) => {
            let r = ratio(c.fs_used, c.capacity.max(1));
            let tone = th::usage_tone(r);
            components::render_usage(
                f,
                area,
                "capacity",
                r,
                format!(
                    "{} used / {}   ·   {} free   ·   {:.0}%",
                    human_bytes(c.fs_used),
                    human_bytes(c.capacity),
                    human_bytes(c.available),
                    r * 100.0
                ),
                tone,
            );
        }
        (None, true) => {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!(" {} ", components::live_spinner()),
                        Style::default().fg(th::ACCENT),
                    ),
                    Span::styled("connecting…", Style::default().fg(th::MUTED)),
                ]))
                .block(components::panel_block("capacity")),
                area,
            );
        }
        (None, false) => {
            let msg = app.cluster.error.as_deref().unwrap_or("no data");
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Tag::solid("offline", Tone::Error).span(),
                    Span::styled(format!(" {msg}"), Style::default().fg(th::ERROR)),
                ]))
                .block(components::panel_block("capacity")),
                area,
            );
        }
    }
}

/// A single-line strip of headline counters, each rendered as a badge.
fn render_strip(f: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();

    match &app.cluster.data {
        Some(c) => {
            let stat = |label: &str, value: String, tone: Tone| -> Vec<Span<'static>> {
                vec![
                    Span::styled(
                        format!("  {label} "),
                        Style::default().fg(th::MUTED),
                    ),
                    Tag::new(value, tone).span(),
                ]
            };
            spans.extend(stat("dirs", c.inode_dir_num.to_string(), Tone::Info));
            spans.extend(stat("files", c.inode_file_num.to_string(), Tone::Info));
            spans.extend(stat("blocks", c.block_num.to_string(), Tone::Neutral));
            spans.extend(stat(
                "workers",
                c.workers.len().to_string(),
                if c.workers.is_empty() {
                    Tone::Warn
                } else {
                    Tone::Ok
                },
            ));
            spans.extend(stat(
                "non-fs",
                human_bytes(c.non_fs_used),
                Tone::Neutral,
            ));
        }
        None => spans.push(Span::styled(
            " waiting for cluster metadata…",
            Style::default().fg(th::MUTED),
        )),
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(components::panel_block("at a glance")),
        area,
    );
}

fn render_body(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    render_summary(f, app, cols[0]);
    render_workers(f, app, cols[1]);
}

fn render_summary(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = vec![Line::from("")];

    match &app.cluster.data {
        Some(c) => {
            lines.push(kv_line("master", c.active_master.clone(), Tone::Accent, 13));
            lines.push(kv_line(
                "journal",
                format!("{} nodes", c.journal_nodes.len()),
                if c.journal_nodes.is_empty() {
                    Tone::Warn
                } else {
                    Tone::Ok
                },
                13,
            ));
            lines.push(kv_line("fs used", human_bytes(c.fs_used), Tone::Info, 13));
            lines.push(kv_line(
                "available",
                human_bytes(c.available),
                Tone::Ok,
                13,
            ));
            lines.push(kv_line(
                "non-fs used",
                human_bytes(c.non_fs_used),
                Tone::Neutral,
                13,
            ));
            lines.push(Line::from(""));
            lines.push(components::section("recent events"));
            for entry in app
                .log
                .iter()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let tone = if entry.contains("ERROR") {
                    Tone::Error
                } else {
                    Tone::Neutral
                };
                lines.push(Line::from(Span::styled(
                    format!("  {entry}"),
                    Style::default().fg(tone.color()),
                )));
            }
        }
        None => {
            lines.push(Line::from(Span::styled(
                if app.cluster.loading {
                    "  loading…"
                } else {
                    "  unavailable"
                },
                Style::default().fg(th::MUTED),
            )));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(components::panel_block("cluster")),
        area,
    );
}

fn render_workers(f: &mut Frame, app: &App, area: Rect) {
    // Drop low-value columns on narrow terminals rather than chopping the
    // worker address into unreadable fragments.
    let wide = area.width >= 56;
    let with_trend = area.width >= 74;

    let mut header_cells = vec![Cell::from("worker"), Cell::from("state")];
    if with_trend {
        header_cells.push(Cell::from("load"));
    }
    header_cells.push(Cell::from("used"));
    if wide {
        header_cells.push(Cell::from("free"));
        header_cells.push(Cell::from("blocks"));
    }

    let header = Row::new(header_cells).style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .cluster
        .data
        .as_ref()
        .map(|c| {
            c.workers
                .iter()
                .take(area.height.saturating_sub(3) as usize)
                .map(|w| {
                    let r = ratio(w.fs_used, w.capacity.max(1));
                    let mut cells = vec![
                        Cell::from(w.address.clone()).style(Style::default().fg(th::TEXT)),
                        Cell::from(Tag::from_status(&w.status).cell()),
                    ];
                    if with_trend {
                        cells.push(Cell::from(bar_with_pct(r, 12)).style(
                            Style::default().fg(th::usage_tone(r).color()),
                        ));
                    }
                    cells.push(Cell::from(human_bytes(w.fs_used)));
                    if wide {
                        cells.push(Cell::from(human_bytes(w.available)));
                        cells.push(Cell::from(w.block_num.to_string()));
                    }
                    Row::new(cells)
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match &app.cluster.data {
        Some(c) => format!("live workers — {}", c.workers.len()),
        None => "live workers".to_string(),
    };

    let mut widths: Vec<Constraint> = vec![Constraint::Min(16), Constraint::Length(10)];
    if with_trend {
        widths.push(Constraint::Length(13));
    }
    widths.push(Constraint::Length(10));
    if wide {
        widths.push(Constraint::Length(10));
        widths.push(Constraint::Min(8));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(components::panel_block(&title))
        .column_spacing(1);

    f.render_widget(table, area);
}
