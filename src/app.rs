//! Application state and reducer.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::event::{
    AppEvent, BenchRowData, BenchSnapshot, ClusterSnapshot, FsEntry, FsckFileRow,
    FsckSnapshot, JobRow, MountRow, TaskResult, TransferKind, TransferState, WorkerRow,
};
use crate::rpc::ClusterClient;
use crate::anim::AnimState;
use crate::ui::menu::{Menu, MenuAction, MenuItem, KEY_DELETE, KEY_ENTER};
use crate::transfer::{new_cancel_flag, spawn_transfer, CancelFlag};
use curvine_runtime::runtime::RpcRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Dashboard,
    Files,
    Nodes,
    Mounts,
    Jobs,
    Transfers,
    Fsck,
    Bench,
}

impl Panel {
    pub const ALL: [Panel; 8] = [
        Panel::Dashboard,
        Panel::Files,
        Panel::Nodes,
        Panel::Mounts,
        Panel::Jobs,
        Panel::Transfers,
        Panel::Fsck,
        Panel::Bench,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Dashboard => "Dashboard",
            Panel::Files => "Files",
            Panel::Nodes => "Nodes",
            Panel::Mounts => "Mounts",
            Panel::Jobs => "Jobs",
            Panel::Transfers => "Transfers",
            Panel::Fsck => "Fsck",
            Panel::Bench => "Bench",
        }
    }

    /// Nerd Font glyph shown in the tab strip.
    pub fn icon(self) -> &'static str {
        crate::ui::icons::for_panel(self)
    }

    /// Compact label used when the tab bar cannot fit full titles.
    pub fn short_title(self) -> &'static str {
        match self {
            Panel::Dashboard => "Dash",
            Panel::Files => "Files",
            Panel::Nodes => "Nodes",
            Panel::Mounts => "Mounts",
            Panel::Jobs => "Jobs",
            Panel::Transfers => "Xfer",
            Panel::Fsck => "Fsck",
            Panel::Bench => "Bench",
        }
    }

    pub fn index(self) -> usize {
        Panel::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    /// `:` command line, mirrors the CLI syntax.
    Command,
    /// Filter box in the file browser.
    Filter,
    /// Guided put/get form.
    Transfer,
    /// Mount form popup.
    Mount,
    /// Destructive-action confirmation popup.
    Confirm,
    /// Read-only detail popup (directories, confirmations).
    Describe,
    /// Contextual action menu.
    Menu,
    /// Full-screen detail view.
    Detail,
}

/// Per-panel load state, so we can render spinners and errors uniformly.
#[derive(Debug)]
pub struct Loadable<T> {
    pub loading: bool,
    pub error: Option<String>,
    pub data: Option<T>,
    pub loaded_at: Option<Instant>,
}

impl<T> Default for Loadable<T> {
    fn default() -> Self {
        Self {
            loading: false,
            error: None,
            data: None,
            loaded_at: None,
        }
    }
}

impl<T> Loadable<T> {
    pub fn begin(&mut self) {
        self.loading = true;
        self.error = None;
    }

    pub fn fail(&mut self, err: String) {
        self.loading = false;
        self.error = Some(err);
    }

    pub fn ok(&mut self, data: T) {
        self.loading = false;
        self.error = None;
        self.data = Some(data);
        self.loaded_at = Some(Instant::now());
    }
}

pub struct FilesState {
    pub cwd: String,
    pub entries: Vec<FsEntry>,
    pub selected: usize,
    pub filter: String,
    pub load: Loadable<()>,
    pub show_hidden: bool,
    /// Absolute paths marked for batch operations. Paths, not names, so two
    /// entries with the same name in different directories cannot collide.
    pub marked: Vec<String>,
}

impl Default for FilesState {
    fn default() -> Self {
        Self {
            cwd: "/".to_string(),
            entries: Vec::new(),
            selected: 0,
            filter: String::new(),
            load: Loadable::default(),
            show_hidden: true,
            marked: Vec::new(),
        }
    }
}

impl FilesState {
    /// Entries after the filter is applied.
    pub fn visible(&self) -> Vec<&FsEntry> {
        let f = self.filter.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .filter(|e| f.is_empty() || e.name.to_ascii_lowercase().contains(&f))
            .collect()
    }

    pub fn selected_entry(&self) -> Option<&FsEntry> {
        let f = self.filter.clone();
        self.entries
            .iter()
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .filter(|e| f.is_empty() || e.name.to_ascii_lowercase().contains(&f.to_ascii_lowercase()))
            .nth(self.selected)
    }

