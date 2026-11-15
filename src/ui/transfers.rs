//! Transfer panel: per-file progress bars, direction badges, put/get form.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Clear, Gauge, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{
    components::{self, bar_with_pct, input_line, section, Tag, Tone},
    human_bytes, theme as th,
};
use crate::app::{App, TransferForm};
use crate::event::{TransferKind, TransferState};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(3), // current transfer gauge
            Constraint::Length(3), // queue counters
        ])
        .split(area);

    render_table(f, app, chunks[0]);
    render_active_gauge(f, app, chunks[1]);
    render_counters(f, app, chunks[2]);

    if let Some(form) = &app.form {
        render_form(f, form, area);
    }
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 100;

    let mut header_cells = vec![Cell::from("#"), Cell::from("dir")];
    if wide {
        header_cells.push(Cell::from("path"));
    }
    header_cells.push(Cell::from("progress"));
    header_cells.push(Cell::from("size"));
    header_cells.push(Cell::from("speed"));
    header_cells.push(Cell::from("state"));

    let header = Row::new(header_cells).style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .transfers
        .iter()
        .map(|t| {
            let pct = t.ratio();
            let (source, target) = match t.kind {
                TransferKind::Upload => (t.local.clone(), t.remote.clone()),
                TransferKind::Download => (t.remote.clone(), t.local.clone()),
            };
            let (arrow, tone) = match t.kind {
                TransferKind::Upload => ("up", Tone::Warn),
                TransferKind::Download => ("down", Tone::Info),
            };

            let mut cells = vec![
                Cell::from(format!("#{}", t.id)).style(Style::default().fg(th::MUTED)),
                Cell::from(Tag::new(arrow, tone).cell()),
            ];
            if wide {
                cells.push(Cell::from(format!("{source} → {target}")));
            }
            cells.push(
                Cell::from(bar_with_pct(pct, 14))
                    .style(Style::default().fg(th::usage_tone(1.0 - pct).color())),
            );
            cells.push(Cell::from(if t.total > 0 {
                human_bytes(t.total as i64)
            } else {
                "—".to_string()
            }));
            cells.push(Cell::from(if t.speed > 0 {
                format!("{}/s", human_bytes(t.speed as i64))
            } else {
                "—".to_string()
            }));
            cells.push(Cell::from(components::dot_label(
                tone_for_transfer(t.state),
                t.state.label(),
            )));
            Row::new(cells)
        })
        .collect();

    let title = if app.transfers.is_empty() {
        "transfers — press p to upload or g to download".to_string()
    } else {
        let running = app
            .transfers
            .iter()
            .filter(|t| t.state == TransferState::Running)
            .count();
        format!("transfers — {} total, {} running", app.transfers.len(), running)
    };

    let mut widths = vec![Constraint::Length(4), Constraint::Length(5)];
    if wide {
        widths.push(Constraint::Percentage(34));
    }
    widths.push(Constraint::Length(15));
    widths.push(Constraint::Length(10));
    widths.push(Constraint::Length(12));
    widths.push(Constraint::Min(10));

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.transfer_selected.min(rows.len() - 1)));
    }

    let table = Table::new(rows, widths)
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

fn tone_for_transfer(s: TransferState) -> Tone {
    match s {
        TransferState::Done => Tone::Ok,
        TransferState::Failed => Tone::Error,
        TransferState::Cancelled => Tone::Neutral,
        TransferState::Running => Tone::Accent,
        TransferState::Queued => Tone::Warn,
    }
}

