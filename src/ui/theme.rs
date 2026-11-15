//! Colour palette. Kept in one place so the whole app re-skins at once.

use ratatui::style::Color;

pub const ACCENT: Color = Color::Rgb(94, 234, 212); // teal
pub const ACCENT_DIM: Color = Color::Rgb(45, 130, 118);
pub const OK: Color = Color::Rgb(134, 239, 172);
pub const WARN: Color = Color::Rgb(253, 224, 71);
pub const ERROR: Color = Color::Rgb(248, 113, 113);
pub const MUTED: Color = Color::Rgb(120, 128, 140);
pub const BORDER: Color = Color::Rgb(70, 78, 92);
pub const TEXT: Color = Color::Rgb(226, 232, 240);
pub const DIR: Color = Color::Rgb(125, 211, 252);
pub const SELECTED_BG: Color = Color::Rgb(38, 46, 58);

/// Colour for a usage bar based on how full it is.
pub fn usage_color(ratio: f64) -> Color {
    if ratio >= 0.9 {
        ERROR
    } else if ratio >= 0.75 {
        WARN
    } else {
        OK
    }
}