    pub fn move_down(&mut self) {
        let n = self.visible().len();
        if n > 0 {
            self.selected = (self.selected + 1).min(n - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Absolute path of the highlighted row.
    pub fn selected_path(&self) -> Option<String> {
        self.selected_entry()
            .map(|e| join_path(&self.cwd, &e.name))
    }

    pub fn is_marked(&self, path: &str) -> bool {
        self.marked.iter().any(|m| m == path)
    }

    pub fn toggle_mark(&mut self, path: String) {
        if let Some(pos) = self.marked.iter().position(|m| *m == path) {
            self.marked.remove(pos);
        } else {
            self.marked.push(path);
        }
    }

    pub fn mark_all_visible(&mut self) {
        let paths: Vec<String> = self
            .visible()
            .iter()
            .map(|e| join_path(&self.cwd, &e.name))
            .collect();
        for path in paths {
            if !self.is_marked(&path) {
                self.marked.push(path);
            }
        }
    }
}

/// A single in-flight or finished transfer, rendered in the Transfers panel.
pub struct TransferEntry {
    pub id: u64,
    pub kind: TransferKind,
    pub local: String,
    pub remote: String,
    pub total: u64,
    pub transferred: u64,
    pub speed: u64,
    pub state: TransferState,
    pub error: Option<String>,
    pub cancel: CancelFlag,
}

impl TransferEntry {
    pub fn ratio(&self) -> f64 {
        if self.total == 0 {
            if self.state == TransferState::Done {
                1.0
            } else {
                0.0
            }
        } else {
            (self.transferred as f64 / self.total as f64).clamp(0.0, 1.0)
        }
    }
}

/// Cyclable write mode for the mount form, mirroring `WriteType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountWriteMode {
    CacheMode,
    FsMode,
}

impl MountWriteMode {
    #[allow(dead_code)]
    pub const ALL: [MountWriteMode; 2] = [MountWriteMode::CacheMode, MountWriteMode::FsMode];

    pub fn label(self) -> &'static str {
        match self {
            MountWriteMode::CacheMode => "cache",
            MountWriteMode::FsMode => "fs",
        }
    }

    pub fn next(self) -> Self {
        match self {
            MountWriteMode::CacheMode => MountWriteMode::FsMode,
            MountWriteMode::FsMode => MountWriteMode::CacheMode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountStorage {
    Disk,
    Ssd,
    Hdd,
    Mem,
    Ufs,
}

impl MountStorage {
    #[allow(dead_code)]
    pub const ALL: [MountStorage; 5] = [
        MountStorage::Disk,
        MountStorage::Ssd,
        MountStorage::Hdd,
        MountStorage::Mem,
        MountStorage::Ufs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MountStorage::Disk => "disk",
            MountStorage::Ssd => "ssd",
            MountStorage::Hdd => "hdd",
            MountStorage::Mem => "mem",
            MountStorage::Ufs => "ufs",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn to_model(self) -> curvine_model::StorageType {
        match self {
            MountStorage::Disk => curvine_model::StorageType::Disk,
            MountStorage::Ssd => curvine_model::StorageType::Ssd,
            MountStorage::Hdd => curvine_model::StorageType::Hdd,
            MountStorage::Mem => curvine_model::StorageType::Mem,
            MountStorage::Ufs => curvine_model::StorageType::Ufs,
        }
    }
}

/// A destructive action waiting on confirmation. Holding the intent (instead of
/// a pre-rendered message) means the confirm handler cannot fire a different
/// action than the one the user was shown.
#[derive(Debug, Clone)]
pub enum PendingAction {
    Delete { paths: Vec<String>, recursive: bool },
    Umount { path: String },
    Free { paths: Vec<String>, recursive: bool },
}

pub struct PendingConfirm {
    pub action: PendingAction,
    pub title: String,
    pub target: String,
    pub lines: Vec<String>,
}

/// Read-only detail popup. Rows are pre-formatted so the renderer stays dumb.
#[derive(Debug, Clone, Default)]
pub struct DescribeView {
    pub title: String,
    /// What this popup is about (a path or a worker address). Async responses
    /// are matched against it so a late reply cannot populate a newer popup.
    pub subject: String,
    pub headline: Option<(String, String)>,
    pub rows: Vec<(String, String)>,
    /// Per-worker block distribution, already aggregated and sorted.
    pub worker_blocks: Vec<WorkerBlocks>,
    /// Flat block list, sorted by offset.
    pub blocks: Vec<crate::event::BlockRow>,
    pub loading: bool,
}

/// Which full-screen detail layout to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailKind {
    File,
    Worker,
}

/// Full-screen detail for a file or worker. Replaces the old describe popup
/// for these two subjects; `DescribeView` is kept for directory popups.
#[derive(Debug, Clone)]
pub struct DetailView {
    pub title: String,
    /// What the view is about (a path or worker endpoint). Async block replies
    /// are matched against it so a late response cannot fill a newer view.
    pub subject: String,
    pub kind: DetailKind,
    pub headline: Option<(String, String)>,
    pub rows: Vec<(String, String)>,
    /// Flat replicas, sorted by offset; the renderer aggregates per block.
    pub blocks: Vec<crate::event::BlockRow>,
    /// Per-worker aggregate, already sorted.
    pub worker_blocks: Vec<WorkerBlocks>,
    /// Storage-class breakdown for workers.
    pub storage: Vec<(String, i64)>,
    pub loading: bool,
    pub error: Option<String>,
}

/// Blocks held by one worker, used for the distribution view.
#[derive(Debug, Clone)]
pub struct WorkerBlocks {
    pub worker: String,
    pub block_ids: Vec<String>,
    pub bytes: i64,
    /// Number of that worker's replicas that are actually readable.
    pub available: usize,
    pub total: usize,
}

/// Fields of the mount form, in tab order.
pub const MOUNT_FIELDS: usize = 7;

/// Multi-field mount form. Values are kept as strings and only parsed on submit
/// so a partially typed duration never blocks editing.
pub struct MountForm {
    pub ufs_path: String,
    pub cv_path: String,
    pub write_type: MountWriteMode,
    pub storage_type: MountStorage,
    pub ttl: String,
    pub auto_cache: bool,
    pub read_verify_ufs: bool,
    pub focus: usize,
    pub error: Option<String>,
}

impl MountForm {
    pub fn new(cwd: &str) -> Self {
        Self {
            ufs_path: String::new(),
            cv_path: cwd.to_string(),
            write_type: MountWriteMode::CacheMode,
            storage_type: MountStorage::Disk,
            ttl: "0".to_string(),
            auto_cache: false,
            read_verify_ufs: false,
            focus: 0,
            error: None,
        }
    }

    fn field_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            0 => Some(&mut self.ufs_path),
            1 => Some(&mut self.cv_path),
            4 => Some(&mut self.ttl),
            _ => None,
        }
    }
}

/// Two-field put/get form. `focus` selects which field receives keystrokes.
pub struct TransferForm {
    pub kind: TransferKind,
    pub local: String,
    pub remote: String,
    pub focus: usize,
    pub error: Option<String>,
}

impl TransferForm {
    pub fn upload(cwd: &str) -> Self {
        Self {
            kind: TransferKind::Upload,
            local: String::new(),
            remote: if cwd == "/" {
                "/".to_string()
            } else {
                format!("{cwd}/")
            },
            focus: 0,
            error: None,
        }
    }

    pub fn download(cwd: &str) -> Self {
        Self {
            kind: TransferKind::Download,
            local: String::new(),
            remote: if cwd == "/" {
                "/".to_string()
            } else {
                format!("{cwd}/")
            },
            focus: 1,
            error: None,
        }
    }

    fn field_mut(&mut self) -> &mut String {
        if self.focus == 0 {
            &mut self.local
        } else {
            &mut self.remote
        }
    }
}

pub struct App {
    pub cfg: AppConfig,
    pub client: ClusterClient,
    pub tx: mpsc::UnboundedSender<AppEvent>,

    pub panel: Panel,
    pub mode: InputMode,
    pub command: String,
    pub should_quit: bool,

    pub status: String,
    pub status_is_error: bool,

    /// Configured auto-refresh interval, and the moment the next tick lands.
    /// The header renders the remaining seconds as a countdown.
    pub refresh_secs: u64,
    pub next_refresh: Instant,

    pub files: FilesState,
    pub cluster: Loadable<ClusterSnapshot>,
    pub mounts: Loadable<Vec<MountRow>>,
    pub jobs: Loadable<Vec<JobRow>>,
    pub job_selected: usize,
    /// Jobs this session started. The client library has no "list jobs" call,
    /// so this is the only way to show more than a single lookup.
    pub tracked_jobs: Vec<String>,

    pub fsck: Loadable<FsckSnapshot>,
    pub fsck_path: String,
    pub fsck_cancel: CancelFlag,
    pub bench: Loadable<BenchSnapshot>,
    pub bench_opts: crate::rpc::bench::BenchOptions,
    pub bench_running: Option<std::time::Instant>,

    pub transfers: Vec<TransferEntry>,
    pub transfer_selected: usize,
    pub form: Option<TransferForm>,
    pub mount_selected: usize,
    pub mount_form: Option<MountForm>,
    pub node_selected: usize,
    /// Contextual action menu (`x`), when open.
    pub menu: Option<crate::ui::menu::Menu>,
    /// Recently executed `:` commands, newest last.
    pub command_history: Vec<String>,
    /// Position while browsing history with ↑/↓, or None for a fresh line.
    pub command_history_pos: Option<usize>,
    /// Blocking confirmation for a destructive action.
    pub confirm: Option<PendingConfirm>,
    /// Read-only popup for directory details.
    pub describe: Option<DescribeView>,
    /// Full-screen detail view for files and workers.
    pub detail: Option<DetailView>,
    pub(crate) next_transfer_id: u64,

    /// Every time-based value the UI renders. Advanced once per frame.
    pub anim: AnimState,

    pub log: Vec<String>,
    pub help_visible: bool,
}

impl App {
    pub fn new(
        cfg: AppConfig,
        client: ClusterClient,
        tx: mpsc::UnboundedSender<AppEvent>,
        reduced_motion: bool,
        refresh_secs: u64,
    ) -> Self {
        let refresh_secs = refresh_secs.max(1);
        let mut app = Self {
            cfg,
            client,
            tx,
            panel: Panel::Dashboard,
            mode: InputMode::Normal,
            command: String::new(),
            should_quit: false,
            status: String::new(),
            status_is_error: false,
            refresh_secs,
            next_refresh: Instant::now() + std::time::Duration::from_secs(refresh_secs),
            files: FilesState::default(),
            cluster: Loadable::default(),
            mounts: Loadable::default(),
            jobs: Loadable::default(),
            job_selected: 0,
            tracked_jobs: Vec::new(),
            fsck: Loadable::default(),
            fsck_path: "/".to_string(),
            fsck_cancel: new_cancel_flag(),
            bench: Loadable::default(),
            bench_opts: crate::rpc::bench::BenchOptions::default(),
            bench_running: None,
            transfers: Vec::new(),
            transfer_selected: 0,
            form: None,
            mount_selected: 0,
            mount_form: None,
            node_selected: 0,
            menu: None,
            command_history: Vec::new(),
            command_history_pos: None,
            confirm: None,
            describe: None,
            detail: None,
            next_transfer_id: 1,
            anim: AnimState::new(reduced_motion),
            log: Vec::new(),
            help_visible: false,
        };
        app.log(format!(
            "connected: config={} masters={}",
            app.cfg.source,
            app.cfg.master_addrs.join(",")
        ));
        app.refresh_files();
        app.refresh_cluster();
        app
    }

    /// Seconds left until the next automatic refresh, as shown in the header.
    pub fn refresh_countdown_secs(&self) -> u64 {
        self.next_refresh
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
            .ceil() as u64
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        let ts = chrono::Local::now().format("%H:%M:%S");
        self.log.push(format!("{ts} {}", msg.into()));
        if self.log.len() > 500 {
            self.log.remove(0);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_status_for_test(&mut self, msg: impl Into<String>) {
        self.set_status(msg, false);
    }

    fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        let msg = msg.into();
        self.log(if is_error {
            format!("ERROR {msg}")
        } else {
            msg.clone()
        });
        self.status = msg;
        self.status_is_error = is_error;
        self.anim.mark_status();
    }

    // ------------------------------------------------------------- refreshers

    pub fn refresh_files(&mut self) {
        let path = self.files.cwd.clone();
        self.files.load.begin();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.client.runtime().spawn(async move {
            let res = client
                .ls(&path)
                .await
                .map(|list| list.into_iter().map(to_fs_entry).collect::<Vec<_>>())
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::FsListing {
                path,
                result: res,
            }));
        });
    }

    pub fn refresh_cluster(&mut self) {
        self.cluster.begin();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.client.runtime().spawn(async move {
            let res = client
                .filesystem_info()
                .await
                .map(|info| ClusterSnapshot {
                    active_master: info.active_master.clone(),
                    journal_nodes: info.journal_nodes.clone(),
                    inode_dir_num: info.inode_dir_num,
                    inode_file_num: info.inode_file_num,
                    block_num: info.block_num,
                    capacity: info.capacity,
                    available: info.available,
                    fs_used: info.fs_used,
                    non_fs_used: info.non_fs_used,
                    workers: info
                        .live_workers
                        .iter()
                        .map(|w| WorkerRow {
                            // Display on WorkerAddress is the verbose
                            // "worker_id = .., addr = host/ip:port" form; build
                            // the endpoint explicitly instead.
                            address: format!("{}:{}", w.address.ip_addr, w.address.rpc_port),
                            ip: w.address.ip_addr.clone(),
                            hostname: w.address.hostname.clone(),
                            rpc_port: w.address.rpc_port,
                            web_port: w.address.web_port,
                            worker_id: w.address.worker_id,
                            status: format!("{:?}", w.status),
                            capacity: w.capacity,
                            available: w.available,
                            fs_used: w.fs_used,
                            block_num: w.block_num,
                            weight: w.weight,
                            version: w.software_version.clone(),
                            storage: w
                                .storage_map
                                .iter()
                                .map(|(k, v)| (k.clone(), v.capacity))
                                .collect(),
                        })
                        .collect(),
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::ClusterInfo(Box::new(res))));
        });
    }

    pub fn refresh_mounts(&mut self) {
        self.mounts.begin();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.client.runtime().spawn(async move {
            let res = client
                .mounts()
                .await
                .map(|list| {
                    list.into_iter()
                        .map(|m| MountRow {
                            cv_path: m.cv_path,
                            ufs_path: m.ufs_path,
                            mount_id: m.mount_id,
                            ttl_ms: m.ttl_ms,
                            storage_type: m.storage_type.map(|s| format!("{s:?}")).unwrap_or_default(),
                            write_type: format!("{:?}", m.write_type),
                            provider: m.provider.map(|p| format!("{p:?}")).unwrap_or_default(),
                        })
                        .collect()
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::Mounts(res)));
        });
    }

    pub fn refresh_jobs(&mut self) {
        self.jobs.begin();
        let client = self.client.clone();
        let tx = self.tx.clone();
        let ids = self.tracked_jobs.clone();
        self.client.runtime().spawn(async move {
            let res = client
                .jobs(&ids)
                .await
                .map(|list| list.into_iter().map(to_job_row).collect::<Vec<_>>())
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::Jobs(res)));
        });
    }

    /// Kick off a full-tree replica scan.
    pub fn start_fsck(&mut self) {
        if self.fsck.loading {
            return;
        }
        // A fresh flag each run so a previous cancel does not poison this scan.
        self.fsck_cancel = new_cancel_flag();
        let cancel = self.fsck_cancel.clone();
        let root = self.fsck_path.clone();
        self.fsck.begin();

        let client = self.client.clone();
        let tx = self.tx.clone();
        self.client.runtime().spawn(async move {
            let started = std::time::Instant::now();
            let res = client
                .fsck(
                    &root,
                    crate::rpc::fsck::ScanLimits::default(),
                    &cancel,
                    |_, _| {},
                )
                .await
                .map(|r| {
                    let mut bad: Vec<FsckFileRow> = r
                        .files
                        .iter()
                        .filter(|f| {
                            let unhealthy = f.error.is_some()
                                || f.blocks.iter().any(|b| {
                                    b.available_replicas() < f.expected_replicas
                                });
                            unhealthy
                        })
                        .map(|f| FsckFileRow {
                            path: f.path.clone(),
                            len: f.len,
                            blocks: f.block_count,
                            expected: f.expected_replicas,
                            available: f.available_replicas,
                            error: f.error.clone(),
                        })
                        .collect();
                    // Scan failures are per-file problems too, so surface them
                    // in the same table rather than only as a count.
                    for (path, err) in &r.errors {
                        bad.push(FsckFileRow {
                            path: path.clone(),
                            len: 0,
                            blocks: 0,
                            expected: 0,
                            available: 0,
                            error: Some(err.clone()),
                        });
                    }
                    bad.sort_by(|a, b| a.path.cmp(&b.path));

                    FsckSnapshot {
                        root: r.root.clone(),
                        file_count: r.file_count,
                        block_count: r.block_count,
                        expected_replicas: r.expected_replicas,
                        recorded_replicas: r.recorded_replicas,
                        available_replicas: r.available_replicas,
                        unavailable_replicas: r.unavailable_replicas,
                        under_replicated_blocks: r.under_replicated_blocks,
                        missing_replica_blocks: r.missing_replica_blocks,
                        health_ratio: r.health_ratio(),
                        warnings: r.has_warnings(),
                        errors: r.errors.clone(),
                        bad_files: bad,
                        elapsed_ms: started.elapsed().as_millis(),
                    }
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::Fsck(Box::new(res))));
        });
        self.set_status(format!("fsck {} …", self.fsck_path), false);
    }

    pub fn cancel_fsck(&mut self) {
        if self.fsck.loading {
            self.fsck_cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self.set_status("fsck cancel requested", false);
        }
    }

    /// Run the benchmark on a background task.
    pub fn start_bench(&mut self) {
        if self.bench_running.is_some() {
            return;
        }
        self.bench.begin();
        self.bench_running = Some(std::time::Instant::now());

        let client = self.client.clone();
        let tx = self.tx.clone();
        let opts = self.bench_opts.clone();
        self.client.runtime().spawn(async move {
            let res = client
                .run_bench(&opts)
                .await
                .map(|s| BenchSnapshot {
                    rows: s
                        .rows
                        .into_iter()
                        .map(|r| BenchRowData {
                            group: r.group,
                            op: r.op,
                            item: r.item,
                            value: r.value,
                            unit: r.value_unit,
                            p50: r.p50_ms,
                            p95: r.p95_ms,
                            p99: r.p99_ms,
                            errors: r.errors,
                        })
                        .collect(),
                    temp_path: s.temp_path,
                    elapsed_ms: s.elapsed_ms,
                    started: std::time::Instant::now(),
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::Bench(Box::new(res))));
        });
        self.set_status("bench started …", false);
    }

    pub fn refresh_panel(&mut self) {
        match self.panel {
            Panel::Dashboard => self.refresh_cluster(),
            Panel::Files => self.refresh_files(),
            Panel::Nodes => self.refresh_cluster(),
            Panel::Mounts => self.refresh_mounts(),
            Panel::Jobs => self.refresh_jobs(),
            Panel::Transfers => {} // driven by progress events, not polling
            Panel::Fsck => self.start_fsck(),
            Panel::Bench => {}
        }
    }

    // ---------------------------------------------------------------- reducer

    pub fn on_event(&mut self, evt: AppEvent) {
        match evt {
            AppEvent::Key(k) => self.on_key(k),
            AppEvent::Resize(_, _) => {}
            AppEvent::Mouse(_) => {}
            AppEvent::Frame(dt) => self.on_frame(dt),
            AppEvent::Tick => {
                self.next_refresh =
                    Instant::now() + std::time::Duration::from_secs(self.refresh_secs);
                // Auto-refresh the active data panel if stale.
                match self.panel {
                    Panel::Dashboard | Panel::Nodes => self.refresh_cluster(),
                    Panel::Jobs => self.refresh_jobs(),
                    // Scan/benchmark are expensive and user-triggered; never
                    // re-run them on the tick.
                    Panel::Fsck | Panel::Bench | Panel::Transfers => {}
                    _ => {}
                }
            }
            AppEvent::TaskDone(t) => self.on_task(t),
        }
    }

    /// Point every spring at the value currently held in state.
    ///
    /// Springs are interruptible, so retargeting mid-flight bends the curve
    /// instead of restarting it. Called when data changes and once per frame;
    /// `set_target` with an unchanged value is a no-op.
    pub fn retarget_animations(&mut self) {
        if let Some(c) = &self.cluster.data {
            self.anim
                .capacity
                .set_target(crate::ui::ratio(c.fs_used, c.capacity.max(1)));
            self.anim.dirs.set_target(c.inode_dir_num as f64);
            self.anim.files.set_target(c.inode_file_num as f64);
            self.anim.blocks.set_target(c.block_num as f64);
            self.anim.workers.set_target(c.workers.len() as f64);
        }
        if let Some(f) = &self.fsck.data {
            self.anim.health.set_target(f.health_ratio);
        }

        let ids: Vec<u64> = self.transfers.iter().map(|t| t.id).collect();
        for t in &self.transfers {
            let target = if t.total > 0 {
                (t.transferred as f64 / t.total as f64).clamp(0.0, 1.0)
            } else {
                t.ratio()
            };
            self.anim.transfer(t.id, target).set_target(target);
        }
        self.anim.prune_transfers(&ids);
    }

    /// Advance animation state. Called once per frame.
    fn on_frame(&mut self, dt: std::time::Duration) {
        self.retarget_animations();


        self.anim.step(dt);

        if self.anim.status_expired() {
            self.status.clear();
            self.status_is_error = false;
            self.anim.clear_status_mark();
        }
    }

    /// True when something on screen is still moving.
    pub fn is_animating(&self) -> bool {
        self.anim.is_animating()
    }

    /// True when a spinner or progress bar needs periodic redraws.
    pub fn is_busy(&self) -> bool {
        self.fsck.loading
            || self.cluster.loading
            || self.mounts.loading
            || self.jobs.loading
            || self.files.load.loading
            || self.bench_running.is_some()
            || self
                .transfers
                .iter()
                .any(|t| matches!(t.state, TransferState::Queued | TransferState::Running))
    }

    fn on_task(&mut self, t: TaskResult) {
        self.apply_task(t);
        self.retarget_animations();
    }

    fn apply_task(&mut self, t: TaskResult) {
        match t {
            TaskResult::FsListing { path, result } => {
                if path != self.files.cwd {
                    return; // stale response, user navigated away
                }
                match result {
                    Ok(mut entries) => {
                        entries.sort_by(|a, b| {
                            b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
                        });
                        self.files.entries = entries;
                        self.files.load.ok(());
                        let n = self.files.visible().len();
                        if self.files.selected >= n {
                            self.files.selected = n.saturating_sub(1);
                        }
                    }
                    Err(e) => {
                        self.files.load.fail(e.clone());
                        self.set_status(format!("ls {path}: {e}"), true);
                    }
                }
            }
            TaskResult::ClusterInfo(res) => match *res {
                Ok(s) => self.cluster.ok(s),
                Err(e) => {
                    self.cluster.fail(e.clone());
                    self.set_status(format!("cluster info: {e}"), true);
                }
            },
            TaskResult::Mounts(res) => match res {
                Ok(m) => self.mounts.ok(m),
                Err(e) => {
                    self.mounts.fail(e.clone());
                    self.set_status(format!("mounts: {e}"), true);
                }
            },
            TaskResult::Jobs(res) => match res {
                Ok(j) => self.jobs.ok(j),
                Err(e) => {
                    self.jobs.fail(e.clone());
                    self.set_status(format!("jobs: {e}"), true);
                }
            },
            TaskResult::TransferProgress {
                id,
                transferred,
                total,
                speed,
            } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    t.transferred = transferred;
                    if total > 0 {
                        t.total = total;
                    }
                    t.speed = speed;
                    t.state = TransferState::Running;
                }
            }
            TaskResult::TransferDone { id, result } => {
                let snapshot = self.transfers.iter().position(|t| t.id == id);
                if let Some(idx) = snapshot {
                    let already_cancelled = self.transfers[idx].state == TransferState::Cancelled;
                    match result {
                        Ok(total) => {
                            let t = &mut self.transfers[idx];
                            t.state = TransferState::Done;
                            t.total = total;
                            t.transferred = total;
                            t.speed = 0;
                            self.set_status(
                                format!("transfer #{id} complete ({} bytes)", total),
                                false,
                            );
                        }
                        Err(e) => {
                            let t = &mut self.transfers[idx];
                            t.speed = 0;
                            // A cancel surfaces as an error from the worker; keep
                            // the user-facing state accurate.
                            if already_cancelled || e.contains("cancelled") {
                                t.state = TransferState::Cancelled;
                            } else {
                                t.state = TransferState::Failed;
                                t.error = Some(e.clone());
                            }
                            self.set_status(format!("transfer #{id}: {e}"), true);
                        }
                    }
                }
                if matches!(self.panel, Panel::Files) {
                    self.refresh_files();
                }
            }
            TaskResult::Describe { path, blocks } => {
                // Only the view that asked for this path may consume it; a
                // response that arrives after the user closed or moved on is
                // dropped instead of resurrecting an old panel.
                let mut consumed = false;
                if let Some(view) = self.detail.as_mut() {
                    if view.subject == path {
                        consumed = true;
                        view.loading = false;
                        match blocks {
                            Ok(rows) => {
                                view.blocks = rows;
                                view.worker_blocks = aggregate_by_worker(&view.blocks);
                                let distinct = view
                                    .blocks
                                    .iter()
                                    .map(|b| b.block_id.as_str())
                                    .collect::<std::collections::BTreeSet<_>>()
                                    .len();
                                if let Some(slot) =
                                    view.rows.iter_mut().find(|(k, _)| k == "blocks")
                                {
                                    slot.1 = format!(
                                        "{} across {} workers",
                                        distinct,
                                        view.worker_blocks.len()
                                    );
                                }
                            }
                            Err(e) => {
                                view.error = Some(e.clone());
                                if let Some(slot) =
                                    view.rows.iter_mut().find(|(k, _)| k == "blocks")
                                {
                                    slot.1 = format!("unavailable: {e}");
                                }
                            }
                        }
                    }
                }
                if !consumed {
                    self.log(format!("describe {path}: view closed, response dropped"));
                }
            }
            TaskResult::Fsck(res) => match *res {
                Ok(s) => {
                    let msg = format!(
                        "fsck {}: {} files, {} blocks, {:.0}% healthy",
                        s.root,
                        s.file_count,
                        s.block_count,
                        s.health_ratio * 100.0
                    );
                    let warn = s.warnings;
                    self.fsck.ok(s);
                    self.set_status(msg, warn);
                }
                Err(e) => {
                    self.fsck.fail(e.clone());
                    self.set_status(format!("fsck: {e}"), true);
                }
            },
            TaskResult::Bench(res) => {
                self.bench_running = None;
                match *res {
                    Ok(s) => {
                        let n = s.rows.len();
                        let elapsed = s.elapsed_ms;
                        self.bench.ok(s);
                        self.set_status(
                            format!("bench complete: {n} metrics in {elapsed:.0}ms"),
                            false,
                        );
                    }
                    Err(e) => {
                        self.bench.fail(e.clone());
                        self.set_status(format!("bench: {e}"), true);
                    }
                }
            }
            TaskResult::OpDone { label, result } => match result {
                Ok(msg) => {
                    self.set_status(format!("{label}: {msg}"), false);
                    self.refresh_panel();
                }
                Err(e) => self.set_status(format!("{label}: {e}"), true),
            },
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        match self.mode {
            InputMode::Command => return self.on_command_key(key),
            InputMode::Filter => return self.on_filter_key(key),
            InputMode::Transfer => return self.on_form_key(key),
            InputMode::Mount => return self.on_mount_key(key),
            InputMode::Confirm => return self.on_confirm_key(key),
            InputMode::Describe => return self.on_describe_key(key),
            InputMode::Menu => return self.on_menu_key(key),
            InputMode::Detail => return self.on_detail_key(key),
            InputMode::Normal => {}
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Char('r') => self.refresh_panel(),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.help_visible = !self.help_visible,
            KeyCode::Esc => {
                if self.help_visible {
                    self.help_visible = false;
                } else if self.panel == Panel::Files && !self.files.marked.is_empty() {
                    self.clear_marks();
                }
            }
            KeyCode::Char(':') => {
                self.mode = InputMode::Command;
                self.command.clear();
            }
            KeyCode::Tab => {
                let i = self.panel.index();
                self.panel = Panel::ALL[(i + 1) % Panel::ALL.len()];
                self.refresh_panel();
            }
            KeyCode::BackTab => {
                let i = self.panel.index();
                self.panel = Panel::ALL[(i + Panel::ALL.len() - 1) % Panel::ALL.len()];
                self.refresh_panel();
            }
            KeyCode::Char(c @ '1'..='8') => {
                self.panel = Panel::ALL[(c as usize) - ('1' as usize)];
                self.refresh_panel();
            }
            KeyCode::Char('r') => self.refresh_panel(),
            KeyCode::Char('x') => self.open_menu(),
            KeyCode::Char('p') => self.open_form(TransferKind::Upload),
            KeyCode::Char('g') => self.open_form(TransferKind::Download),
            _ => self.on_panel_key(key),
        }
    }

    // ------------------------------------------------------------ action menu

    /// Short, stable name for the menu title: the current selection.
    fn menu_subject(&self) -> String {
        let raw = match self.panel {
            Panel::Files => self
                .files
                .selected_entry()
                .map(|e| e.name.clone())
                .or_else(|| Some(self.files.cwd.clone())),
            Panel::Nodes | Panel::Dashboard => self
                .cluster
                .data
                .as_ref()
                .and_then(|c| c.workers.get(self.node_selected))
                .map(|w| w.address.clone()),
            Panel::Mounts => self
                .mounts
                .data
                .as_ref()
                .and_then(|l| l.get(self.mount_selected))
                .map(|m| m.cv_path.clone()),
            Panel::Jobs => self
                .jobs
                .data
                .as_ref()
                .and_then(|l| l.get(self.job_selected))
                .map(|j| j.job_id.clone()),
            Panel::Transfers => self
                .transfers
                .get(self.transfer_selected)
                .map(|t| format!("#{} {}", t.id, t.remote)),
            Panel::Fsck => Some(self.fsck_path.clone()),
            Panel::Bench => Some(self.bench_opts.base_path.clone()),
        };
        truncate_menu_title(raw.unwrap_or_else(|| self.panel.title().to_string()))
    }

    /// Contextual menu entries. This is the single place that encodes what an
    /// `x` press can do, so the menu and the direct keys cannot drift apart.
    pub fn menu_items(&self) -> Vec<MenuItem> {
        use MenuAction::*;
        let item = |key: char, label: &str, action: MenuAction| MenuItem {
            key,
            label: label.to_string(),
            action,
        };
        match self.panel {
            Panel::Files => {
                if !self.files.marked.is_empty() {
                    let n = self.files.marked.len();
                    return vec![
                        item(KEY_DELETE, &format!("delete {n} marked items"), FileDelete { recursive: true }),
                        item('F', &format!("free {n} marked items"), FileFree),
                        item('u', "clear marks", FileClearMarks),
                    ];
                }
                match self.files.selected_entry() {
                    Some(e) if e.is_dir => vec![
                        item(KEY_ENTER, "enter directory", FileEnter),
                        item(KEY_DELETE, "delete recursively", FileDelete { recursive: true }),
                        item('r', "refresh", Refresh),
                    ],
                    Some(_) => vec![
                        item('d', "detail", FileDetail),
                        item(KEY_DELETE, "delete", FileDelete { recursive: false }),
                        item('F', "free cached blocks", FileFree),
                        item('p', "upload", FileUpload),
                        item('r', "refresh", Refresh),
                    ],
                    None => vec![item('r', "refresh", Refresh)],
                }
            }
            Panel::Dashboard | Panel::Nodes => vec![
                item('d', "detail", NodeDetail),
                item('r', "refresh", Refresh),
            ],
            Panel::Mounts => vec![
                item('n', "new mount", MountNew),
                item('u', "unmount", MountUmount),
                item('r', "refresh", Refresh),
            ],
            Panel::Jobs => {
                let mut items = Vec::new();
                let running = self
                    .jobs
                    .data
                    .as_ref()
                    .and_then(|l| l.get(self.job_selected))
                    .map(|j| {
                        let state = j.state.to_ascii_lowercase();
                        ["run", "progress", "queue", "pending"]
                            .iter()
                            .any(|k| state.contains(k))
                    })
                    .unwrap_or(false);
                if running {
                    items.push(item('c', "cancel job", JobCancel));
                }
                items.push(item('r', "refresh", Refresh));
                items
            }
            Panel::Transfers => vec![
                item('x', "cancel selected", TransferCancel),
                item('c', "clear finished", TransferClear),
                item('p', "upload", FileUpload),
                item('g', "download", FileDownload),
            ],
            Panel::Fsck => vec![
                item('s', "scan", FsckScan),
                item('x', "abort scan", FsckAbort),
                item('f', "target = browsed directory", FsckTargetCwd),
                item('r', "rescan", FsckScan),
            ],
            Panel::Bench => vec![
                item('b', "run benchmark", BenchRun),
                item('p', "toggle profile", BenchProfile),
                item('t', "toggle threads", BenchThreads),
                item('d', "toggle duration", BenchDuration),
            ],
        }
    }

    pub fn open_menu(&mut self) {
        let title = self.menu_subject();
        match Menu::new(title, self.menu_items()) {
            Some(menu) => {
                self.menu = Some(menu);
                self.mode = InputMode::Menu;
            }
            None => self.set_status("no actions for the current selection", true),
        }
    }

    fn close_menu(&mut self) {
        self.menu = None;
        self.mode = InputMode::Normal;
    }

    fn on_menu_key(&mut self, key: KeyEvent) {
        let Some(menu) = self.menu.as_mut() else {
            self.mode = InputMode::Normal;
            return;
        };
        let direct = match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                menu.move_down();
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                menu.move_up();
                return;
            }
            KeyCode::Esc => {
                self.close_menu();
                return;
            }
            KeyCode::Enter => menu.selected_item().map(|i| i.action),
            KeyCode::Delete => menu.item_for_key(KEY_DELETE).map(|i| i.action),
            KeyCode::Char(c) => menu.item_for_key(c).map(|i| i.action),
            _ => None,
        };
        if let Some(action) = direct {
            self.close_menu();
            self.run_menu_action(action);
        }
    }

    /// Execute a menu choice. Kept next to the direct-key handlers so both
    /// paths share one implementation.
    fn run_menu_action(&mut self, action: MenuAction) {
        use MenuAction::*;
        match action {
            FileDetail => self.files_describe(),
            FileEnter => self.files_enter(),
            FileDelete { recursive } => self.files_request_delete(recursive),
            FileFree => self.files_request_free(false),
            FileUpload => self.open_form(TransferKind::Upload),
            FileDownload => self.open_form(TransferKind::Download),
            FileClearMarks => self.clear_marks(),
            NodeDetail => self.node_describe(),
            MountNew => {
                let cwd = self.files.cwd.clone();
                self.mount_form = Some(MountForm::new(&cwd));
                self.mode = InputMode::Mount;
            }
            MountUmount => self.request_unmount(),
            JobCancel => self.cancel_selected_job(),
            TransferCancel => self.cancel_selected_transfer(),
            TransferClear => self.clear_finished_transfers(),
            FsckScan => self.start_fsck(),
            FsckAbort => self.cancel_fsck(),
            FsckTargetCwd => {
                self.fsck_path = self.files.cwd.clone();
                self.set_status(format!("fsck target: {}", self.fsck_path), false);
            }
            BenchRun => self.start_bench(),
            BenchProfile => self.bench_toggle_profile(),
            BenchThreads => self.bench_toggle_threads(),
            BenchDuration => self.bench_toggle_duration(),
            Refresh => self.refresh_panel(),
        }
    }

    fn clear_marks(&mut self) {
        let n = self.files.marked.len();
        self.files.marked.clear();
        self.set_status(format!("cleared {n} mark(s)"), false);
    }

    fn on_nodes_key(&mut self, key: KeyEvent) {
        let n = self.cluster.data.as_ref().map(|c| c.workers.len()).unwrap_or(0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 {
                    self.node_selected = (self.node_selected + 1).min(n - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.node_selected = self.node_selected.saturating_sub(1);
            }
            KeyCode::Char('d') | KeyCode::Enter => self.node_describe(),
            _ => {}
        }
    }

    fn on_jobs_key(&mut self, key: KeyEvent) {
        let n = self.jobs.data.as_ref().map(|l| l.len()).unwrap_or(0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 {
                    self.job_selected = (self.job_selected + 1).min(n - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.job_selected = self.job_selected.saturating_sub(1);
            }
            KeyCode::Char('c') | KeyCode::Delete => self.cancel_selected_job(),
            _ => {}
        }
    }

    fn cancel_selected_job(&mut self) {
        let Some(job) = self
            .jobs
            .data
            .as_ref()
            .and_then(|l| l.get(self.job_selected))
            .cloned()
        else {
            self.set_status("no job selected", true);
            return;
        };
        let id = job.job_id.clone();
        let client = self.client.clone();
        let tx = self.tx.clone();
        let msg = format!("cancelled job {id}");
        self.set_status(format!("cancel job {id}…"), false);
        self.spawn_call(async move {
            let res = client
                .cancel_job(&id)
                .await
                .map(|_| msg)
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                label: "job".to_string(),
                result: res,
            }));
        });
    }

    /// Detail mode: `esc`/`q` returns to the panel. Panel switching stays live,
    /// so a detail view never traps the user.
    fn on_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                self.detail = None;
                self.mode = InputMode::Normal;
            }
            KeyCode::Tab => {
                let i = self.panel.index();
                self.panel = Panel::ALL[(i + 1) % Panel::ALL.len()];
                self.detail = None;
                self.mode = InputMode::Normal;
                self.refresh_panel();
            }
            KeyCode::BackTab => {
                let i = self.panel.index();
                self.panel = Panel::ALL[(i + Panel::ALL.len() - 1) % Panel::ALL.len()];
                self.detail = None;
                self.mode = InputMode::Normal;
                self.refresh_panel();
            }
            KeyCode::Char(c @ '1'..='8') => {
                self.panel = Panel::ALL[(c as usize) - ('1' as usize)];
                self.detail = None;
                self.mode = InputMode::Normal;
                self.refresh_panel();
            }
            _ => {}
        }
    }

    /// `d` on the Nodes/Dashboard panels — full-screen worker detail.
    pub(crate) fn node_describe(&mut self) {
        let Some(w) = self
            .cluster
            .data
            .as_ref()
            .and_then(|c| c.workers.get(self.node_selected))
            .cloned()
        else {
            self.set_status("no worker selected", true);
            return;
        };

        let r = crate::ui::ratio(w.fs_used, w.capacity.max(1));
        let free = (w.capacity - w.fs_used).max(0);
        let view = DetailView {
            title: "worker".to_string(),
            subject: w.address.clone(),
            kind: DetailKind::Worker,
            headline: Some(("worker".to_string(), w.address.clone())),
            rows: vec![
                ("endpoint".into(), w.address.clone()),
                ("ip".into(), w.ip.clone()),
                ("hostname".into(), w.hostname.clone()),
                ("rpc port".into(), w.rpc_port.to_string()),
                ("web port".into(), w.web_port.to_string()),
                ("worker id".into(), w.worker_id.to_string()),
                ("state".into(), w.status.clone()),
                ("version".into(), w.version.clone()),
                ("weight".into(), w.weight.to_string()),
                ("capacity".into(), crate::ui::human_bytes(w.capacity)),
                ("used".into(), crate::ui::human_bytes(w.fs_used)),
                ("free".into(), crate::ui::human_bytes(free)),
                (
                    "occupancy".into(),
                    format!("{:.1}% (disk, not CPU load)", r * 100.0),
                ),
                ("blocks".into(), w.block_num.to_string()),
            ],
            blocks: Vec::new(),
            worker_blocks: Vec::new(),
            storage: w.storage.clone(),
            loading: false,
            error: None,
        };

        self.detail = Some(view);
        self.mode = InputMode::Detail;
    }

    fn on_mounts_key(&mut self, key: KeyEvent) {
        let n = self.mounts.data.as_ref().map(|l| l.len()).unwrap_or(0);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 {
                    self.mount_selected = (self.mount_selected + 1).min(n - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.mount_selected = self.mount_selected.saturating_sub(1);
            }
            KeyCode::Char('n') => {
                let cwd = self.files.cwd.clone();
                self.mount_form = Some(MountForm::new(&cwd));
                self.mode = InputMode::Mount;
            }
            KeyCode::Char('u') => self.request_unmount(),
            _ => {}
        }
    }

    fn request_unmount(&mut self) {
        if let Some(m) = self
            .mounts
            .data
            .as_ref()
            .and_then(|l| l.get(self.mount_selected))
        {
            let path = m.cv_path.clone();
            let ufs = m.ufs_path.clone();
            self.confirm = Some(PendingConfirm {
                action: PendingAction::Umount { path: path.clone() },
                title: "unmount".to_string(),
                target: path,
                lines: vec![
                    format!("ufs        {ufs}"),
                    "cached data on this mount becomes unreachable until it is".to_string(),
                    "mounted again; the ufs copy itself is not touched.".to_string(),
                ],
            });
            self.mode = InputMode::Confirm;
        } else {
            self.set_status("no mount selected", true);
        }
    }

    fn on_mount_key(&mut self, key: KeyEvent) {
        let Some(form) = self.mount_form.as_mut() else {
            self.mode = InputMode::Normal;
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.mount_form = None;
                self.mode = InputMode::Normal;
                self.set_status("mount cancelled", false);
            }
            KeyCode::Tab => form.focus = (form.focus + 1) % MOUNT_FIELDS,
            KeyCode::BackTab => form.focus = (form.focus + MOUNT_FIELDS - 1) % MOUNT_FIELDS,
            KeyCode::Backspace => {
                if let Some(f) = form.field_mut() {
                    f.pop();
                }
            }
            KeyCode::Left => match form.focus {
                2 => form.write_type = form.write_type.next(),
                3 => form.storage_type = form.storage_type.prev(),
                _ => {}
            },
            KeyCode::Right => match form.focus {
                2 => form.write_type = form.write_type.next(),
                3 => form.storage_type = form.storage_type.next(),
                _ => {}
            },
            KeyCode::Char(' ') => match form.focus {
                2 => form.write_type = form.write_type.next(),
                3 => form.storage_type = form.storage_type.next(),
                5 => form.auto_cache = !form.auto_cache,
                6 => form.read_verify_ufs = !form.read_verify_ufs,
                _ => {
                    if let Some(f) = form.field_mut() {
                        f.push(' ');
                    }
                }
            },
            KeyCode::Char(c) => {
                if let Some(f) = form.field_mut() {
                    f.push(c);
                }
            }
            KeyCode::Enter => self.submit_mount(),
            _ => {}
        }
    }

    fn submit_mount(&mut self) {
        let Some(form) = self.mount_form.as_ref() else {
            return;
        };

        let ufs = form.ufs_path.trim().to_string();
        let cv = normalize_path(form.cv_path.trim(), "/");
        if ufs.is_empty() {
            if let Some(f) = self.mount_form.as_mut() {
                f.error = Some("ufs path is required (e.g. s3://bucket/prefix or file:///path)".into());
            }
            return;
        }
        if !ufs.contains("://") {
            if let Some(f) = self.mount_form.as_mut() {
                f.error = Some("ufs path needs a scheme, e.g. s3://, file://, oss://".into());
            }
            return;
        }
        let ttl_ms = match form.ttl.trim().parse::<i64>() {
            Ok(v) if v >= 0 => v * 1000,
            _ => {
                if let Some(f) = self.mount_form.as_mut() {
                    f.error = Some("ttl must be a non-negative number of seconds".into());
                }
                return;
            }
        };

        let opts = curvine_model::MountOptions::builder()
            .write_type(match form.write_type {
                MountWriteMode::CacheMode => curvine_model::WriteType::CacheMode,
                MountWriteMode::FsMode => curvine_model::WriteType::FsMode,
            })
            .storage_type(form.storage_type.to_model())
            .ttl_ms(ttl_ms)
            .ttl_action(match form.write_type {
                MountWriteMode::CacheMode => curvine_model::TtlAction::Delete,
                MountWriteMode::FsMode => curvine_model::TtlAction::Free,
            })
            .read_verify_ufs(form.read_verify_ufs)
            .auto_cache(form.auto_cache)
            .build();

        self.mount_form = None;
        self.mode = InputMode::Normal;

        let client = self.client.clone();
        let tx = self.tx.clone();
        let label = "mount".to_string();
        let msg = format!("mounted {ufs} -> {cv}");
        self.spawn_call(async move {
            let res = client
                .mount(&ufs, &cv, opts)
                .await
                .map(|_| msg)
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                label,
                result: res,
            }));
        });
    }

    fn on_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                self.confirm = None;
                self.mode = InputMode::Normal;
                self.set_status("cancelled", false);
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                let Some(pending) = self.confirm.take() else {
                    self.mode = InputMode::Normal;
                    return;
                };
                self.mode = InputMode::Normal;
                if matches!(
                    pending.action,
                    PendingAction::Delete { .. } | PendingAction::Free { .. }
                ) {
                    self.files.marked.clear();
                }
                self.run_pending(pending.action);
            }
            _ => {}
        }
    }

    /// Execute a confirmed destructive action.
    fn run_pending(&mut self, action: PendingAction) {
        let client = self.client.clone();
        let tx = self.tx.clone();

        let (label, msg) = match &action {
            PendingAction::Delete { paths, recursive } => (
                "delete".to_string(),
                if paths.len() == 1 {
                    if *recursive {
                        format!("removed tree {}", paths[0])
                    } else {
                        format!("removed {}", paths[0])
                    }
                } else {
                    format!("removed {} items", paths.len())
                },
            ),
            PendingAction::Umount { path } => ("umount".to_string(), format!("unmounted {path}")),
            PendingAction::Free { paths, .. } => (
                if paths.len() == 1 { "free".to_string() } else { "free batch".to_string() },
                if paths.len() == 1 {
                    format!("freed {}", paths[0])
                } else {
                    format!("freed {} items", paths.len())
                },
            ),
        };

        self.spawn_call(async move {
            let res = async move {
                match action {
                    PendingAction::Delete { paths, recursive } => {
                        for path in &paths {
                            client.delete(path, recursive).await.map_err(|e| e.to_string())?;
                        }
                    }
                    PendingAction::Umount { path } => {
                        client.umount(&path).await.map_err(|e| e.to_string())?;
                    }
                    PendingAction::Free { paths, recursive } => {
                        for path in &paths {
                            client.free(path, recursive).await.map_err(|e| e.to_string())?;
                        }
                    }
                }
                Ok::<_, String>(())
            }
            .await;
            let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                label,
                result: res.map(|_| msg),
            }));
        });
    }

    fn on_describe_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
            self.describe = None;
            self.mode = InputMode::Normal;
        }
    }

    fn on_fsck_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Char('s') => self.start_fsck(),
            KeyCode::Char('f') => {
                // Scan the directory currently open in the file browser.
                self.fsck_path = self.files.cwd.clone();
                self.set_status(format!("fsck target: {}", self.fsck_path), false);
            }
            _ => {}
        }
    }

    fn on_bench_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Char('b') => self.start_bench(),
            KeyCode::Char('p') => self.bench_toggle_profile(),
            KeyCode::Char('t') => self.bench_toggle_threads(),
            KeyCode::Char('d') => self.bench_toggle_duration(),
            _ => {}
        }
    }

    fn bench_toggle_profile(&mut self) {
        self.bench_opts.profile = match self.bench_opts.profile {
            curvine_bench::BenchProfile::Basic => curvine_bench::BenchProfile::Deep,
            curvine_bench::BenchProfile::Deep => curvine_bench::BenchProfile::Basic,
        };
    }

    fn bench_toggle_threads(&mut self) {
        self.bench_opts.threads = match self.bench_opts.threads {
            1 => 2,
            2 => 4,
            4 => 8,
            _ => 1,
        };
    }

    fn bench_toggle_duration(&mut self) {
        self.bench_opts.duration_ms = match self.bench_opts.duration_ms {
            Some(5_000) => Some(10_000),
            Some(10_000) => Some(30_000),
            Some(30_000) => Some(60_000),
            _ => Some(5_000),
        };
    }

    fn on_panel_key(&mut self, key: KeyEvent) {
        if self.panel == Panel::Transfers {
            return self.on_transfers_key(key);
        }
        if self.panel == Panel::Mounts {
            return self.on_mounts_key(key);
        }
        if self.panel == Panel::Fsck {
            return self.on_fsck_key(key);
        }
        if self.panel == Panel::Bench {
            return self.on_bench_key(key);
        }
        if self.panel == Panel::Jobs {
            return self.on_jobs_key(key);
        }
        if matches!(self.panel, Panel::Dashboard | Panel::Nodes) {
            return self.on_nodes_key(key);
        }
        if self.panel == Panel::Files {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => self.files.move_down(),
                KeyCode::Up | KeyCode::Char('k') => self.files.move_up(),
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    self.files_enter()
                }
                KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                    self.files_up()
                }
                KeyCode::Char('/') => {
                    self.mode = InputMode::Filter;
                    self.files.filter.clear();
                }
                KeyCode::Char(' ') => {
                    if let Some(path) = self.files.selected_path() {
                        self.files.toggle_mark(path);
                    }
                }
                KeyCode::Char('a') => {
                    self.files.mark_all_visible();
                    let n = self.files.marked.len();
                    self.set_status(format!("marked {n} item(s)"), false);
                }
                KeyCode::Char('u') => self.clear_marks(),
                KeyCode::Char('.') => {
                    self.files.show_hidden = !self.files.show_hidden;
                    self.files.selected = 0;
                }
                KeyCode::Delete => self.files_request_delete(false),
                KeyCode::Char('D') => self.files_request_delete(true),
                KeyCode::Char('d') => self.files_describe(),
                KeyCode::Char('m') => self.files_mkdir(),
                KeyCode::Char('F') => self.files_request_free(false),
                KeyCode::Char('p') => self.open_form(TransferKind::Upload),
                KeyCode::Char('g') => self.open_form(TransferKind::Download),
                _ => {}
            }
        }
    }

    fn on_transfers_key(&mut self, key: KeyEvent) {
        let n = self.transfers.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 {
                    self.transfer_selected = (self.transfer_selected + 1).min(n - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.transfer_selected = self.transfer_selected.saturating_sub(1);
            }
            KeyCode::Delete => self.cancel_selected_transfer(),
            KeyCode::Char('c') => self.clear_finished_transfers(),
            KeyCode::Char('p') => self.open_form(TransferKind::Upload),
            KeyCode::Char('g') => self.open_form(TransferKind::Download),
            _ => {}
        }
    }

    pub(crate) fn cancel_selected_transfer(&mut self) {
        let Some(t) = self.transfers.get_mut(self.transfer_selected) else {
            return;
        };
        if t.state.is_terminal() {
            self.set_status("transfer already finished", true);
            return;
        }
        t.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        t.state = TransferState::Cancelled;
        let id = t.id;
        self.set_status(format!("transfer #{id} cancel requested"), false);
    }

    fn clear_finished_transfers(&mut self) {
        let before = self.transfers.len();
        self.transfers.retain(|t| !t.state.is_terminal());
        let removed = before - self.transfers.len();
        if self.transfers.len() <= self.transfer_selected {
            self.transfer_selected = self.transfers.len().saturating_sub(1);
        }
        self.set_status(format!("cleared {removed} finished transfer(s)"), false);
    }

    fn open_form(&mut self, kind: TransferKind) {
        if self.mode == InputMode::Transfer {
            return;
        }
        let cwd = self.files.cwd.clone();
        self.form = Some(match kind {
            TransferKind::Upload => TransferForm::upload(&cwd),
            TransferKind::Download => TransferForm::download(&cwd),
        });
        self.mode = InputMode::Transfer;
    }

    fn close_form(&mut self) {
        self.form = None;
        self.mode = InputMode::Normal;
    }

    fn on_form_key(&mut self, key: KeyEvent) {
        let Some(form) = self.form.as_mut() else {
            self.mode = InputMode::Normal;
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_form(),
            KeyCode::Tab | KeyCode::BackTab => form.focus = 1 - form.focus,
            KeyCode::Backspace => {
                form.field_mut().pop();
            }
            KeyCode::Char(c) => form.field_mut().push(c),
            KeyCode::Enter => self.submit_form(),
            _ => {}
        }
    }

    fn submit_form(&mut self) {
        let Some(form) = self.form.as_ref() else { return };
        let (local, remote) = (form.local.trim().to_string(), form.remote.trim().to_string());
        if local.is_empty() || remote.is_empty() {
            if let Some(f) = self.form.as_mut() {
                f.error = Some("both fields are required".to_string());
            }
            return;
        }
        let kind = form.kind;
        let cwd = self.files.cwd.clone();
        self.close_form();

        // Resolve a bare filename against the browsing directory so the form
        // stays terse for the common case.
        let remote = crate::app::normalize_path(&remote, &cwd);
        let local_path = if std::path::Path::new(&local).is_absolute() {
            std::path::PathBuf::from(&local)
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join(&local)
        };

        self.start_transfer(kind, local_path, remote);
    }

    /// Queue and spawn a transfer, appending it to the Transfers panel.
    pub fn start_transfer(
        &mut self,
        kind: TransferKind,
        local: std::path::PathBuf,
        remote: String,
    ) {
        let id = self.next_transfer_id;
        self.next_transfer_id += 1;

        let cancel = new_cancel_flag();
        let total = match kind {
            TransferKind::Upload => std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0),
            TransferKind::Download => 0,
        };

        self.transfers.push(TransferEntry {
            id,
            kind,
            local: local.display().to_string(),
            remote: remote.clone(),
            total,
            transferred: 0,
            speed: 0,
            state: TransferState::Queued,
            error: None,
            cancel: cancel.clone(),
        });
        self.transfer_selected = self.transfers.len() - 1;
        self.panel = Panel::Transfers;

        spawn_transfer(
            self.client.clone(),
            self.tx.clone(),
            id,
            kind,
            local.clone(),
            remote.clone(),
            cancel,
        );

        let what = match kind {
            TransferKind::Upload => format!("up {} -> {remote}", local.display()),
            TransferKind::Download => format!("down {remote} -> {}", local.display()),
        };
        self.set_status(format!("transfer #{id}: {what}"), false);
    }

    fn files_enter(&mut self) {
        let Some(entry) = self.files.selected_entry().cloned() else {
            return;
        };
        if entry.is_dir {
            self.files.cwd = join_path(&self.files.cwd, &entry.name);
            self.files.selected = 0;
            self.files.filter.clear();
            self.refresh_files();
        }
    }

    fn files_up(&mut self) {
        if self.files.cwd == "/" {
            return;
        }
        self.files.cwd = parent_path(&self.files.cwd);
        self.files.selected = 0;
        self.files.filter.clear();
        self.refresh_files();
    }

    fn files_mkdir(&mut self) {
        let name = format!("newdir-{}", chrono::Local::now().format("%H%M%S"));
        let full = join_path(&self.files.cwd, &name);
        self.spawn_op("mkdir", {
            let client = self.client.clone();
            let tx = self.tx.clone();
            let label = "mkdir".to_string();
            let msg = format!("created {full}");
            let full = full.clone();
            async move {
                let res = client
                    .mkdir(&full, true)
                    .await
                    .map(|_| msg)
                    .map_err(|e| e.to_string());
                let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                    label,
                    result: res,
                }));
            }
        });
    }

    /// Ask before deleting. Deleting a directory always requires `-r`, so the
    /// prompt states exactly what will be removed. When multiple entries are
    /// marked, one confirmation covers the whole batch.
    fn files_request_delete(&mut self, recursive: bool) {
        if !self.files.marked.is_empty() {
            self.request_batch(false);
            return;
        }
        let Some(entry) = self.files.selected_entry().cloned() else {
            self.set_status("nothing selected", true);
            return;
        };
        if entry.is_dir && !recursive {
            self.set_status(
                format!("{} is a directory — press D to delete it recursively", entry.name),
                true,
            );
            return;
        }

        let path = join_path(&self.files.cwd, &entry.name);
        let mut lines = vec![
            format!("size       {}", if entry.is_dir { "directory".to_string() } else { crate::ui::human_bytes(entry.len) }),
            format!("owner      {}:{}", entry.owner, entry.group),
        ];
        if !entry.is_dir {
            lines.push(format!("replicas   {}", entry.replicas));
        }
        lines.push(String::new());
        lines.push(if entry.is_dir {
            "this removes the directory and everything beneath it.".to_string()
        } else {
            "this removes the file and all of its block replicas.".to_string()
        });

        self.confirm = Some(PendingConfirm {
            action: PendingAction::Delete {
                paths: vec![path.clone()],
                recursive,
            },
            title: if recursive { "delete (recursive)" } else { "delete" }.to_string(),
            target: path,
            lines,
        });
        self.mode = InputMode::Confirm;
    }

    fn files_request_free(&mut self, recursive: bool) {
        if !self.files.marked.is_empty() {
            self.request_batch(true);
            return;
        }
        let Some(entry) = self.files.selected_entry().cloned() else {
            self.set_status("nothing selected", true);
            return;
        };
        let path = join_path(&self.files.cwd, &entry.name);
        self.confirm = Some(PendingConfirm {
            action: PendingAction::Free {
                paths: vec![path.clone()],
                recursive,
            },
            title: "free cached blocks".to_string(),
            target: path,
            lines: vec![
                "drops the Curvine block replicas while keeping the entry.".to_string(),
                "re-reading the path pulls it back from the mounted ufs.".to_string(),
            ],
        });
        self.mode = InputMode::Confirm;
    }

    /// Batch confirmation for delete (`free == false`) or free.
    fn request_batch(&mut self, free: bool) {
        let paths = self.files.marked.clone();
        let n = paths.len();
        let what = if free { "free" } else { "delete" };
        let mut lines = vec![
            format!("count      {n} marked items"),
            format!("scope      every marked path under {}", self.files.cwd),
            String::new(),
            format!("{what} {n} marked items?"),
        ];
        if !free {
            lines.push("this removes the entries and all of their block replicas.".to_string());
        } else {
            lines.push("cached blocks are dropped; the entries themselves stay.".to_string());
        }
        self.confirm = Some(PendingConfirm {
            action: if free {
                PendingAction::Free { paths, recursive: true }
            } else {
                PendingAction::Delete { paths, recursive: true }
            },
            title: if free { "free marked items" } else { "delete marked items" }.to_string(),
            target: format!("{n} marked items"),
            lines,
        });
        self.mode = InputMode::Confirm;
    }

    /// `d` — full-screen detail for a file, describe popup for a directory.
    fn files_describe(&mut self) {
        let Some(entry) = self.files.selected_entry().cloned() else {
            self.set_status("nothing selected", true);
            return;
        };
        let path = join_path(&self.files.cwd, &entry.name);

        let base_rows: Vec<(String, String)> = vec![
            ("path".into(), path.clone()),
            (
                "mode".into(),
                format!("{} ({:04o})", crate::ui::format_mode(entry.mode, entry.is_dir), entry.mode),
            ),
            ("owner".into(), format!("{}:{}", entry.owner, entry.group)),
            (
                "size".into(),
                if entry.is_dir {
                    "—".to_string()
                } else {
                    format!("{} ({} bytes)", crate::ui::human_bytes(entry.len), entry.len)
                },
            ),
            ("modified".into(), crate::ui::format_mtime(entry.mtime)),
            (
                "replicas".into(),
                format!("{} (configured factor)", entry.replicas),
            ),
        ];

        if entry.is_dir {
            let mut view = DescribeView {
                title: format!("describe {}", entry.name),
                subject: path.clone(),
                headline: Some(("directory".to_string(), path)),
                rows: base_rows,
                worker_blocks: Vec::new(),
                blocks: Vec::new(),
                loading: false,
            };
            view.rows.push(("kind".into(), "directory".to_string()));
            self.describe = Some(view);
            self.mode = InputMode::Describe;
            return;
        }

        let mut rows = base_rows;
        rows.push(("blocks".into(), "loading…".to_string()));
        self.detail = Some(DetailView {
            title: format!("file {}", entry.name),
            subject: path.clone(),
            kind: DetailKind::File,
            headline: Some(("file".to_string(), path.clone())),
            rows,
            blocks: Vec::new(),
            worker_blocks: Vec::new(),
            storage: Vec::new(),
            loading: true,
            error: None,
        });
        self.mode = InputMode::Detail;

        let client = self.client.clone();
        let tx = self.tx.clone();
        let describe_path = path.clone();
        self.spawn_call(async move {
            let res = client
                .file_blocks(&describe_path)
                .await
                .map(|blocks| {
                    // Flatten to one row per replica; the UI aggregates.
                    let mut rows = Vec::new();
                    for b in blocks {
                        for r in &b.replicas {
                            rows.push(crate::event::BlockRow {
                                block_id: format!("{}", b.block_id),
                                offset: b.offset,
                                len: b.len,
                                worker: r.address.clone(),
                                available: r.available,
                                state: r.state.clone(),
                            });
                        }
                    }
                    rows
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::Describe {
                path: describe_path,
                blocks: res,
            }));
        });
    }

    fn spawn_op<F>(&mut self, label: &str, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.set_status(format!("{label}..."), false);
        self.client.runtime().spawn(fut);
    }

    fn spawn_call<F>(&mut self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.client.runtime().spawn(fut);
    }

    fn on_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.command.clear();
                self.command_history_pos = None;
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.command);
                self.mode = InputMode::Normal;
                self.command_history_pos = None;
                if !line.trim().is_empty() {
                    self.command_history.push(line.clone());
                    if self.command_history.len() > 20 {
                        self.command_history.remove(0);
                    }
                }
                self.execute_command(&line);
            }
            KeyCode::Tab => self.complete_command(),
            KeyCode::Up => self.command_history_prev(),
            KeyCode::Down => self.command_history_next(),
            KeyCode::Backspace => {
                self.command.pop();
                self.command_history_pos = None;
            }
            KeyCode::Char(c) => {
                self.command.push(c);
                self.command_history_pos = None;
            }
            _ => {}
        }
    }

    /// Tab completes to the first prefix match, adding the trailing space so
    /// the next keystroke starts on arguments.
    fn complete_command(&mut self) {
        if let Some(first) = matching_commands(&self.command).first() {
            self.command = format!("{first} ");
        }
    }

    fn command_history_prev(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        let pos = match self.command_history_pos {
            Some(0) => 0,
            Some(i) => i - 1,
            None => self.command_history.len() - 1,
        };
        self.command_history_pos = Some(pos);
        self.command = self.command_history[pos].clone();
    }

    fn command_history_next(&mut self) {
        let Some(pos) = self.command_history_pos else {
            return;
        };
        if pos + 1 < self.command_history.len() {
            self.command_history_pos = Some(pos + 1);
            self.command = self.command_history[pos + 1].clone();
        } else {
            // Walking past the newest entry returns to an empty prompt.
            self.command_history_pos = None;
            self.command.clear();
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.files.filter.clear();
                self.files.selected = 0;
            }
            KeyCode::Enter => self.mode = InputMode::Normal,
            KeyCode::Backspace => {
                self.files.filter.pop();
                self.files.selected = 0;
            }
            KeyCode::Char(c) => {
                self.files.filter.push(c);
                self.files.selected = 0;
            }
            _ => {}
        }
    }

    /// `:` command line — a subset of the CLI surface, extended incrementally.
    fn execute_command(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "q" | "quit" | "exit" => self.should_quit = true,
            "cd" => {
                let target = parts.get(1).copied().unwrap_or("/");
                self.panel = Panel::Files;
                self.files.cwd = normalize_path(target, &self.files.cwd);
                self.files.selected = 0;
                self.files.filter.clear();
                self.refresh_files();
            }
            "ls" => {
                self.panel = Panel::Files;
                if let Some(p) = parts.get(1) {
                    self.files.cwd = normalize_path(p, &self.files.cwd);
                    self.files.selected = 0;
                }
                self.refresh_files();
            }
            "mkdir" => match parts.get(1) {
                Some(p) => {
                    let full = normalize_path(p, &self.files.cwd);
                    let label = "mkdir".to_string();
                    let msg = format!("created {full}");
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.spawn_call(async move {
                        let res = client
                            .mkdir(&full, true)
                            .await
                            .map(|_| msg)
                            .map_err(|e| e.to_string());
                        let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                            label,
                            result: res,
                        }));
                    });
                }
                None => self.set_status("usage: mkdir <path>", true),
            },
            "rm" => {
                let args: Vec<&str> = parts[1..].iter().copied().filter(|a| !a.starts_with('-')).collect();
                let recursive = parts.iter().any(|a| *a == "-r" || *a == "-R");
                match args.first() {
                    Some(p) => {
                        let full = normalize_path(p, &self.files.cwd);
                        let label = "rm".to_string();
                        let msg = format!("removed {full}");
                        let client = self.client.clone();
                        let tx = self.tx.clone();
                        self.spawn_call(async move {
                            let res = client
                                .delete(&full, recursive)
                                .await
                                .map(|_| msg)
                                .map_err(|e| e.to_string());
                            let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                                label,
                                result: res,
                            }));
                        });
                    }
                    None => self.set_status("usage: rm [-r] <path>", true),
                }
            }
            "mv" => match (parts.get(1), parts.get(2)) {
                (Some(a), Some(b)) => {
                    let src = normalize_path(a, &self.files.cwd);
                    let dst = normalize_path(b, &self.files.cwd);
                    let label = "mv".to_string();
                    let msg = format!("{src} -> {dst}");
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    self.spawn_call(async move {
                        let res = client
                            .rename(&src, &dst)
                            .await
                            .map(|_| msg)
                            .map_err(|e| e.to_string());
                        let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                            label,
                            result: res,
                        }));
                    });
                }
                _ => self.set_status("usage: mv <src> <dst>", true),
            },
            "load" => match parts.get(1) {
                Some(p) => {
                    let label = "load".to_string();
                    let path = p.to_string();
                    let msg = format!("submitted load for {path}");
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    if !self.tracked_jobs.contains(&path) {
                        self.tracked_jobs.push(path.clone());
                    }
                    self.spawn_call(async move {
                        let res = client
                            .submit_load(&path)
                            .await
                            .map(|_| msg)
                            .map_err(|e| e.to_string());
                        let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                            label,
                            result: res,
                        }));
                    });
                }
                None => self.set_status("usage: load <path>", true),
            },
            "put" => match (parts.get(1), parts.get(2)) {
                (Some(local), Some(remote)) => {
                    let remote = normalize_path(remote, &self.files.cwd);
                    self.start_transfer(
                        TransferKind::Upload,
                        std::path::PathBuf::from(local),
                        remote,
                    );
                }
                _ => self.set_status("usage: put <local> <remote>", true),
            },
            "get" => match (parts.get(1), parts.get(2)) {
                (Some(remote), Some(local)) => {
                    let remote = normalize_path(remote, &self.files.cwd);
                    self.start_transfer(
                        TransferKind::Download,
                        std::path::PathBuf::from(local),
                        remote,
                    );
                }
                _ => self.set_status("usage: get <remote> <local>", true),
            },
            "cancel" => {
                self.transfer_selected = 0;
                self.panel = Panel::Transfers;
                self.cancel_selected_transfer();
            }
            "mount" => {
                if parts.len() >= 3 {
                    let ufs = parts[1].to_string();
                    let cv = normalize_path(parts[2], &self.files.cwd);
                    let opts = curvine_model::MountOptions::builder()
                        .write_type(curvine_model::WriteType::CacheMode)
                        .ttl_action(curvine_model::TtlAction::Delete)
                        .build();
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    let label = "mount".to_string();
                    let msg = format!("mounted {ufs} -> {cv}");
                    self.panel = Panel::Mounts;
                    self.spawn_call(async move {
                        let res = client
                            .mount(&ufs, &cv, opts)
                            .await
                            .map(|_| msg)
                            .map_err(|e| e.to_string());
                        let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                            label,
                            result: res,
                        }));
                    });
                } else {
                    // No arguments: open the form instead of erroring.
                    let cwd = self.files.cwd.clone();
                    self.mount_form = Some(MountForm::new(&cwd));
                    self.mode = InputMode::Mount;
                    self.panel = Panel::Mounts;
                }
            }
            "umount" | "unmount" => match parts.get(1) {
                Some(p) => {
                    let cv = normalize_path(p, &self.files.cwd);
                    let client = self.client.clone();
                    let tx = self.tx.clone();
                    let label = "umount".to_string();
                    let msg = format!("unmounted {cv}");
                    self.panel = Panel::Mounts;
                    self.spawn_call(async move {
                        let res = client
                            .umount(&cv)
                            .await
                            .map(|_| msg)
                            .map_err(|e| e.to_string());
                        let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                            label,
                            result: res,
                        }));
                    });
                }
                None => self.set_status("usage: umount <cv-path>", true),
            },
            "fsck" => {
                if let Some(p) = parts.get(1) {
                    self.fsck_path = normalize_path(p, &self.files.cwd);
                }
                self.panel = Panel::Fsck;
                self.start_fsck();
            }
            "bench" => {
                self.panel = Panel::Bench;
                self.start_bench();
            }
            "refresh" | "reload" => self.refresh_panel(),
            "panel" => match parts.get(1).and_then(|p| match *p {
                "dashboard" => Some(Panel::Dashboard),
                "files" => Some(Panel::Files),
                "nodes" => Some(Panel::Nodes),
                "mounts" => Some(Panel::Mounts),
                "jobs" => Some(Panel::Jobs),
                "transfers" | "xfer" => Some(Panel::Transfers),
                "fsck" => Some(Panel::Fsck),
                "bench" => Some(Panel::Bench),
                _ => None,
            }) {
                Some(p) => {
                    self.panel = p;
                    self.refresh_panel();
                }
                None => self.set_status(
                    "usage: panel <dashboard|files|nodes|mounts|jobs|transfers|fsck|bench>",
                    true,
                ),
            },
            "help" => self.help_visible = true,
            other => self.set_status(format!("unknown command: {other}"), true),
        }
    }
}

