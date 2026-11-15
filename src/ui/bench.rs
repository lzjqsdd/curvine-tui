//! Benchmark panel: knob summary, run state, and the results table.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use super::{components, theme as th};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
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
        .unwrap_or_else(|| "unbounded".to_string());

    let state = if let Some(started) = app.bench_running {
        Span::styled(
            format!("{} running {}s", components::spinner(0), started.elapsed().as_secs()),
            Style::default().fg(th::ACCENT),
        )
    } else if app.bench.error.is_some() {
        Span::styled("failed", Style::default().fg(th::ERROR))
    } else if app.bench.data.is_some() {
        Span::styled("done", Style::default().fg(th::OK))
    } else {
        Span::styled("idle — enter to run", Style::default().fg(th::MUTED))
    };

    let line = Line::from(vec![
        Span::styled("  profile ", Style::default().fg(th::MUTED)),
        Span::styled(format!("{profile:<6}"), Style::default().fg(th::TEXT)),
        Span::styled("threads ", Style::default().fg(th::MUTED)),
        Span::styled(format!("{:<3}", o.threads), Style::default().fg(th::TEXT)),
        Span::styled("duration ", Style::default().fg(th::MUTED)),
        Span::styled(format!("{duration:<10}"), Style::default().fg(th::TEXT)),
        state,
    ]);

    f.render_widget(
        Paragraph::new(line).block(components::panel_block(&format!(
            "bench — {}",
            o.base_path
        ))),
        area,
    );
}

fn render_results(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("op"),
        Cell::from("metric"),
        Cell::from("value"),
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

    let rows: Vec<Row> = app
        .bench
        .data
        .as_ref()
        .map(|s| {
            s.rows
                .iter()
                .map(|r| {
                    let err_style = if r.errors > 0 {
                        Style::default().fg(th::ERROR)
                    } else {
                        Style::default().fg(th::MUTED)
                    };
                    Row::new(vec![
                        Cell::from(r.op.clone()).style(Style::default().fg(th::DIR)),
                        Cell::from(r.item.clone()),
                        Cell::from(format!("{:.2} {}", r.value, r.unit))
                            .style(Style::default().fg(th::TEXT)),
                        Cell::from(format!("{:.3} ms", r.p50)),
                        Cell::from(format!("{:.3} ms", r.p95)),
                        Cell::from(format!("{:.3} ms", r.p99)),
                        Cell::from(r.errors.to_string()).style(err_style),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.bench.data, app.bench_running.is_some()) {
        (Some(s), _) => format!(
            "results — {} metrics, {:.0} ms wall",
            s.rows.len(),
            s.elapsed_ms
        ),
        (None, true) => "results — running…".to_string(),
        (None, false) => format!(
            "results — {}",
            app.bench.error.as_deref().unwrap_or("no run yet")
        ),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Percentage(34),
            Constraint::Length(18),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Min(8),
        ],
    )
    .header(header)
    .block(components::panel_block(&title))
    .column_spacing(1);

    f.render_widget(table, area);
}
