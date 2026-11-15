//! Benchmark panel: knob bar, run state, and results with relative magnitude bars.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{
    components::{self, bar, Tag, Tone},
    theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // knobs + state
            Constraint::Min(4),    // results
        ])
        .split(area);

    render_config(f, app, rows[0]);
    render_results(f, app, rows[1]);
}

fn render_config(f: &mut Frame, app: &App, area: Rect) {
    let o = &app.bench_opts;
    let profile = match o.profile {
        curvine_bench::BenchProfile::Basic => "basic",
        curvine_bench::BenchProfile::Deep => "deep",
    };
    let duration = o
        .duration_ms
        .map(|d| format!("{}s", d / 1000))
        .unwrap_or_else(|| "∞".to_string());

    let state = if let Some(started) = app.bench_running {
        Tag::solid(
            format!(" {} running {}s ", components::live_spinner(), started.elapsed().as_secs()),
            Tone::Accent,
        )
    } else if app.bench.error.is_some() {
        Tag::solid("failed", Tone::Error)
    } else if app.bench.data.is_some() {
        Tag::solid("done", Tone::Ok)
    } else {
        Tag::new("idle", Tone::Neutral)
    };

    let spans = vec![
        Span::raw(" "),
        Span::styled("profile ", Style::default().fg(th::MUTED)),
        Tag::new(profile, Tone::Info).span(),
        Span::raw(" "),
        Span::styled("threads ", Style::default().fg(th::MUTED)),
        Tag::new(o.threads.to_string(), Tone::Neutral).span(),
        Span::raw(" "),
        Span::styled("duration ", Style::default().fg(th::MUTED)),
        Tag::new(duration, Tone::Neutral).span(),
        Span::raw("  "),
        state.span(),
    ];

    f.render_widget(
        Paragraph::new(Line::from(spans))
            .block(components::panel_block(&format!("bench — {}", o.base_path))),
        area,
    );
}

fn render_results(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("op"),
        Cell::from("metric"),
        Cell::from("value"),
        Cell::from("mag"),
        Cell::from("p50"),
        Cell::from("p95"),
        Cell::from("p99"),
        Cell::from("errors"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    // Normalise every metric against the largest value in the run so the bars
    // convey relative magnitude (ops/s and MiB/s cannot share an axis).
    let max_value = app
        .bench
        .data
        .as_ref()
        .map(|s| s.rows.iter().map(|r| r.value).fold(0.0_f64, f64::max))
        .unwrap_or(0.0);

    let rows: Vec<Row> = app
        .bench
        .data
        .as_ref()
        .map(|s| {
            s.rows
                .iter()
                .map(|r| {
                    let rel = if max_value > 0.0 {
                        (r.value / max_value).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let err_tag = if r.errors > 0 {
                        Tag::new(r.errors.to_string(), Tone::Error)
                    } else {
                        Tag::new("0", Tone::Neutral)
                    };
                    Row::new(vec![
                        Cell::from(r.op.clone()).style(Style::default().fg(th::DIR)),
                        Cell::from(r.item.clone()),
                        Cell::from(format!("{:.2} {}", r.value, r.unit))
                            .style(Style::default().fg(th::TEXT)),
                        Cell::from(bar(rel, 10))
                            .style(Style::default().fg(th::usage_tone(1.0 - rel).color())),
                        Cell::from(format!("{:.3}", r.p50)),
                        Cell::from(format!("{:.3}", r.p95)),
                        Cell::from(format!("{:.3}", r.p99)),
                        Cell::from(err_tag.cell()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.bench.data, app.bench_running.is_some()) {
        (Some(s), _) => format!(
            "results — {} metrics, {:.0} ms wall (latency in ms)",
            s.rows.len(),
            s.elapsed_ms
        ),
        (None, true) => format!("results — {} running…", components::live_spinner()),
        (None, false) => format!(
            "results — {}",
            app.bench.error.as_deref().unwrap_or("no run yet")
        ),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Percentage(26),
            Constraint::Length(18),
            Constraint::Length(11),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Min(8),
        ],
    )
    .header(header)
    .block(components::panel_block(&title))
    .column_spacing(1);

    f.render_widget(table, area);
}
