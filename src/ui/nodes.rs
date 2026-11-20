//! Node browser.
//!
//! Note on terminology: a worker reports no CPU or IO load metric — only
//! capacity/available/used bytes and a block count. The bar here is *disk
//! occupancy*, not load, and is labelled accordingly.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{
    components::{self, bar_with_pct, Tone},
    human_bytes, icons, ratio,
    theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(4), Constraint::Length(3)])
        .split(area);

    render_header(f, app, chunks[0]);
    render_table(f, app, chunks[1]);
    render_detail(f, app, chunks[2]);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let mid = Rect {
        y: area.y + area.height.saturating_sub(1) / 2,
        height: 1,
        ..area
    };
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(format!("{} ", icons::CUBES), Style::default().fg(th::ACCENT)),
    ];

    let badge = app.cluster.data.as_ref().map(|c| {
        let total_blocks: i64 = c.workers.iter().map(|w| w.block_num).sum();
        format!("{total_blocks} blocks")
    });
    let badge_w = badge
        .as_ref()
        .map(|b| components::pill_width(b) + 2)
        .unwrap_or(0);

    if let Some(c) = &app.cluster.data {
        let total_cap: i64 = c.workers.iter().map(|w| w.capacity).sum();
        let total_used: i64 = c.workers.iter().map(|w| w.fs_used).sum();
        let r = ratio(total_used, total_cap.max(1));

        spans.push(components::dot(if c.workers.is_empty() {
            Tone::Warn
        } else {
            Tone::Ok
        }));
        spans.push(Span::styled(
            format!(" {} live", c.workers.len()),
            Style::default().fg(th::MUTED),
        ));
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            bar_with_pct(r, 14),
            Style::default().fg(th::usage_tone(r).color()),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("{} / {}", human_bytes(total_used), human_bytes(total_cap)),
            Style::default().fg(th::TEXT),
        ));
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
        Paragraph::new(Line::from(spans)),
        Rect {
            width: area.width.saturating_sub(badge_w),
            ..mid
        },
    );
    if let Some(badge) = badge {
        components::pills_end(f, area, &[(badge, Tone::Neutral)]);
    }
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 84;

    let mut header_cells = vec![
        Cell::from("  ip"),
        Cell::from("state"),
        Cell::from("disk used"),
        Cell::from("used"),
    ];
    if wide {
        header_cells.push(Cell::from("free"));
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

    let workers = app.cluster.data.as_ref().map(|c| &c.workers);
    let rows: Vec<Row> = workers
        .map(|list| {
            list.iter()
                .map(|w| {
                    let r = ratio(w.fs_used, w.capacity.max(1));
                    let mut cells = vec![
                        Cell::from(Line::from(vec![
                            Span::styled(
                                format!("{} ", icons::SERVER),
                                Style::default().fg(th::MUTED),
                            ),
                            Span::styled(
                                w.address.clone(),
                                Style::default().fg(th::DIR),
                            ),
                        ])),
                        Cell::from(components::dot_status(&w.status)),
                        Cell::from(bar_with_pct(r, 12))
                            .style(Style::default().fg(th::usage_tone(r).color())),
                        Cell::from(human_bytes(w.fs_used)).style(Style::default().fg(th::TEXT)),
                    ];
                    if wide {
                        cells.push(Cell::from(human_bytes(w.available)));
                        cells.push(Cell::from(human_bytes(w.capacity)));
                        cells.push(Cell::from(w.block_num.to_string()));
                        cells.push(
                            Cell::from(w.version.clone())
                                .style(Style::default().fg(th::MUTED)),
                        );
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
        Constraint::Length(24),
        Constraint::Length(10),
        Constraint::Length(15),
        Constraint::Length(10),
    ];
    if wide {
        widths.push(Constraint::Length(10));
        widths.push(Constraint::Length(10));
        widths.push(Constraint::Length(8));
        widths.push(Constraint::Length(10));
    }
    widths.push(Constraint::Length(7));

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.node_selected.min(rows.len() - 1)));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(components::panel_block(&title))
        .row_highlight_style(Style::default().bg(th::SELECTED_BG))
        .highlight_symbol("▶ ")
        .column_spacing(1);

    f.render_stateful_widget(table, area, &mut state);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let mid = Rect {
        y: area.y + area.height.saturating_sub(1) / 2,
        height: 1,
        ..area
    };
    let Some(w) = app
        .cluster
        .data
        .as_ref()
        .and_then(|c| c.workers.get(app.node_selected))
    else {
        f.render_widget(
            Paragraph::new(Span::styled(
                " no worker selected — press d to describe",
                Style::default().fg(th::MUTED),
            )),
            Rect {
                width: area.width,
                ..mid
            },
        );
        return;
    };

    // State is a status badge, so it gets a pill; the metrics stay plain.
    let tone = components::tone_for_status(&w.status);
    let pill_rect = components::pill(
        f,
        Rect {
            x: area.x + 1,
            width: area.width.saturating_sub(1),
            ..area
        },
        format!("● {}", w.status),
        tone,
    );
    let r = ratio(w.fs_used, w.capacity.max(1));
    let hint = "d detail ";
    let hint_w = hint.chars().count() as u16;
    let x = pill_rect.right().saturating_add(1);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", icons::SERVER),
                Style::default().fg(th::DIR),
            ),
            Span::styled(w.address.clone(), Style::default().fg(th::TEXT)),
            Span::raw("  "),
            Span::styled(
                format!("{} blocks", w.block_num),
                Style::default().fg(th::MUTED),
            ),
            Span::raw("  "),
            Span::styled(
                format!("disk {:.0}%", r * 100.0),
                Style::default().fg(th::usage_tone(r).color()),
            ),
            Span::raw("  "),
            Span::styled(
                format!("weight {}", w.weight),
                Style::default().fg(th::MUTED),
            ),
        ])),
        Rect {
            x,
            width: area.right().saturating_sub(x + hint_w + 1),
            ..mid
        },
    );
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(th::BORDER))),
        Rect {
            x: area.right().saturating_sub(hint_w),
            width: hint_w,
            ..mid
        },
    );
}
