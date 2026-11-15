//! Node browser. Reuses the cluster snapshot but presents per-worker detail.

use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Row, Table},
    Frame,
};

use super::{components, human_bytes, ratio, theme as th};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("address"),
        Cell::from("status"),
        Cell::from("version"),
        Cell::from("weight"),
        Cell::from("used"),
        Cell::from("available"),
        Cell::from("capacity"),
        Cell::from("use%"),
        Cell::from("blocks"),
    ])
    .style(
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
                    Row::new(vec![
                        Cell::from(w.address.clone()).style(Style::default().fg(th::DIR)),
                        Cell::from(w.status.clone()).style(Style::default().fg(th::OK)),
                        Cell::from(w.version.clone()),
                        Cell::from(w.weight.to_string()),
                        Cell::from(human_bytes(w.fs_used)),
                        Cell::from(human_bytes(w.available)),
                        Cell::from(human_bytes(w.capacity)),
                        Cell::from(format!("{:.0}%", r * 100.0))
                            .style(Style::default().fg(th::usage_color(r))),
                        Cell::from(w.block_num.to_string()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let title = match (&app.cluster.data, app.cluster.loading) {
        (Some(c), _) => format!("nodes — {} live", c.workers.len()),
        (None, true) => "nodes — loading…".to_string(),
        (None, false) => format!(
            "nodes — {}",
            app.cluster.error.as_deref().unwrap_or("no data")
        ),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Min(8),
        ],
    )
    .header(header)
    .block(components::panel_block(&title))
    .column_spacing(1);

    f.render_widget(table, area);
}
