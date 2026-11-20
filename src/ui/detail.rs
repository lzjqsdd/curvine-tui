//! Full-screen detail views for files and workers.
//!
//! These replace the old describe popup for the two subjects that need room:
//! a file's block map plus per-worker distribution, and a worker's full
//! endpoint/capacity/storage breakdown.

use std::collections::BTreeMap;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{
    components::{self, bar, bar_with_pct, kv_line, section, Tag, Tone},
    human_bytes, theme as th,
};
use crate::app::{DetailKind, DetailView};

/// (offset, len, available replicas, total replicas, non-live states)
type BlockAgg<'a> = (i64, i64, usize, usize, Vec<&'a str>);

pub fn render(f: &mut Frame, view: &DetailView, area: Rect) {
    match view.kind {
        DetailKind::File => render_file(f, view, area),
        DetailKind::Worker => render_worker(f, view, area),
    }
}

fn render_file(f: &mut Frame, view: &DetailView, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        // Just over a third gives the 7-row kv block its headline plus one
        // row of breathing room; the tables take everything below.
        .constraints([Constraint::Percentage(36), Constraint::Min(3)])
        .split(area);

    render_kv(f, view, rows[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(rows[1]);
    render_blocks(f, view, cols[0]);
    render_distribution(f, view, cols[1]);
}

fn render_worker(f: &mut Frame, view: &DetailView, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(64), Constraint::Min(3)])
        .split(area);
    render_kv(f, view, rows[0]);
    render_storage(f, view, rows[1]);
}

fn value_tone(key: &str, value: &str) -> Tone {
    match key {
        "state" => components::tone_for_status(value),
        "path" | "endpoint" | "ip" => Tone::Info,
        "blocks" => Tone::Accent,
        "occupancy" => {
            // The string is "NN.N% (disk, not CPU load)"; thresholds still
            // apply to the number before the percent sign.
            let pct = value
                .split('%')
                .next()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(0.0);
            th::usage_tone(pct / 100.0)
        }
        _ => Tone::Neutral,
    }
}

