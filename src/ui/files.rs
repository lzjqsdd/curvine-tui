//! File browser with breadcrumb header and a long-format listing.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{components, format_mode, format_mtime, human_bytes, theme as th};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_breadcrumb(f, app, chunks[0]);
    render_table(f, app, chunks[1]);
}

fn render_breadcrumb(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled(" ", Style::default())];
    let parts: Vec<&str> = app.files.cwd.split('/').filter(|s| !s.is_empty()).collect();
    spans.push(Span::styled(
        "/",
        Style::default()
            .fg(th::ACCENT)
            .add_modifier(Modifier::BOLD),
    ));
    for p in &parts {
        spans.push(Span::styled(
            format!("{p}/"),
            Style::default().fg(th::TEXT),
        ));
    }
    if !app.files.filter.is_empty() {
        spans.push(Span::styled(
            format!("  filter={}", app.files.filter),
            Style::default().fg(th::WARN),
        ));
    }

    let count = app.files.visible().len();
    let right = format!("{count} entries ");
    let mut line = spans;
    // Right-align the entry count by padding.
    let used: usize = line.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + right.len() + 1);
    line.push(Span::raw(" ".repeat(pad)));
    line.push(Span::styled(right, Style::default().fg(th::MUTED)));

    f.render_widget(
        Paragraph::new(Line::from(line)).block(components::panel_block("path")),
        area,
    );
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec![
        Cell::from("mode"),
        Cell::from("owner"),
        Cell::from("group"),
        Cell::from("size"),
        Cell::from("modified"),
        Cell::from("name"),
    ])
    .style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let visible = app.files.visible();
    let rows: Vec<Row> = visible
        .iter()
        .map(|e| {
            let name_style = if e.is_dir {
                Style::default().fg(th::DIR)
            } else {
                Style::default().fg(th::TEXT)
            };
            Row::new(vec![
                Cell::from(format_mode(e.mode, e.is_dir)),
                Cell::from(e.owner.clone()),
                Cell::from(e.group.clone()),
                Cell::from(if e.is_dir {
                    "-".to_string()
                } else {
                    human_bytes(e.len)
                }),
                Cell::from(format_mtime(e.mtime)),
                Cell::from(if e.is_dir {
                    format!("{}/", e.name)
                } else {
                    e.name.clone()
                })
                .style(name_style),
            ])
        })
        .collect();

    let title = if app.files.load.loading {
        format!("list {} {}", app.files.cwd, components::spinner(0))
    } else if let Some(err) = &app.files.load.error {
        format!("list {} — {err}", app.files.cwd)
    } else {
        format!("list {}", app.files.cwd)
    };

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.files.selected.min(rows.len() - 1)));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(components::panel_block(&title))
    .row_highlight_style(
        Style::default()
            .bg(th::SELECTED_BG)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ")
    .column_spacing(1);

    f.render_stateful_widget(table, area, &mut state);
}
