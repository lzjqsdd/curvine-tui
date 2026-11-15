//! Mount table, job list and runtime config views.

use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
    Frame,
};

use super::{components, theme as th};
use crate::app::App;

pub fn render_mounts(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("cv path"),
        Cell::from("ufs path"),
        Cell::from("id"),
        Cell::from("storage"),
        Cell::from("write"),
        Cell::from("provider"),
        Cell::from("ttl(ms)"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .mounts
        .data
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|m| {
                    Row::new(vec![
                        Cell::from(m.cv_path.clone()).style(Style::default().fg(th::DIR)),
                        Cell::from(m.ufs_path.clone()),
                        Cell::from(m.mount_id.to_string()),
                        Cell::from(m.storage_type.clone()),
                        Cell::from(m.write_type.clone()),
                        Cell::from(m.provider.clone()),
                        Cell::from(m.ttl_ms.to_string()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.mounts.data, app.mounts.loading) {
        (Some(l), _) => format!("mounts — {} entries", l.len()),
        (None, true) => "mounts — loading…".to_string(),
        (None, false) => format!(
            "mounts — {}",
            app.mounts.error.as_deref().unwrap_or("no data")
        ),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(24),
            Constraint::Percentage(32),
            Constraint::Length(6),
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

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("job id"),
        Cell::from("state"),
        Cell::from("progress"),
        Cell::from("files"),
        Cell::from("source"),
        Cell::from("target"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .jobs
        .data
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|j| {
                    let pct = j.progress * 100.0;
                    let state_style = match j.state.as_str() {
                        s if s.contains("Complete") => Style::default().fg(th::OK),
                        s if s.contains("Fail") => Style::default().fg(th::ERROR),
                        s if s.contains("Run") => Style::default().fg(th::ACCENT),
                        _ => Style::default().fg(th::MUTED),
                    };
                    Row::new(vec![
                        Cell::from(short_id(&j.job_id)),
                        Cell::from(j.state.clone()).style(state_style),
                        Cell::from(format!("{pct:.0}%")),
                        Cell::from(format!("{}/{}", j.completed, j.total_files)),
                        Cell::from(j.source.clone()),
                        Cell::from(j.target.clone()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.jobs.data, app.jobs.loading) {
        (Some(l), _) => format!("jobs — {} entries", l.len()),
        (None, true) => "jobs — loading…".to_string(),
        (None, false) => format!("jobs — {}", app.jobs.error.as_deref().unwrap_or("no data")),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Percentage(28),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(components::panel_block(&title))
    .column_spacing(1);

    f.render_widget(table, area);
}

pub fn render_config(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![Cell::from("key"), Cell::from("value")]).style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .config
        .data
        .as_ref()
        .map(|list| {
            list.iter()
                .map(|c| {
                    Row::new(vec![
                        Cell::from(c.key.clone()).style(Style::default().fg(th::DIR)),
                        Cell::from(c.value.clone()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.config.data, app.config.loading) {
        (Some(l), _) => format!("runtime config — {} keys", l.len()),
        (None, true) => "runtime config — loading…".to_string(),
        (None, false) => format!(
            "runtime config — {}",
            app.config.error.as_deref().unwrap_or("no data")
        ),
    };

    let table = Table::new(
        rows,
        [Constraint::Percentage(40), Constraint::Min(30)],
    )
    .header(header)
    .block(components::panel_block(&title))
    .column_spacing(1);

    f.render_widget(table, area);
}

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..12])
    } else {
        id.to_string()
    }
}