fn render_kv(f: &mut Frame, view: &DetailView, area: Rect) {
    // Two extra cells guarantee a gap after the longest label; without it the
    // widest key ("modified") would run straight into its value.
    let label_w = view
        .rows
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(8)
        + 2;
    let block = components::panel_block(&view.title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut top = inner.y;
    if let Some((tag, subject)) = &view.headline {
        let badge_w = components::pill_width(tag).min(inner.width.saturating_sub(1));
        let badge = Rect {
            x: inner.x + 1,
            y: inner.y,
            width: badge_w,
            height: inner.height.min(1),
        };
        f.render_widget(
            Paragraph::new(components::badge(tag.clone(), Tone::Info)),
            badge,
        );
        if badge.height >= 1 {
            f.render_widget(
                Paragraph::new(Span::styled(
                    subject.clone(),
                    Style::default()
                        .fg(th::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )),
                Rect {
                    x: badge.right().saturating_add(1),
                    y: badge.y,
                    width: inner.right().saturating_sub(badge.right() + 1),
                    height: 1,
                },
            );
        }
        top = inner.y + 1;
    }

    let lines: Vec<Line> = view
        .rows
        .iter()
        .map(|(key, value)| kv_line(key, value.clone(), value_tone(key, value), label_w))
        .collect();
    f.render_widget(
        ratatui::widgets::Paragraph::new(lines),
        Rect {
            y: top,
            height: inner.bottom().saturating_sub(top),
            ..inner
        },
    );
}

fn render_blocks(f: &mut Frame, view: &DetailView, area: Rect) {
    // One row per block, with the replica count collapsed into avail/total.
    let mut per_block: BTreeMap<&str, BlockAgg> = BTreeMap::new();
    for r in &view.blocks {
        let entry = per_block
            .entry(r.block_id.as_str())
            .or_insert((r.offset, r.len, 0, 0, Vec::new()));
        entry.3 += 1;
        if r.available {
            entry.2 += 1;
        } else if !entry.4.contains(&r.state.as_str()) {
            entry.4.push(r.state.as_str());
        }
    }

    // The table is titled by offset, so sort by it rather than by block id.
    let mut blocks: Vec<(&str, BlockAgg)> = per_block.into_iter().collect();
    blocks.sort_by_key(|(id, (offset, ..))| (*offset, *id));

    let rows: Vec<Row> = blocks
        .iter()
        .map(|(id, (offset, len, avail, total, bad))| {
            let tone = if *avail == 0 {
                Tone::Error
            } else if avail < total {
                Tone::Warn
            } else {
                Tone::Ok
            };
            Row::new(vec![
                Cell::from((*id).to_string()).style(Style::default().fg(th::DIR)),
                Cell::from(human_bytes(*len)).style(Style::default().fg(th::TEXT)),
                Cell::from(human_bytes(*offset)).style(Style::default().fg(th::MUTED)),
                Cell::from(components::dot_label(tone, format!("{avail}/{total}"))),
                Cell::from(if bad.is_empty() {
                    Span::styled("live", Style::default().fg(th::OK))
                } else {
                    Span::styled(bad.join(", "), Style::default().fg(th::ERROR))
                }),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("block"),
        Cell::from("size"),
        Cell::from("offset"),
        Cell::from("avail"),
        Cell::from("state"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let title = if blocks.is_empty() {
        "blocks by offset".to_string()
    } else {
        format!("blocks by offset — {}", blocks.len())
    };
    let badge = if let Some(err) = &view.error {
        Tag::new(format!("error: {err}"), Tone::Error)
    } else if view.loading {
        Tag::new(format!("{} loading", components::live_spinner()), Tone::Accent)
    } else {
        Tag::new("replica availability", Tone::Neutral)
    };
    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Min(8),
        ],
    )
    .header(header)
    .block(components::panel_block_badged(&title, badge))
    .column_spacing(1);
    f.render_widget(table, area);
}

fn render_distribution(f: &mut Frame, view: &DetailView, area: Rect) {
    let total_bytes: i64 = view.worker_blocks.iter().map(|w| w.bytes).sum();
    let rows: Vec<Row> = view
        .worker_blocks
        .iter()
        .map(|w| {
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
            Row::new(vec![
                Cell::from(Line::from(vec![
                    components::dot(tone),
                    Span::raw(" "),
                    Span::styled(w.worker.clone(), Style::default().fg(th::TEXT)),
                ])),
                Cell::from(w.block_ids.len().to_string()),
                Cell::from(human_bytes(w.bytes)),
                Cell::from(format!(
                    "{} {:>3.0}%",
                    bar(share, 5),
                    share * 100.0
                ))
                .style(Style::default().fg(th::usage_tone(1.0 - share).color())),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("worker"),
        Cell::from("blocks"),
        Cell::from("bytes"),
        Cell::from("share"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let title = if view.worker_blocks.is_empty() {
        "distribution by worker".to_string()
    } else {
        format!("distribution by worker — {}", view.worker_blocks.len())
    };
    let table = Table::new(
        rows,
        [
            Constraint::Min(8),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(11),
        ],
    )
    .header(header)
    .block(components::panel_block(&title))
    .column_spacing(1);
    f.render_widget(table, area);
}

fn render_storage(f: &mut Frame, view: &DetailView, area: Rect) {
    let total: i64 = view.storage.iter().map(|(_, v)| *v).sum();
    let rows: Vec<Row> = view
        .storage
        .iter()
        .map(|(class, bytes)| {
            let share = if total > 0 {
                *bytes as f64 / total as f64
            } else {
                0.0
            };
            Row::new(vec![
                Cell::from(Tag::new(class.clone(), Tone::Info).cell()),
                Cell::from(human_bytes(*bytes)).style(Style::default().fg(th::TEXT)),
                Cell::from(bar_with_pct(share, 16))
                    .style(Style::default().fg(th::usage_tone(1.0 - share).color())),
            ])
        })
        .collect();
    let header = Row::new(vec![
        Cell::from("class"),
        Cell::from("capacity"),
        Cell::from("share"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let mut lines: Vec<Line> = Vec::new();
    if view.storage.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("no storage breakdown reported by this worker"));
    }

    // The storage table and the empty-state note share the same block so the
    // lower third never looks like an unexplained blank panel.
    if rows.is_empty() {
        f.render_widget(
            ratatui::widgets::Paragraph::new(lines)
                .block(components::panel_block("storage classes")),
            area,
        );
        return;
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(components::panel_block("storage classes"))
    .column_spacing(1);
    f.render_widget(table, area);
}
