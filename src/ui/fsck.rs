//! Replica-health panel: verdict gauge, stat badges, unhealthy-file table.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{
    components::{self, bar_with_pct, kv_line, Tag, Tone},
    human_bytes, theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // verdict gauge
            Constraint::Length(3), // stat badge strip
            Constraint::Length(7), // numeric summary
            Constraint::Min(3),    // unhealthy files
        ])
        .split(area);

    render_verdict(f, app, rows[0]);
    render_strip(f, app, rows[1]);
    render_summary(f, app, rows[2]);
    render_bad_files(f, app, rows[3]);
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

fn render_strip(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ")];

    match &app.fsck.data {
        Some(s) => {
            // The dot colour is the verdict; the label only names the counter.
            let replica_tone = if s.expected_replicas == 0 {
                Tone::Neutral
            } else if s.available_replicas == s.expected_replicas {
                Tone::Ok
            } else if s.available_replicas == 0 {
                Tone::Error
            } else {
                Tone::Warn
            };
            let counter = |tone: Tone, text: String| -> Vec<Span<'static>> {
                vec![
                    components::dot(tone),
                    Span::raw(" "),
                    Span::styled(text, Style::default().fg(th::TEXT)),
                    Span::raw("   "),
                ]
            };
            spans.extend(counter(Tone::Info, format!("{} files", s.file_count)));
            spans.extend(counter(Tone::Neutral, format!("{} blocks", s.block_count)));
            spans.extend(counter(
                replica_tone,
                format!("{}/{} replicas", s.available_replicas, s.expected_replicas),
            ));
            spans.extend(counter(
                if s.under_replicated_blocks == 0 {
                    Tone::Ok
                } else {
                    Tone::Warn
                },
                format!("{} under-replicated", s.under_replicated_blocks),
            ));
            spans.extend(counter(
                if s.missing_replica_blocks == 0 {
                    Tone::Neutral
                } else {
                    Tone::Error
                },
                format!("{} empty", s.missing_replica_blocks),
            ));
            spans.extend(counter(
                if s.errors.is_empty() {
                    Tone::Neutral
                } else {
                    Tone::Error
                },
                format!("{} errors", s.errors.len()),
            ));
        }
        None => spans.push(Span::styled(
            " no report yet",
            Style::default().fg(th::MUTED),
        )),
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(components::panel_block("verdict")),
        area,
    );
}

fn render_summary(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    match &app.fsck.data {
        Some(s) => {
            lines.push(kv_line("scanned", s.root.clone(), Tone::Info, 20));
            lines.push(kv_line(
                "replicas recorded",
                format!("{} (expected {})", s.recorded_replicas, s.expected_replicas),
                Tone::Neutral,
                20,
            ));
            lines.push(kv_line(
                "replicas available",
                format!(
                    "{} ({} unavailable)",
                    s.available_replicas, s.unavailable_replicas
                ),
                if s.unavailable_replicas == 0 {
                    Tone::Ok
                } else {
                    Tone::Warn
                },
                20,
            ));
            let r = s.health_ratio;
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<20}", "health"),
                    Style::default().fg(th::MUTED),
                ),
                Span::styled(
                    bar_with_pct(r, 20),
                    Style::default().fg(th::usage_tone(1.0 - r).color()),
                ),
            ]));
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
    .column_spacing(1);

    f.render_widget(table, area);
}
