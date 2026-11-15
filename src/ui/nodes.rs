//! Node browser: per-worker load bars, state badges, and a summary header.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{
    components::{self, bar_with_pct, Tag, Tone},
    human_bytes, ratio, theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_header(f, app, chunks[0]);
    render_table(f, app, chunks[1]);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ")];

    if let Some(c) = &app.cluster.data {
        let total_cap: i64 = c.workers.iter().map(|w| w.capacity).sum();
        let total_used: i64 = c.workers.iter().map(|w| w.fs_used).sum();
        let total_blocks: i64 = c.workers.iter().map(|w| w.block_num).sum();
        let r = ratio(total_used, total_cap.max(1));

        spans.push(Tag::new(format!("{} live", c.workers.len()), Tone::Ok).span());
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            bar_with_pct(r, 14),
            Style::default().fg(th::usage_tone(r).color()),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("{} / {}", human_bytes(total_used), human_bytes(total_cap)),
            Style::default().fg(th::TEXT),
        ));
        spans.push(Span::raw("  "));
        spans.push(Tag::new(format!("{total_blocks} blocks"), Tone::Neutral).span());
    } else {
        spans.push(Span::styled(
            if app.cluster.loading {
                format!(" {} loading nodes…", components::live_spinner())
            } else {
                " no node data".to_string()
            },
            Style::default().fg(th::MUTED),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(components::panel_block("cluster workers")),
        area,
    );
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 80;

    let mut header_cells = vec![
        Cell::from("address"),
        Cell::from("state"),
        Cell::from("load"),
        Cell::from("used"),
    ];
    if wide {
        header_cells.push(Cell::from("available"));
        header_cells.push(Cell::from("capacity"));
        header_cells.push(Cell::from("blocks"));
        header_cells.push(Cell::from("version"));
    }
    header_cells.push(Cell::from("weight"));

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
                .map(|w| {
                    let r = ratio(w.fs_used, w.capacity.max(1));
                    let mut cells = vec![
                        Cell::from(w.address.clone()).style(Style::default().fg(th::DIR)),
                        Cell::from(Tag::from_status(&w.status).cell()),
                        Cell::from(bar_with_pct(r, 12))
                            .style(Style::default().fg(th::usage_tone(r).color())),
                        Cell::from(human_bytes(w.fs_used)).style(Style::default().fg(th::TEXT)),
                    ];
                    if wide {
                        cells.push(Cell::from(human_bytes(w.available)));
                        cells.push(Cell::from(human_bytes(w.capacity)));
                        cells.push(Cell::from(w.block_num.to_string()));
                        cells.push(Cell::from(w.version.clone()).style(
                            Style::default().fg(th::MUTED),
                        ));
                    }
                    cells.push(
                        Cell::from(w.weight.to_string()).style(Style::default().fg(th::MUTED)),
                    );
                    Row::new(cells)
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.cluster.data, app.cluster.loading) {
        (Some(c), _) => format!("nodes — {} live", c.workers.len()),
        (None, true) => format!("nodes — {} loading", components::live_spinner()),
        (None, false) => format!(
            "nodes — {}",
            app.cluster.error.as_deref().unwrap_or("no data")
        ),
    };

    let mut widths = vec![
        Constraint::Length(22),
        Constraint::Length(10),
        Constraint::Length(13),
        Constraint::Length(10),
    ];
    if wide {
        widths.push(Constraint::Length(10));
        widths.push(Constraint::Length(10));
        widths.push(Constraint::Length(8));
        widths.push(Constraint::Length(10));
    }
    widths.push(Constraint::Length(7));

    let table = Table::new(rows, widths)
        .header(header)
        .block(components::panel_block(&title))
        .column_spacing(1);

    f.render_widget(table, area);
}
