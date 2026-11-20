//! File browser: breadcrumb header, iconised listing with replica counts, and
//! a detail strip for the highlighted entry.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use super::{
    components::{self, bar, Tone},
    format_mode, format_mtime, human_bytes,
    icons,
    theme as th,
};
use crate::app::App;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // breadcrumb: path + inline counters
            Constraint::Min(4),    // listing
            Constraint::Length(1), // selected entry: badge + plain metrics
        ])
        .split(area);

    render_breadcrumb(f, app, chunks[0]);
    render_table(f, app, chunks[1]);
    render_detail(f, app, chunks[2]);
}

fn render_breadcrumb(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    // Status badges on the right, path on the left; both share the single row
    // of the band, and `badges_end` leaves a gap between the groups.
    let mut right: Vec<(String, Tone)> = Vec::new();
    if !app.files.filter.is_empty() {
        right.push((format!("filter {}", app.files.filter), Tone::Warn));
    }
    if !app.files.show_hidden {
        right.push(("hidden off".to_string(), Tone::Neutral));
    }
    let visible = app.files.visible().len();
    let total = app.files.entries.len();
    right.push((format!("{visible}/{total}"), Tone::Neutral));
    if !app.files.marked.is_empty() {
        right.push((
            format!("{} marked", app.files.marked.len()),
            Tone::Warn,
        ));
    }
    let right_w = components::pills_width(&right);
    components::badges_end(f, area, &right);

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            format!("{} ", icons::DIR_OPEN),
            Style::default().fg(th::ACCENT),
        ),
        Span::styled(
            app.files.cwd.clone(),
            Style::default()
                .fg(th::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if app.files.load.loading {
        spans.push(Span::styled(
            format!(" {}", components::live_spinner()),
            Style::default().fg(th::ACCENT),
        ));
    } else if app.files.load.error.is_some() {
        spans.push(Span::styled(" !", Style::default().fg(th::ERROR)));
    }

    let width = area
        .width
        .saturating_sub(right_w + if right_w > 0 { 2 } else { 0 });
    f.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            y: area.y + area.height.saturating_sub(1) / 2,
            width,
            height: 1,
            ..area
        },
    );
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    // Icon + name always; owner/modified appear as space allows; replicas are
    // the point of the panel so they survive the longest.
    let wide = area.width >= 92;
    let medium = area.width >= 74;

    let mut header_cells = vec![Cell::from("  name")];
    if wide {
        header_cells.push(Cell::from("mode"));
        header_cells.push(Cell::from("owner"));
    }
    header_cells.push(Cell::from("size"));
    if wide {
        header_cells.push(Cell::from("modified"));
    }
    header_cells.push(Cell::from("repl"));

    let header = Row::new(header_cells).style(
        Style::default()
            .fg(th::ACCENT_DIM)
            .add_modifier(Modifier::BOLD),
    );

    let visible = app.files.visible();
    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(idx, e)| {
            let (glyph, name_style) = if e.is_dir {
                (icons::DIR, Style::default().fg(th::DIR).add_modifier(Modifier::BOLD))
            } else {
                (
                    icons::for_file(&e.name),
                    Style::default().fg(th::TEXT),
                )
            };

            let path = crate::app::join_path(&app.files.cwd, &e.name);
            // Two fixed cells: the first is the cursor, the second the mark.
            // A row can be both selected and marked, so `▶` must not be
            // replaced by `●`; that would hide where the cursor is.
            let selected = idx == app.files.selected;
            let marked = app.files.is_marked(&path);
            let cursor = if selected {
                Span::styled("▶", Style::default().fg(th::ACCENT))
            } else {
                Span::raw(" ")
            };
            let mark = if marked {
                Span::styled("●", Style::default().fg(th::WARN))
            } else {
                Span::raw(" ")
            };

            let name_cell = Cell::from(Line::from(vec![
                cursor,
                mark,
                Span::raw(" "),
                Span::styled(format!("{glyph} "), Style::default().fg(if e.is_dir {
                    th::DIR
                } else {
                    th::MUTED
                })),
                Span::styled(
                    if e.is_dir {
                        format!("{}/", e.name)
                    } else {
                        e.name.clone()
                    },
                    name_style,
                ),
            ]));

            let mut cells = vec![name_cell];
            if wide {
                cells.push(
                    Cell::from(format_mode(e.mode, e.is_dir))
                        .style(Style::default().fg(th::MUTED)),
                );
                cells.push(
                    Cell::from(crate::ui::owner_dash(&e.owner))
                        .style(Style::default().fg(th::MUTED)),
                );
            }
            cells.push(
                Cell::from(if e.is_dir {
                    "—".to_string()
                } else {
                    human_bytes(e.len)
                })
                .style(Style::default().fg(th::TEXT)),
            );
            if wide {
                cells.push(Cell::from(format_mtime(e.mtime)));
            }
            cells.push(replica_cell(e.replicas, e.is_dir));

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

    let mut widths = vec![Constraint::Min(22)];
    if wide {
        widths.push(Constraint::Length(10));
        widths.push(Constraint::Length(8));
    }
    widths.push(Constraint::Length(10));
    if wide {
        widths.push(Constraint::Length(16));
    }
    widths.push(Constraint::Length(6));
    let _ = medium;

    if rows.is_empty() {
        // An empty listing is a state, not a blank table: say why it is empty.
        let block = components::panel_block(&title);
        let inner = block.inner(area);
        f.render_widget(block, area);
        if !app.files.load.loading && inner.height > 0 {
            let msg = if app.files.filter.trim().is_empty() {
                "empty directory"
            } else {
                "no matches"
            };
            f.render_widget(
                Paragraph::new(Span::styled(msg, Style::default().fg(th::MUTED)))
                    .alignment(Alignment::Center),
                Rect {
                    y: inner.y + inner.height / 2,
                    height: 1,
                    ..inner
                },
            );
        }
        return;
    }

    let mut state = TableState::default();
    if !rows.is_empty() {
        state.select(Some(app.files.selected.min(rows.len() - 1)));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(components::panel_block(&title))
        .row_highlight_style(Style::default().bg(th::SELECTED_BG))
        // The cursor/mark glyph lives in the name cell so a marked row can
        // show ● instead of ▶ without losing its highlight.
        .highlight_symbol("")
        .column_spacing(1);

    f.render_stateful_widget(table, area, &mut state);
}

/// Replica factor for the entry.
///
/// `FileStatus::replicas` is the replication factor a file was created with, not
/// the number of live copies — so it is drawn with a neutral dot. Whether those
/// copies actually exist is a separate question, answered by the Fsck panel,
/// which compares against block locations.
fn replica_cell(replicas: i32, is_dir: bool) -> Cell<'static> {
    if is_dir || replicas <= 0 {
        return Cell::from(Span::styled(
            format!("{} —", components::DOT_HOLLOW),
            Style::default().fg(th::MUTED),
        ));
    }
    Cell::from(components::dot_count(Tone::Info, replicas))
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let Some(e) = app.files.selected_entry() else {
        f.render_widget(
            Paragraph::new(Span::styled(
                " no entry selected",
                Style::default().fg(th::MUTED),
            )),
            Rect {
                y: area.y + area.height.saturating_sub(1) / 2,
                height: 1,
                ..area
            },
        );
        return;
    };

    // The type is an inline badge; the rest of the row is plain text.
    let kind = if e.is_dir { "directory" } else { "file" };
    let kind_tone = if e.is_dir { Tone::Info } else { Tone::Neutral };
    let badge_w = components::pill_width(kind).min(area.width.saturating_sub(1));
    let row_y = area.y + area.height.saturating_sub(1) / 2;
    f.render_widget(
        Paragraph::new(components::badge(kind, kind_tone)),
        Rect {
            x: area.x + 1,
            y: row_y,
            width: badge_w,
            height: 1,
        },
    );

    let size_ratio = if e.is_dir || e.len == 0 {
        0.0
    } else {
        (e.len as f64 / (1024.0 * 1024.0 * 1024.0)).clamp(0.02, 1.0)
    };
    let mut spans = vec![
        Span::styled(
            format!(
                "{} ",
                if e.is_dir {
                    icons::DIR
                } else {
                    icons::for_file(&e.name)
                }
            ),
            Style::default().fg(if e.is_dir { th::DIR } else { th::MUTED }),
        ),
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
        crate::ui::owner_group(&e.owner, &e.group),
        Style::default().fg(th::MUTED),
    ));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        format_mtime(e.mtime),
        Style::default().fg(th::MUTED),
    ));
    if e.replicas > 0 && !e.is_dir {
        spans.push(Span::raw("  "));
        spans.push(components::dot(Tone::Info));
        spans.push(Span::styled(
            format!(" {} replicas (configured)", e.replicas),
            Style::default().fg(th::MUTED),
        ));
    }
    let hint = "x menu ".to_string();
    let hint_w = hint.chars().count() as u16;
    let details_x = area.x + 1 + badge_w + 1;
    let avail = area
        .right()
        .saturating_sub(details_x)
        .saturating_sub(hint_w + 1);

    f.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            x: details_x,
            y: row_y,
            width: avail,
            height: 1,
        },
    );
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(th::BORDER))),
        Rect {
            x: area.right().saturating_sub(hint_w),
            y: row_y,
            width: hint_w,
            height: 1,
        },
    );
}
