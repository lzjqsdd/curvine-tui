//! Event plumbing: terminal input on one side, background task results on the
//! other. Both funnel into a single `AppEvent` channel consumed by the main loop.

use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    #[allow(dead_code)]
    Mouse(MouseEvent),
    #[allow(dead_code)]
    Resize(u16, u16),
    /// Periodic tick driving auto-refresh.
    Tick,
    /// A background job finished. The payload is panel-specific state.
    TaskDone(TaskResult),
}

/// Result of an async operation, tagged with the panel that requested it so
/// the reducer can route it without storing closures.
#[derive(Debug)]
pub enum TaskResult {
    FsListing {
        path: String,
        result: Result<Vec<FsEntry>, String>,
    },
    ClusterInfo(Box<Result<ClusterSnapshot, String>>),
    Mounts(Result<Vec<MountRow>, String>),
    Jobs(Result<Vec<JobRow>, String>),
    Config(Result<Vec<ConfigRow>, String>),
    TransferProgress {
        id: u64,
        transferred: u64,
        /// Total size in bytes; 0 while still unknown (e.g. a download that has
        /// not opened its reader yet).
        total: u64,
        speed: u64,
    },
    TransferDone {
        id: u64,
        result: std::result::Result<u64, String>,
    },
    OpDone {
        label: String,
        result: Result<String, String>,
    },
}

#[derive(Debug, Clone)]
pub struct FsEntry {
    pub name: String,
    #[allow(dead_code)]
    pub path: String,
    pub is_dir: bool,
    pub len: i64,
    pub mtime: i64,
    pub mode: u32,
    pub owner: String,
    pub group: String,
    #[allow(dead_code)]
    pub replicas: i32,
}

#[derive(Debug, Clone)]
pub struct ClusterSnapshot {
    pub active_master: String,
    pub journal_nodes: Vec<String>,
    pub inode_dir_num: i64,
    pub inode_file_num: i64,
    pub block_num: i64,
    pub capacity: i64,
    pub available: i64,
    pub fs_used: i64,
    pub non_fs_used: i64,
    pub workers: Vec<WorkerRow>,
}

#[derive(Debug, Clone)]
pub struct WorkerRow {
    pub address: String,
    pub status: String,
    pub capacity: i64,
    pub available: i64,
    pub fs_used: i64,
    pub block_num: i64,
    pub weight: u32,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct MountRow {
    pub cv_path: String,
    pub ufs_path: String,
    pub mount_id: u32,
    pub ttl_ms: i64,
    pub storage_type: String,
    pub write_type: String,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct JobRow {
    pub job_id: String,
    pub state: String,
    pub source: String,
    pub target: String,
    pub progress: f64,
    pub total_files: u64,
    pub completed: u64,
}

#[derive(Debug, Clone)]
pub struct ConfigRow {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Upload,
    Download,
}

impl TransferKind {
    pub fn arrow(self) -> &'static str {
        match self {
            TransferKind::Upload => "up",
            TransferKind::Download => "down",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl TransferState {
    pub fn label(self) -> &'static str {
        match self {
            TransferState::Queued => "queued",
            TransferState::Running => "running",
            TransferState::Done => "done",
            TransferState::Failed => "failed",
            TransferState::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TransferState::Done | TransferState::Failed | TransferState::Cancelled
        )
    }
}

/// Spawns the crossterm reader thread. Returns immediately.
pub fn spawn_input_thread(tx: mpsc::UnboundedSender<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(250)) {
                Ok(true) => match event::read() {
                    Ok(CrosstermEvent::Key(k)) => {
                        if tx.send(AppEvent::Key(k)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Mouse(m)) => {
                        if tx.send(AppEvent::Mouse(m)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Resize(w, h)) => {
                        if tx.send(AppEvent::Resize(w, h)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    });
}

/// Spawns the tick generator driving auto-refresh.
pub fn spawn_tick_thread(tx: mpsc::UnboundedSender<AppEvent>, interval: Duration) {
    std::thread::spawn(move || loop {
        std::thread::sleep(interval);
        if tx.send(AppEvent::Tick).is_err() {
            break;
        }
    });
}
