//! Event plumbing: terminal input on one side, background task results on the
//! other. Both funnel into a single `AppEvent` channel consumed by the main loop.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
    /// Frame timer: carries the elapsed time since the last frame so springs
    /// can be integrated against real time rather than an assumed rate.
    Frame(Duration),
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
    Fsck(Box<Result<FsckSnapshot, String>>),
    Bench(Box<Result<BenchSnapshot, String>>),
    Describe {
        path: String,
        blocks: Result<Vec<BlockRow>, String>,
    },
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
    /// `ip:rpc_port` — the identifying endpoint, not the verbose Display form.
    pub address: String,
    pub ip: String,
    pub hostname: String,
    pub rpc_port: u32,
    pub web_port: u32,
    pub worker_id: u32,
    pub status: String,
    pub capacity: i64,
    pub available: i64,
    pub fs_used: i64,
    pub block_num: i64,
    pub weight: u32,
    pub version: String,
    /// `(storage class, capacity bytes)` for the per-disk breakdown.
    pub storage: Vec<(String, i64)>,
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

#[derive(Debug, Clone)]
pub struct FsckSnapshot {
    pub root: String,
    pub file_count: usize,
    pub block_count: usize,
    pub expected_replicas: usize,
    pub recorded_replicas: usize,
    pub available_replicas: usize,
    pub unavailable_replicas: usize,
    pub under_replicated_blocks: usize,
    pub missing_replica_blocks: usize,
    pub health_ratio: f64,
    pub warnings: bool,
    pub errors: Vec<(String, String)>,
    /// Only unhealthy files are kept; a healthy tree would otherwise hold
    /// thousands of rows the user never looks at.
    pub bad_files: Vec<FsckFileRow>,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct FsckFileRow {
    pub path: String,
    pub len: i64,
    pub blocks: usize,
    pub expected: usize,
    pub available: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BenchSnapshot {
    pub rows: Vec<BenchRowData>,
    #[allow(dead_code)]
    pub temp_path: String,
    pub elapsed_ms: f64,
    #[allow(dead_code)]
    pub started: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct BenchRowData {
    #[allow(dead_code)]
    pub group: String,
    pub op: String,
    pub item: String,
    pub value: f64,
    pub unit: String,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub errors: u64,
}

/// One replica of one block, flattened for the UI.
#[derive(Debug, Clone)]
pub struct BlockRow {
    pub block_id: String,
    pub offset: i64,
    pub len: i64,
    /// `ip:port` of the holding worker.
    pub worker: String,
    pub available: bool,
    /// Worker status at the time of the query (`Live`, `Lost`, …).
    pub state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Upload,
    Download,
}

impl TransferKind {
    #[allow(dead_code)]
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

/// Shared frame interval in milliseconds.
///
/// The input thread already polls crossterm with a timeout, so it doubles as the
/// frame clock: the main loop simply changes this value to say how often it
/// wants to be woken. That avoids a second thread and, more importantly, avoids
/// redrawing at 60 fps while the UI is idle.
pub type FrameClock = Arc<AtomicU64>;

pub fn new_frame_clock() -> FrameClock {
    Arc::new(AtomicU64::new(crate::anim::IDLE_FRAME.as_millis() as u64))
}

pub fn set_frame_interval(clock: &FrameClock, interval: Duration) {
    clock.store(interval.as_millis().max(1) as u64, Ordering::Relaxed);
}

/// Spawns the crossterm reader thread. Returns immediately.
///
/// When no input arrives within the frame interval the thread emits
/// `AppEvent::Frame`, which is what drives animation.
pub fn spawn_input_thread(tx: mpsc::UnboundedSender<AppEvent>, clock: FrameClock) {
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        loop {
            let interval = Duration::from_millis(clock.load(Ordering::Relaxed).max(1));
            match event::poll(interval) {
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
                Ok(false) => {
                    // No input within the interval: this is a frame boundary.
                    let now = std::time::Instant::now();
                    let dt = now.duration_since(last);
                    last = now;
                    if tx.send(AppEvent::Frame(dt)).is_err() {
                        break;
                    }
                }
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
