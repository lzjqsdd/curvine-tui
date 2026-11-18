//! Cluster overview.
//!
//! One `cluster` panel carries every aggregate (capacity, namespace counts,
//! storage) — previously capacity / at-a-glance / cluster repeated each other
//! and pushed the worker table into whatever space was left.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{
    components::{self, bar, bar_with_pct, section, Tag, Tone},
    human_bytes, icons, ratio, theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);

    render_cluster(f, app, cols[0]);
    render_workers(f, app, cols[1]);
}

fn render_cluster(f: &mut Frame, app: &App, area: Rect) {
    // No leading blank: the first row is the capacity bar so the column stays
    // information-dense on short terminals.
    let mut lines: Vec<Line> = Vec::new();

    match (&app.cluster.data, app.cluster.loading) {
        (Some(c), _) => {
            // The spring follows the real ratio; the raw value is used for the
            // text so the numbers never lag behind the data.
            let cap_ratio = app.anim.capacity.ratio();
            let _ = ratio(c.fs_used, c.capacity.max(1));

            lines.push(section("capacity"));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    bar(cap_ratio, 18),
                    Style::default().fg(th::usage_tone(cap_ratio).color()),
                ),
                Span::styled(
                    format!(" {:>3.0}%", cap_ratio * 100.0),
                    Style::default().fg(th::usage_tone(cap_ratio).color()),
                ),
                Span::styled(
                    format!(
                        "   {} / {}",
                        human_bytes(c.fs_used),
                        human_bytes(c.capacity)
                    ),
                    Style::default().fg(th::TEXT),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{:<18}", "free"),
                    Style::default().fg(th::MUTED),
                ),
                Span::styled(human_bytes(c.available), Style::default().fg(th::OK)),
            ]));
            if c.non_fs_used > 0 {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{:<18}", "non-fs used"),
                        Style::default().fg(th::MUTED),
                    ),
                    Span::styled(human_bytes(c.non_fs_used), Style::default().fg(th::MUTED)),
                ]));
            }

            lines.push(section("namespace"));
            // Counters animate from their previous value; the label prints the
            // rounded spring, so a refresh counts up instead of snapping.
            lines.push(count_line(
                icons::DIR,
                "directories",
                app.anim.dirs.value().round() as i64,
                Tone::Info,
            ));
            lines.push(count_line(
                icons::FILE,
                "files",
                app.anim.files.value().round() as i64,
                Tone::Info,
            ));
            lines.push(count_line(
                icons::CUBES,
                "blocks",
                app.anim.blocks.value().round() as i64,
                Tone::Neutral,
            ));

            lines.push(section("members"));
            let master_tone = Tone::Ok;
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<18}", "master"),
                    Style::default().fg(th::MUTED),
                ),
                components::dot(master_tone),
                Span::raw(" "),
                Span::styled(c.active_master.clone(), Style::default().fg(th::ACCENT)),
            ]));
            let journal_tone = if c.journal_nodes.is_empty() {
                Tone::Warn
            } else {
                Tone::Ok
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<18}", "journal"),
                    Style::default().fg(th::MUTED),
                ),
                components::dot(journal_tone),
                Span::raw(" "),
                Span::styled(
                    format!("{} nodes", c.journal_nodes.len()),
                    Style::default().fg(th::TEXT),
                ),
            ]));
            let worker_tone = if c.workers.is_empty() {
                Tone::Warn
            } else {
                Tone::Ok
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<18}", "workers"),
                    Style::default().fg(th::MUTED),
                ),
                components::dot(worker_tone),
                Span::raw(" "),
                Span::styled(
                    format!("{:.0} live", app.anim.workers.value()),
                    Style::default().fg(th::TEXT),
                ),
            ]));

            // Recent events only when there is room for them.
            if area.height > lines.len() as u16 + 4 {
                lines.push(Line::from(""));
                lines.push(section("recent events"));
                for entry in app.log.iter().rev().take(5).collect::<Vec<_>>().into_iter().rev() {
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
        }
        (None, true) => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(components::live_spinner(), Style::default().fg(th::ACCENT)),
                Span::raw(" "),
                Span::styled("connecting…", Style::default().fg(th::MUTED)),
            ]));
        }
        (None, false) => {
            let msg = app.cluster.error.as_deref().unwrap_or("no data");
            lines.push(Line::from(vec![
                Span::raw("  "),
                components::dot(Tone::Error),
                Span::styled(" offline ", Style::default().fg(th::ERROR)),
                Span::styled(msg.to_string(), Style::default().fg(th::MUTED)),
            ]));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(components::panel_block("cluster")),
        area,
    );
}

fn count_line(glyph: &'static str, label: &str, value: i64, tone: Tone) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{glyph} "), Style::default().fg(tone.color())),
        Span::styled(format!("{label:<16}"), Style::default().fg(th::MUTED)),
        Span::styled(value.to_string(), Style::default().fg(th::TEXT)),
    ])
}

fn render_workers(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 50;
    let with_bar = area.width >= 66;

    let mut header_cells = vec![Cell::from("  ip"), Cell::from("state")];
    if with_bar {
        header_cells.push(Cell::from("disk used"));
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
                .map(|w| {
                    let r = ratio(w.fs_used, w.capacity.max(1));
                    let mut cells = vec![
                        Cell::from(Line::from(vec![
                            Span::raw(" "),
                            Span::styled(format!("{} ", icons::SERVER), Style::default().fg(th::MUTED)),
                            Span::styled(w.address.clone(), Style::default().fg(th::TEXT)),
                        ])),
                        Cell::from(components::dot_status(&w.status)),
                    ];
                    if with_bar {
                        cells.push(
                            Cell::from(bar_with_pct(r, 11))
                                .style(Style::default().fg(th::usage_tone(r).color())),
                        );
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

    let mut widths = vec![Constraint::Min(18), Constraint::Length(10)];
    if with_bar {
        widths.push(Constraint::Length(14));
    }
    widths.push(Constraint::Length(10));
    if wide {
        widths.push(Constraint::Length(10));
        widths.push(Constraint::Min(8));
    }

    let title = match &app.cluster.data {
        Some(c) => format!("workers — {}", c.workers.len()),
        None => "workers".to_string(),
    };

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.node_selected.min(rows.len() - 1)));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(components::panel_block_badged(
            &title,
            Tag::new("disk %, not CPU", Tone::Neutral),
        ))
        .row_highlight_style(
            Style::default()
                .bg(th::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ")
        .column_spacing(1);

    f.render_stateful_widget(table, area, &mut state);
}
