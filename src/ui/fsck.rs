//! Replica-health panel. Shows an aggregate verdict plus only the unhealthy
//! files, since a clean tree would otherwise be thousands of identical rows.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{components, human_bytes, theme as th};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(7), Constraint::Min(0)])
        .split(area);

    render_verdict(f, app, rows[0]);
    render_stats(f, app, rows[1]);
    render_bad_files(f, app, rows[2]);
}

fn render_verdict(f: &mut Frame, app: &App, area: Rect) {
    if let (Some(s), false) = (&app.fsck.data, app.fsck.loading) {
        // A Gauge centres its label over the bar, so keep this to the headline
        // and let `summary` carry the breakdown.
        let (text, color) = if !s.warnings {
            ("healthy".to_string(), th::OK)
        } else {
            ("degraded".to_string(), th::ERROR)
        };
        let detail = format!(
            " {text} — {:.0}% of {:.0} replicas available ",
            s.health_ratio * 100.0,
            s.expected_replicas
        );

        let g = ratatui::widgets::Gauge::default()
            .block(components::panel_block(&format!("fsck {}", s.root)))
            .gauge_style(Style::default().fg(th::usage_color(1.0 - s.health_ratio)))
            .ratio(s.health_ratio)
            .label(Span::styled(detail, Style::default().fg(color).add_modifier(Modifier::BOLD)));
        f.render_widget(g, area);
        return;
    }

    let msg = if app.fsck.loading {
        format!(
            "{} scanning {} … ({} to cancel)",
            components::spinner(0),
            app.fsck_path,
            "x"
        )
    } else if let Some(err) = &app.fsck.error {
        err.clone()
    } else {
        "press enter to scan, f to target the browsed directory".to_string()
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(if app.fsck.error.is_some() {
                th::ERROR
            } else {
                th::MUTED
            }),
        )))
        .block(components::panel_block(&format!("fsck {}", app.fsck_path))),
        area,
    );
}

fn render_stats(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    match &app.fsck.data {
        Some(s) => {
            let kv = |k: &str, v: String| {
                Line::from(vec![
                    Span::styled(format!("  {k:<20}"), Style::default().fg(th::MUTED)),
                    Span::styled(v, Style::default().fg(th::TEXT)),
                ])
            };
            lines.push(kv("scanned", format!("{} ms", s.elapsed_ms)));
            lines.push(kv(
                "files / blocks",
                format!("{} / {}", s.file_count, s.block_count),
            ));
            lines.push(kv(
                "replicas recorded",
                format!("{} (expected {})", s.recorded_replicas, s.expected_replicas),
            ));
            lines.push(kv(
                "replicas available",
                format!(
                    "{} ({} unavailable)",
                    s.available_replicas, s.unavailable_replicas
                ),
            ));
            let issues = format!(
                "{} under-replicated, {} with no replica, {} scan errors",
                s.under_replicated_blocks,
                s.missing_replica_blocks,
                s.errors.len()
            );
            lines.push(Line::from(vec![
                Span::styled("  issues              ", Style::default().fg(th::MUTED)),
                Span::styled(
                    issues,
                    Style::default().fg(if s.warnings { th::WARN } else { th::OK }),
                ),
            ]));
        }
        None => lines.push(Line::from(Span::styled(
            "  no report yet",
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

    let (rows, title): (Vec<Row>, String) = match &app.fsck.data {
        Some(s) if !s.bad_files.is_empty() => (
            s.bad_files
                .iter()
                .map(|f| {
                    let issue = f
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("{} of {} replicas", f.available, f.expected));
                    Row::new(vec![
                        Cell::from(f.path.clone()).style(Style::default().fg(th::TEXT)),
                        Cell::from(human_bytes(f.len)),
                        Cell::from(f.blocks.to_string()),
                        Cell::from(format!("{}/{}", f.available, f.expected))
                            .style(Style::default().fg(th::ERROR)),
                        Cell::from(issue).style(Style::default().fg(th::WARN)),
                    ])
                })
                .collect(),
            format!("unhealthy files ({})", s.bad_files.len()),
        ),
        Some(_) => (
            Vec::new(),
            "unhealthy files — none, every replica is available".to_string(),
        ),
        None => (Vec::new(), "unhealthy files".to_string()),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(46),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(components::panel_block(&title))
    .column_spacing(1);

    f.render_widget(table, area);
}
