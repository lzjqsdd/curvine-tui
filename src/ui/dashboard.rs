//! Cluster overview: capacity, inode counts, top workers.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{components, human_bytes, ratio, theme as th};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // ---- capacity gauge
    if let Some(c) = &app.cluster.data {
        let r = ratio(c.fs_used, c.capacity.max(1));
        components::render_usage(
            f,
            rows[0],
            "capacity",
            r,
            format!(
                "{} used / {} total   ({} available)",
                human_bytes(c.fs_used),
                human_bytes(c.capacity),
                human_bytes(c.available)
            ),
        );
    } else {
        let msg = if app.cluster.loading {
            format!("{} connecting…", components::spinner(0))
        } else {
            app.cluster.error.clone().unwrap_or_else(|| "no data".into())
        };
        f.render_widget(
            Paragraph::new(msg).block(components::panel_block("capacity")),
            rows[0],
        );
    }

    // ---- detail panels
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(rows[1]);

    render_summary(f, app, cols[0]);
    render_workers(f, app, cols[1]);
}

fn render_summary(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    match &app.cluster.data {
        Some(c) => {
            let kv = |k: &str, v: String| {
                Line::from(vec![
                    Span::styled(format!("  {k:<13}"), Style::default().fg(th::MUTED)),
                    Span::styled(v, Style::default().fg(th::TEXT)),
                ])
            };
            lines.push(Line::from(""));
            lines.push(kv("master", c.active_master.clone()));
            lines.push(kv("journal", format!("{} nodes", c.journal_nodes.len())));
            lines.push(kv("directories", c.inode_dir_num.to_string()));
            lines.push(kv("files", c.inode_file_num.to_string()));
            lines.push(kv("blocks", c.block_num.to_string()));
            lines.push(kv("fs used", human_bytes(c.fs_used)));
            lines.push(kv("non-fs used", human_bytes(c.non_fs_used)));
            lines.push(kv("available", human_bytes(c.available)));
            lines.push(kv("workers", c.workers.len().to_string()));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  recent events",
                Style::default()
                    .fg(th::ACCENT_DIM)
                    .add_modifier(Modifier::BOLD),
            )));
            for entry in app.log.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
                lines.push(Line::from(Span::styled(
                    format!("  {entry}"),
                    Style::default().fg(th::MUTED),
                )));
            }
        }
        None => {
            let msg = if app.cluster.loading {
                "loading…"
            } else {
                "unavailable"
            };
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {msg}"),
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
    // Drop the low-value columns on narrow terminals instead of letting fixed
    // widths chop the worker address into unreadable fragments.
    let wide = area.width >= 52;
    let mut header_cells = vec![Cell::from("worker"), Cell::from("status"), Cell::from("used")];
    if wide {
        header_cells.push(Cell::from("avail"));
        header_cells.push(Cell::from("blocks"));
    }
    let header = Row::new(header_cells)
    .style(
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
                    let mut cells = vec![
                        Cell::from(w.address.clone()),
                        Cell::from(w.status.clone()).style(Style::default().fg(th::OK)),
                        Cell::from(human_bytes(w.fs_used)),
                    ];
                    if wide {
                        cells.push(Cell::from(human_bytes(w.available)));
                        cells.push(Cell::from(w.block_num.to_string()));
                    }
                    Row::new(cells)
                })
                .collect()
        })
        .unwrap_or_default();

    let widths: Vec<Constraint> = if wide {
        vec![
            Constraint::Percentage(34),
            Constraint::Percentage(18),
            Constraint::Percentage(17),
            Constraint::Percentage(17),
            Constraint::Percentage(14),
        ]
    } else {
        vec![
            Constraint::Percentage(42),
            Constraint::Percentage(28),
            Constraint::Percentage(30),
        ]
    };

    let table = Table::new(rows, widths)
    .header(header)
    .block(components::panel_block("live workers"))
    .column_spacing(1);

    f.render_widget(table, area);
}
