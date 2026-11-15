//! Reusable widgets: bordered blocks, usage bars, spinner, help popup.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph},
    Frame,
};

use super::theme as th;

pub fn panel_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th::BORDER))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(th::ACCENT_DIM)
                .add_modifier(Modifier::BOLD),
        ))
}

/// A labelled usage bar rendered as a Gauge.
pub fn render_usage(f: &mut Frame, area: Rect, label: &str, ratio: f64, text: String) {
    let g = Gauge::default()
        .block(panel_block(label))
        .gauge_style(
            Style::default()
                .fg(th::usage_color(ratio))
                .bg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        )
        .ratio(ratio.clamp(0.0, 1.0))
        .label(Span::styled(text, Style::default().fg(th::TEXT)));
    f.render_widget(g, area);
}

use ratatui::style::Color;

/// Braille spinner frame for a given tick counter.
pub fn spinner(frame: usize) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[frame % FRAMES.len()]
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
            ratatui::layout::Constraint::Percentage(percent_y),
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
            ratatui::layout::Constraint::Percentage(percent_x),
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

pub fn render_help_popup(f: &mut Frame, area: Rect) {
    let rect = centered_rect(60, 60, area);
    f.render_widget(Clear, rect);

    let rows: Vec<(&str, &str)> = vec![
        ("tab / shift-tab", "switch panel"),
        ("1 … 6", "jump to panel"),
        ("j / k  ↑ ↓", "move selection (files)"),
        ("enter / l", "enter directory"),
        ("backspace / h", "parent directory"),
        ("/", "filter current listing"),
        (".", "toggle hidden files"),
        ("m", "create directory"),
        ("d / D", "delete / delete recursive"),
        ("r", "refresh panel"),
        (":", "command line (ls, cd, mkdir, rm, mv, load…)"),
        ("?", "toggle this help"),
        ("q / ctrl-c", "quit"),
    ];

    let mut lines = vec![
        Line::from(Span::styled(
            "curvine-tui",
            Style::default()
                .fg(th::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for (k, d) in rows {
        lines.push(Line::from(vec![
            Span::styled(format!("  {k:<18}"), Style::default().fg(th::ACCENT_DIM)),
            Span::styled(d.to_string(), Style::default().fg(th::TEXT)),
        ]));
    }

    let p = Paragraph::new(lines)
        .block(panel_block("Help"))
        .alignment(Alignment::Left);
    f.render_widget(p, rect);
}
