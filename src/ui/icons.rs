//! Nerd Font glyphs.
//!
//! Not every glyph in this catalogue is referenced yet; it is kept complete so
//! new panels pick from one list instead of inventing codepoints.
#![allow(dead_code)]
//!
//! Codepoints are from the Font Awesome range that Nerd Fonts patches in, which
//! is the most widely covered subset. Callers should treat these as pure
//! decoration: every panel renders correctly if the font is missing (you just
//! see a box), so nothing depends on them semantically.

/// Folder, closed.
pub const DIR: &str = "\u{f07b}";
/// Folder with contents expanded.
pub const DIR_OPEN: &str = "\u{f07c}";
/// Generic document.
pub const FILE: &str = "\u{f15b}";
pub const FILE_TEXT: &str = "\u{f15c}";
pub const FILE_CODE: &str = "\u{f1c9}";
pub const FILE_ARCHIVE: &str = "\u{f1c6}";
pub const FILE_IMAGE: &str = "\u{f1c5}";
pub const FILE_PDF: &str = "\u{f1c1}";
pub const FILE_AUDIO: &str = "\u{f1c7}";
pub const FILE_VIDEO: &str = "\u{f1c8}";
pub const TABLE: &str = "\u{f0ce}";
pub const DATABASE: &str = "\u{f1c0}";
pub const TERMINAL: &str = "\u{f120}";
pub const GEAR: &str = "\u{f013}";
pub const LOCK: &str = "\u{f023}";
pub const RUST: &str = "\u{e7a8}";
pub const PYTHON: &str = "\u{e73c}";
pub const MARKDOWN: &str = "\u{f48a}";
pub const JSON: &str = "\u{e60b}";
pub const GIT: &str = "\u{f1d3}";

// panel / chrome glyphs
pub const SERVER: &str = "\u{f233}";
pub const DRIVE: &str = "\u{f0a0}";
pub const CLOUD: &str = "\u{f0c2}";
pub const CUBES: &str = "\u{f1b3}";
pub const GAUGE: &str = "\u{f0e4}";
pub const LIST: &str = "\u{f03a}";
pub const EXCHANGE: &str = "\u{f0ec}";
pub const STETHOSCOPE: &str = "\u{f0f1}";
pub const TACHOMETER: &str = "\u{f3fd}";
pub const SLIDERS: &str = "\u{f1de}";
pub const COPY: &str = "\u{f0c5}";
pub const LINK: &str = "\u{f0c1}";
pub const ARROW_UP: &str = "\u{f062}";
pub const ARROW_DOWN: &str = "\u{f063}";
pub const CHECK: &str = "\u{f00c}";
pub const WARN: &str = "\u{f071}";
pub const TIMES: &str = "\u{f00d}";

/// Pick a glyph for a file name by extension. Falls back to a plain document.
pub fn for_file(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");

    match ext {
        // columnar / tabular
        "parquet" | "orc" | "avro" | "csv" | "tsv" => TABLE,
        // structured text
        "json" | "jsonl" | "ndjson" => JSON,
        "yaml" | "yml" | "toml" | "ini" | "conf" | "cfg" | "properties" => GEAR,
        "md" | "markdown" | "rst" => MARKDOWN,
        "xml" | "html" | "htm" => FILE_CODE,
        // code
        "rs" => RUST,
        "py" | "pyi" | "ipynb" => PYTHON,
        "sh" | "bash" | "zsh" | "fish" => TERMINAL,
        "c" | "h" | "cc" | "cpp" | "hpp" | "go" | "java" | "kt" | "scala" | "js" | "ts"
        | "jsx" | "tsx" | "rb" | "php" | "lua" | "sql" => FILE_CODE,
        // archives
        "gz" | "tgz" | "bz2" | "xz" | "zst" | "zip" | "tar" | "7z" | "rar" => FILE_ARCHIVE,
        // media
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" => FILE_IMAGE,
        "pdf" => FILE_PDF,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => FILE_AUDIO,
        "mp4" | "mkv" | "mov" | "avi" | "webm" => FILE_VIDEO,
        // text / logs
        "log" | "out" | "err" => LIST,
        "txt" | "text" | "readme" => FILE_TEXT,
        // data blobs
        "db" | "sqlite" | "sqlite3" | "mdb" | "rocksdb" => DATABASE,
        "bin" | "dat" | "block" | "blk" => CUBES,
        "lock" => LOCK,
        _ => {
            if lower.starts_with("readme") || lower.starts_with("license") || lower == "makefile" {
                FILE_TEXT
            } else if lower.starts_with(".git") {
                GIT
            } else {
                FILE
            }
        }
    }
}

/// Glyph for the replica-count column, kept distinct from the file icon so the
/// column scans without reading digits.
pub const REPLICA: &str = "\u{f0c5}";

/// Glyph for each panel's tab.
pub fn for_panel(panel: crate::app::Panel) -> &'static str {
    use crate::app::Panel;
    match panel {
        Panel::Dashboard => GAUGE,
        Panel::Files => DIR,
        Panel::Nodes => SERVER,
        Panel::Mounts => LINK,
        Panel::Jobs => LIST,
        Panel::Transfers => EXCHANGE,
        Panel::Fsck => STETHOSCOPE,
        Panel::Bench => TACHOMETER,
    }
}
