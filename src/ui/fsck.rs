//! Replica-health panel: verdict gauge, stat badges, unhealthy-file table.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{
    components::{self, kv_line, Tag, Tone},
    human_bytes, theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // verdict gauge
            Constraint::Length(8), // numeric summary, one kv row per counter
            Constraint::Min(3),    // unhealthy files
        ])
        .split(area);

    render_verdict(f, app, rows[0]);
    render_summary(f, app, rows[1]);
    render_bad_files(f, app, rows[2]);
}

fn render_verdict(f: &mut Frame, app: &App, area: Rect) {
    if let (Some(s), false) = (&app.fsck.data, app.fsck.loading) {
        let tone = if !s.warnings { Tone::Ok } else { Tone::Error };
        let headline = if s.warnings { "degraded" } else { "healthy" };
        components::render_usage(
            f,
            area,
            &format!("fsck {}", s.root),
            s.health_ratio,
            format!(
                "{headline}   {:.1}% of {:.0} replicas available   ·   {} ms",
                s.health_ratio * 100.0,
                s.expected_replicas,
                s.elapsed_ms
            ),
            tone,
        );
        return;
    }

    let (line, tone) = if app.fsck.loading {
        (
            Line::from(vec![
                Span::styled(
                    format!(" {} ", components::live_spinner()),
                    Style::default().fg(th::ACCENT),
                ),
                Span::styled(
                    format!("scanning {} … press x to cancel", app.fsck_path),
                    Style::default().fg(th::MUTED),
                ),
            ]),
            Tone::Neutral,
        )
    } else if let Some(err) = &app.fsck.error {
        (
            Line::from(vec![
                Tag::solid("failed", Tone::Error).span(),
                Span::styled(format!(" {err}"), Style::default().fg(th::ERROR)),
            ]),
            Tone::Error,
        )
    } else {
        (
            Line::from(vec![
                Tag::new("idle", Tone::Neutral).span(),
                Span::styled(
                    " press enter to scan · f targets the browsed directory",
                    Style::default().fg(th::MUTED),
                ),
            ]),
            Tone::Neutral,
        )
    };
    let _ = tone;

    f.render_widget(
        Paragraph::new(line).block(components::panel_block(&format!("fsck {}", app.fsck_path))),
        area,
    );
}

fn render_summary(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    match &app.fsck.data {
        Some(s) => {
            let replica_tone = if s.expected_replicas == 0 {
                Tone::Neutral
            } else if s.available_replicas == s.expected_replicas {
                Tone::Ok
            } else if s.available_replicas == 0 {
                Tone::Error
            } else {
                Tone::Warn
            };
            lines.push(kv_line(
                "files",
                format!("{} files", s.file_count),
                Tone::Info,
                18,
            ));
            lines.push(kv_line(
                "blocks",
                format!("{} block{}", s.block_count, if s.block_count == 1 { "" } else { "s" }),
                Tone::Neutral,
                18,
            ));
            lines.push(kv_line(
                "replicas",
                format!(
                    "{}/{} available · {} recorded · {} unavailable",
                    s.available_replicas,
                    s.expected_replicas,
                    s.recorded_replicas,
                    s.unavailable_replicas
                ),
                replica_tone,
                18,
            ));
            lines.push(kv_line(
                "under-replicated",
                format!(
                    "{} block{}",
                    s.under_replicated_blocks,
                    if s.under_replicated_blocks == 1 { "" } else { "s" }
                ),
                if s.under_replicated_blocks == 0 {
                    Tone::Ok
                } else {
                    Tone::Warn
                },
                18,
            ));
            lines.push(kv_line(
                "empty",
                format!(
                    "{} block{}",
                    s.missing_replica_blocks,
                    if s.missing_replica_blocks == 1 { "" } else { "s" }
                ),
                if s.missing_replica_blocks == 0 {
                    Tone::Neutral
                } else {
                    Tone::Error
                },
                18,
            ));
            lines.push(kv_line(
                "errors",
                format!(
                    "{} scan error{}",
                    s.errors.len(),
                    if s.errors.len() == 1 { "" } else { "s" }
                ),
                if s.errors.is_empty() {
                    Tone::Neutral
                } else {
                    Tone::Error
                },
                18,
            ));
        }
        None => lines.push(Line::from(Span::styled(
            "  run a scan to populate this panel",
            Style::default().fg(th::MUTED),
        ))),
    }

    f.render_widget(
        Paragraph::new(lines).block(components::panel_block("summary")),
        area,
    );
}

fn render_bad_files(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("path"),
        Cell::from("size"),
        Cell::from("blocks"),
        Cell::from("replicas"),
        Cell::from("issue"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let (rows, title, tone): (Vec<Row>, String, Tone) = match &app.fsck.data {
        Some(s) if !s.bad_files.is_empty() => (
            s.bad_files
                .iter()
                .map(|f| {
                    let (issue_span, tone) = match &f.error {
                        Some(err) => (err.clone(), Tone::Error),
                        None => (
                            format!("{} of {} replicas available", f.available, f.expected),
                            Tone::Warn,
                        ),
                    };
                    Row::new(vec![
                        Cell::from(f.path.clone()).style(Style::default().fg(th::TEXT)),
                        Cell::from(if f.len > 0 {
                            human_bytes(f.len)
                        } else {
                            "—".to_string()
                        }),
                        Cell::from(if f.blocks > 0 {
                            f.blocks.to_string()
                        } else {
                            "—".to_string()
                        }),
                        Cell::from(if f.expected > 0 {
                            components::dot_label(tone, format!("{}/{}", f.available, f.expected))
                        } else {
                            components::dot_label(Tone::Neutral, "n/a")
                        }),
                        Cell::from(issue_span).style(Style::default().fg(tone.color())),
                    ])
                })
                .collect(),
            format!("unhealthy files — {}", s.bad_files.len()),
            Tone::Error,
        ),
        Some(_) => (
            Vec::new(),
            "unhealthy files — none, every replica is available".to_string(),
            Tone::Ok,
        ),
        None => (Vec::new(), "unhealthy files".to_string(), Tone::Neutral),
    };

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.fsck_selected.min(rows.len() - 1)));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(44),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(components::panel_block_badged(
        &title,
        Tag::new(
            match tone {
                Tone::Ok => "clean",
                Tone::Error => "action needed",
                _ => "—",
            },
            tone,
        ),
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