/// Every command accepted by the `:` prompt, in help order.
pub const COMMANDS: [&str; 17] = [
    "ls",
    "cd",
    "mkdir",
    "rm",
    "mv",
    "put",
    "get",
    "mount",
    "umount",
    "load",
    "fsck",
    "bench",
    "panel",
    "refresh",
    "help",
    "quit",
    "cancel",
];

/// Commands whose name starts with `input`, used by Tab completion and the
/// suggestion row above the footer.
pub fn matching_commands(input: &str) -> Vec<&'static str> {
    let needle = input.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return COMMANDS.to_vec();
    }
    COMMANDS
        .iter()
        .copied()
        .filter(|c| c.starts_with(&needle))
        .collect()
}

/// Group replicas by worker, then sort by worker endpoint so the distribution
/// reads as a stable table rather than block order.
pub(crate) fn aggregate_by_worker(rows: &[crate::event::BlockRow]) -> Vec<WorkerBlocks> {
    use std::collections::BTreeMap;

    let mut map: BTreeMap<String, WorkerBlocks> = BTreeMap::new();
    for r in rows {
        let entry = map.entry(r.worker.clone()).or_insert_with(|| WorkerBlocks {
            worker: r.worker.clone(),
            block_ids: Vec::new(),
            bytes: 0,
            available: 0,
            total: 0,
        });
        if !entry.block_ids.contains(&r.block_id) {
            entry.block_ids.push(r.block_id.clone());
            entry.bytes += r.len;
        }
        entry.total += 1;
        if r.available {
            entry.available += 1;
        }
    }

    let mut out: Vec<WorkerBlocks> = map.into_values().collect();
    for w in &mut out {
        w.block_ids.sort();
    }
    out
}