fn render_active_gauge(f: &mut Frame, app: &App, area: Rect) {
    let Some(t) = app.transfers.get(app.transfer_selected) else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " no transfer selected",
                Style::default().fg(th::MUTED),
            )))
            .block(components::panel_block("current")),
            area,
        );
        return;
    };

    if t.state.is_terminal() {
        let mut spans = vec![
            Span::raw(" "),
            components::dot(tone_for_transfer(t.state)),
            Span::raw(" "),
            Span::styled(t.state.label(), Style::default().fg(tone_for_transfer(t.state).color())),
            Span::raw(" "),
        ];
        spans.push(Span::styled(
            format!("#{} ", t.id),
            Style::default().fg(th::MUTED),
        ));
        match &t.error {
            Some(err) => spans.push(Span::styled(err.clone(), Style::default().fg(th::ERROR))),
            None => spans.push(Span::styled(
                format!("{} transferred", human_bytes(t.transferred as i64)),
                Style::default().fg(th::TEXT),
            )),
        }
        if t.state == TransferState::Cancelled {
            spans.push(Span::styled(
                "   (cancelled before completion)",
                Style::default().fg(th::MUTED),
            ));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).block(components::panel_block("current")),
            area,
        );
        return;
    }

    let label = format!(
        "{}  {} / {}   {}",
        t.state.label(),
        human_bytes(t.transferred as i64),
        if t.total > 0 {
            human_bytes(t.total as i64)
        } else {
            "?".to_string()
        },
        if t.speed > 0 {
            format!("{}/s", human_bytes(t.speed as i64))
        } else {
            String::new()
        }
    );

    let g = Gauge::default()
        .block(components::panel_block("current"))
        .gauge_style(Style::default().fg(th::ACCENT).add_modifier(Modifier::BOLD))
        .ratio(t.ratio())
        .label(Span::styled(label, Style::default().fg(th::TEXT)));
    f.render_widget(g, area);
}

fn render_counters(f: &mut Frame, app: &App, area: Rect) {
    let count = |s: TransferState| app.transfers.iter().filter(|t| t.state == s).count();
    let bytes_up: u64 = app
        .transfers
        .iter()
        .filter(|t| t.kind == TransferKind::Upload)
        .map(|t| t.transferred)
        .sum();
    let bytes_down: u64 = app
        .transfers
        .iter()
        .filter(|t| t.kind == TransferKind::Download)
        .map(|t| t.transferred)
        .sum();

    let counter = |tone: Tone, n: usize, label: &str| -> Vec<Span<'static>> {
        vec![
            components::dot(if n > 0 { tone } else { Tone::Neutral }),
            Span::raw(" "),
            Span::styled(format!("{n} {label}"), Style::default().fg(th::MUTED)),
            Span::raw("   "),
        ]
    };
    let mut spans = vec![Span::raw(" ")];
    spans.extend(counter(Tone::Accent, count(TransferState::Running), "running"));
    spans.extend(counter(Tone::Warn, count(TransferState::Queued), "queued"));
    spans.extend(counter(Tone::Ok, count(TransferState::Done), "done"));
    spans.extend(counter(Tone::Error, count(TransferState::Failed), "failed"));
    spans.extend(vec![
        Span::raw(" "),
        Span::styled("↑ ", Style::default().fg(th::WARN)),
        Span::styled(
            human_bytes(bytes_up as i64),
            Style::default().fg(th::TEXT),
        ),
        Span::styled("   ↓ ", Style::default().fg(th::INFO)),
        Span::styled(
            human_bytes(bytes_down as i64),
            Style::default().fg(th::TEXT),
        ),
    ]);

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(components::panel_block("queue")),
        area,
    );
}

fn render_form(f: &mut Frame, form: &TransferForm, area: Rect) {
    let rect = components::centered_rect(66, 44, area);
    f.render_widget(Clear, rect);

    let (title, tone) = match form.kind {
        TransferKind::Upload => ("upload  local → curvine", Tone::Warn),
        TransferKind::Download => ("download  curvine → local", Tone::Info),
    };

    let local_label = match form.kind {
        TransferKind::Upload => "local",
        TransferKind::Download => "dest",
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::raw("  "), Tag::solid(
            match form.kind {
                TransferKind::Upload => "PUT",
                TransferKind::Download => "GET",
            },
            tone,
        ).span()]),
        Line::from(""),
        section("paths"),
        input_line(local_label, &form.local, form.focus == 0, false),
        input_line("remote", &form.remote, form.focus == 1, false),
        Line::from(""),
    ];

    if let Some(err) = &form.error {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Tag::solid("error", Tone::Error).span(),
            Span::styled(format!(" {err}"), Style::default().fg(th::ERROR)),
        ]));
    } else {
        let hint = match form.kind {
            TransferKind::Upload => "streams from the local file in write_chunk_size slices",
            TransferKind::Download => "streams to the local file in read_chunk_size slices",
        };
        lines.push(Line::from(Span::styled(
            format!("  {hint}"),
            Style::default().fg(th::MUTED),
        )));
    }

    f.render_widget(
        Paragraph::new(lines)
            .block(components::panel_block(title))
            .alignment(Alignment::Left),
        rect,
    );
}
