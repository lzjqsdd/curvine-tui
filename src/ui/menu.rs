//! Contextual action menu (`x`), the discoverable alternative to memorising
//! panel keys. The menu owns no behaviour: it renders items the app built and
//! reports the chosen action back to the reducer.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};

use super::{components, theme as th};

/// Virtual key codes for actions that have no printable character of their own.
pub const KEY_ENTER: char = '\r';
pub const KEY_DELETE: char = '\u{7f}';

/// One executable menu entry.
#[derive(Debug, Clone)]
pub struct MenuItem {
    /// The key that executes this item directly while the menu is open.
    pub key: char,
    pub label: String,
    pub action: MenuAction,
}

/// All actions the menu can dispatch. Execution lives in `app.rs` so panels
/// stay presentational.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    FileDetail,
    FileEnter,
    FileDelete { recursive: bool },
    FileFree,
    FileUpload,
    FileDownload,
    FileClearMarks,
    NodeDetail,
    MountNew,
    MountUmount,
    JobCancel,
    TransferCancel,
    TransferClear,
    FsckScan,
    FsckAbort,
    FsckTargetCwd,
    BenchRun,
    BenchProfile,
    BenchThreads,
    BenchDuration,
    Refresh,
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub selected: usize,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<MenuItem>) -> Option<Self> {
        if items.is_empty() {
            return None;
        }
        Some(Self {
            title: title.into(),
            items,
            selected: 0,
        })
    }

    pub fn move_down(&mut self) {
        self.selected = (self.selected + 1).min(self.items.len().saturating_sub(1));
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_item(&self) -> Option<&MenuItem> {
        self.items.get(self.selected)
    }

    /// Find the item bound to a key, for direct execution.
    pub fn item_for_key(&self, key: char) -> Option<&MenuItem> {
        self.items.iter().find(|i| i.key == key)
    }
}

/// Human label for a virtual key. Every other key is its own label.
pub fn key_label(key: char) -> String {
    match key {
        KEY_ENTER => "Enter".to_string(),
        KEY_DELETE => "Del".to_string(),
        c => c.to_string(),
    }
}

/// Render the menu centred over `area`, sized to its content.
pub fn render(f: &mut Frame, area: Rect, menu: &Menu) {
    let key_w = menu
        .items
        .iter()
        .map(|i| key_label(i.key).chars().count())
        .max()
        .unwrap_or(1);
    let label_w = menu
        .items
        .iter()
        .map(|i| i.label.chars().count())
        .max()
        .unwrap_or(0);
    let width = (key_w + label_w + 8).max(menu.title.chars().count() + 6) as u16;
    let height = (menu.items.len() + 2) as u16;
    let rect = components::centre(
        width.min(area.width.saturating_sub(2)).max(20),
        height.min(area.height.saturating_sub(2)).max(3),
        area,
    );
    f.render_widget(Clear, rect);

    let mut lines: Vec<Line> = Vec::new();
    for (idx, item) in menu.items.iter().enumerate() {
        let selected = idx == menu.selected;
        let row_style = if selected {
            Style::default()
                .bg(th::SELECTED_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(" ", row_style),
            Span::styled(
                format!("{:<key_w$}", key_label(item.key), key_w = key_w),
                row_style.fg(th::ACCENT),
            ),
            Span::styled("  ", row_style),
            Span::styled(item.label.clone(), row_style.fg(
                if selected { th::TEXT } else { th::MUTED },
            )),
            Span::styled(" ", row_style),
        ]));
    }

    let block = components::panel_block(&menu.title);
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Left),
        rect,
    );
}