// ---------------------------------------------------------------- conversions

fn to_fs_entry(s: curvine_model::FileStatus) -> FsEntry {
    FsEntry {
        path: s.path.clone(),
        name: s.name.clone(),
        is_dir: s.is_dir,
        len: s.len,
        mtime: s.mtime,
        mode: s.mode,
        owner: s.owner.clone(),
        group: s.group.clone(),
        replicas: s.replicas,
    }
}

fn to_job_row(j: curvine_model::JobStatus) -> JobRow {
    // v0.6 reports byte progress (loaded_size / total_size) rather than file
    // counters, so the row shows bytes and derives the ratio from them.
    let total = j.progress.total_size.max(0) as u64;
    let loaded = j.progress.loaded_size.max(0) as u64;
    JobRow {
        job_id: j.job_id,
        state: format!("{:?}", j.state),
        source: j.source_path,
        target: j.target_path,
        progress: if total > 0 {
            (loaded as f64 / total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        },
        total_bytes: total,
        loaded_bytes: loaded,
    }
}

/// Menu titles name the selection; past ~24 cells they stop being scannable.
fn truncate_menu_title(mut title: String) -> String {
    if title.chars().count() > 24 {
        title = title.chars().take(23).collect::<String>() + "…";
    }
    title
}

pub fn join_path(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", base.trim_end_matches('/'), name)
    }
}

pub fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    }
}

/// Resolve `p` relative to `cwd`, collapsing `.` and `..`.
pub fn normalize_path(p: &str, cwd: &str) -> String {
    let raw = if p.starts_with('/') {
        p.to_string()
    } else {
        join_path(cwd, p)
    };
    let mut out: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}
