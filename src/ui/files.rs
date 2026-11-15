//! File browser: breadcrumb header, filtered listing, and a detail strip.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{
    components::{self, kv_line, bar, Tag, Tone},
    format_mode, format_mtime, human_bytes, theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // breadcrumb
            Constraint::Min(4),    // listing
            Constraint::Length(3), // selected entry detail
        ])
        .split(area);

    render_breadcrumb(f, app, chunks[0]);
    render_table(f, app, chunks[1]);
    render_detail(f, app, chunks[2]);
}

fn render_breadcrumb(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
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

    let mut right: Vec<Span> = Vec::new();
    if !app.files.filter.is_empty() {
        right.push(Tag::new(format!("filter {}", app.files.filter), Tone::Warn).span());
        right.push(Span::raw(" "));
    }
    if !app.files.show_hidden {
        right.push(Tag::new("hidden off", Tone::Neutral).span());
        right.push(Span::raw(" "));
    }
    let count = app.files.visible().len();
    right.push(Tag::new(format!("{count} entries"), Tone::Info).span());
    right.push(Span::raw(" "));

    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + right_w + 1);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(right);

    let title = if app.files.load.loading {
        format!("path {}", components::live_spinner())
    } else if app.files.load.error.is_some() {
        "path — error".to_string()
    } else {
        "path".to_string()
    };

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(components::panel_block(&title)),
        area,
    );
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 88;

    let mut header_cells = vec![
        Cell::from("mode"),
        Cell::from("owner"),
        Cell::from("size"),
    ];
    if wide {
        header_cells.push(Cell::from("modified"));
    }
    header_cells.push(Cell::from("name"));
    if wide {
        header_cells.push(Cell::from("type"));
    }

    let header = Row::new(header_cells).style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let visible = app.files.visible();
    let rows: Vec<Row> = visible
        .iter()
        .map(|e| {
            let name_style = if e.is_dir {
                Style::default().fg(th::DIR).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th::TEXT)
            };
            let mut cells = vec![
                Cell::from(format_mode(e.mode, e.is_dir)).style(Style::default().fg(th::MUTED)),
                Cell::from(e.owner.clone()).style(Style::default().fg(th::MUTED)),
                Cell::from(if e.is_dir {
                    "—".to_string()
                } else {
                    human_bytes(e.len)
                })
                .style(Style::default().fg(th::TEXT)),
            ];
            if wide {
                cells.push(Cell::from(format_mtime(e.mtime)));
            }
            cells.push(
                Cell::from(if e.is_dir {
                    format!("{}/", e.name)
                } else {
                    e.name.clone()
                })
                .style(name_style),
            );
            if wide {
                cells.push(Cell::from(if e.is_dir {
                    Tag::new("dir", Tone::Info).cell()
                } else {
                    Tag::new("file", Tone::Neutral).cell()
                }));
            }
            Row::new(cells)
        })
        .collect();

    let title = if let Some(err) = &app.files.load.error {
        format!("list — {err}")
    } else if app.files.load.loading {
        format!("list {} {}", app.files.cwd, components::live_spinner())
    } else {
        format!("list {}", app.files.cwd)
    };

    let mut widths = vec![
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(10),
    ];
    if wide {
        widths.push(Constraint::Length(16));
    }
    widths.push(Constraint::Min(18));
    if wide {
        widths.push(Constraint::Length(6));
    }

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.files.selected.min(rows.len() - 1)));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(components::panel_block(&title))
        .row_highlight_style(Style::default().bg(th::SELECTED_BG))
        .highlight_symbol("▶ ")
        .column_spacing(1);

    f.render_stateful_widget(table, area, &mut state);
}

/// Detail strip for the highlighted entry: mode bits, ownership, size bar.
fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let line = match app.files.selected_entry() {
        Some(e) => {
            let size_ratio = if e.is_dir || e.len == 0 {
                0.0
            } else {
                // Relative to 1 GiB, purely as a visual magnitude cue.
                (e.len as f64 / (1024.0 * 1024.0 * 1024.0)).clamp(0.02, 1.0)
            };
            let mut spans = vec![
                Span::raw(" "),
                Tag::new(
                    if e.is_dir { "directory" } else { "file" },
                    if e.is_dir { Tone::Info } else { Tone::Neutral },
                )
                .span(),
                Span::raw(" "),
                Span::styled(
                    format_mode(e.mode, e.is_dir),
                    Style::default().fg(th::ACCENT),
                ),
                Span::raw("  "),
            ];

            if !e.is_dir {
                spans.push(Span::styled(
                    bar(size_ratio, 10),
                    Style::default().fg(th::usage_tone(size_ratio).color()),
                ));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    human_bytes(e.len),
                    Style::default().fg(th::TEXT),
                ));
                spans.push(Span::raw("  "));
            }

            spans.push(Span::styled(
                format!("{}:{}", e.owner, e.group),
                Style::default().fg(th::MUTED),
            ));
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format_mtime(e.mtime),
                Style::default().fg(th::MUTED),
            ));
            if e.replicas > 0 && !e.is_dir {
                spans.push(Span::raw(" "));
                spans.push(Tag::new(format!("{} replicas", e.replicas), Tone::Accent).span());
            }
            Line::from(spans)
        }
        None => Line::from(Span::styled(
            " no entry selected",
            Style::default().fg(th::MUTED),
        )),
    };

    f.render_widget(
        Paragraph::new(line).block(components::panel_block("selected")),
        area,
    );
}

/// Kept for parity with other panels that show free space.
#[allow(dead_code)]
fn _kv(label: &str, value: &str) -> Line<'static> {
    kv_line(label, value, Tone::Neutral, 12)
}
