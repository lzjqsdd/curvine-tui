//! Application state and reducer.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::config::AppConfig;
use crate::event::{
    AppEvent, ClusterSnapshot, ConfigRow, FsEntry, JobRow, MountRow, TaskResult, WorkerRow,
};
use crate::rpc::ClusterClient;
use curvine_runtime::runtime::RpcRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Dashboard,
    Files,
    Nodes,
    Mounts,
    Jobs,
    Config,
}

impl Panel {
    pub const ALL: [Panel; 6] = [
        Panel::Dashboard,
        Panel::Files,
        Panel::Nodes,
        Panel::Mounts,
        Panel::Jobs,
        Panel::Config,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Dashboard => "Dashboard",
            Panel::Files => "Files",
            Panel::Nodes => "Nodes",
            Panel::Mounts => "Mounts",
            Panel::Jobs => "Jobs",
            Panel::Config => "Config",
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

    pub files: FilesState,
    pub cluster: Loadable<ClusterSnapshot>,
    pub mounts: Loadable<Vec<MountRow>>,
    pub jobs: Loadable<Vec<JobRow>>,
    pub config: Loadable<Vec<ConfigRow>>,

    pub log: Vec<String>,
    pub help_visible: bool,
}

impl App {
    pub fn new(
        cfg: AppConfig,
        client: ClusterClient,
        tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
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
            files: FilesState::default(),
            cluster: Loadable::default(),
            mounts: Loadable::default(),
            jobs: Loadable::default(),
            config: Loadable::default(),
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

    pub fn log(&mut self, msg: impl Into<String>) {
        let ts = chrono::Local::now().format("%H:%M:%S");
        self.log.push(format!("{ts} {}", msg.into()));
        if self.log.len() > 500 {
            self.log.remove(0);
        }
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
                            address: format!("{}", w.address),
                            status: format!("{:?}", w.status),
                            capacity: w.capacity,
                            available: w.available,
                            fs_used: w.fs_used,
                            block_num: w.block_num,
                            weight: w.weight,
                            version: w.software_version.clone(),
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
        self.client.runtime().spawn(async move {
            let res = client
                .jobs(50)
                .await
                .map(|list| list.into_iter().map(to_job_row).collect::<Vec<_>>())
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::Jobs(res)));
        });
    }

    pub fn refresh_config(&mut self) {
        self.config.begin();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.client.runtime().spawn(async move {
            let res = client
                .runtime_config(None)
                .await
                .map(|list| {
                    list.into_iter()
                        .map(|e| ConfigRow {
                            key: e.key,
                            value: e.effective_value_json,
                        })
                        .collect::<Vec<_>>()
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::Config(res)));
        });
    }

    pub fn refresh_panel(&mut self) {
        match self.panel {
            Panel::Dashboard => self.refresh_cluster(),
            Panel::Files => self.refresh_files(),
            Panel::Nodes => self.refresh_cluster(),
            Panel::Mounts => self.refresh_mounts(),
            Panel::Jobs => self.refresh_jobs(),
            Panel::Config => self.refresh_config(),
        }
    }

    // ---------------------------------------------------------------- reducer

    pub fn on_event(&mut self, evt: AppEvent) {
        match evt {
            AppEvent::Key(k) => self.on_key(k),
            AppEvent::Resize(_, _) => {}
            AppEvent::Mouse(_) => {}
            AppEvent::Tick => {
                // Auto-refresh the active data panel if stale.
                match self.panel {
                    Panel::Dashboard | Panel::Nodes => self.refresh_cluster(),
                    Panel::Jobs => self.refresh_jobs(),
                    _ => {}
                }
            }
            AppEvent::TaskDone(t) => self.on_task(t),
        }
    }

    fn on_task(&mut self, t: TaskResult) {
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
            TaskResult::Config(res) => match res {
                Ok(c) => self.config.ok(c),
                Err(e) => {
                    self.config.fail(e.clone());
                    self.set_status(format!("config: {e}"), true);
                }
            },
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
            KeyCode::Char(c @ '1'..='6') => {
                self.panel = Panel::ALL[(c as usize) - ('1' as usize)];
                self.refresh_panel();
            }
            KeyCode::Char('r') => self.refresh_panel(),
            _ => self.on_panel_key(key),
        }
    }

    fn on_panel_key(&mut self, key: KeyEvent) {
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
                KeyCode::Char('.') => {
                    self.files.show_hidden = !self.files.show_hidden;
                    self.files.selected = 0;
                }
                KeyCode::Char('d') => self.files_delete(false),
                KeyCode::Char('D') => self.files_delete(true),
                KeyCode::Char('m') => self.files_mkdir(),
                _ => {}
            }
        }
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

    fn files_delete(&mut self, recursive: bool) {
        let Some(entry) = self.files.selected_entry().cloned() else {
            return;
        };
        let path = join_path(&self.files.cwd, &entry.name);
        let label = "delete".to_string();
        let msg = format!("deleted {path}");
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.spawn_call(async move {
            let res = client
                .delete(&path, recursive)
                .await
                .map(|_| msg)
                .map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::TaskDone(TaskResult::OpDone {
                label,
                result: res,
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
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.command);
                self.mode = InputMode::Normal;
                self.execute_command(&line);
            }
            KeyCode::Backspace => {
                self.command.pop();
            }
            KeyCode::Char(c) => self.command.push(c),
            _ => {}
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
            "refresh" | "reload" => self.refresh_panel(),
            "panel" => match parts.get(1).and_then(|p| match *p {
                "dashboard" => Some(Panel::Dashboard),
                "files" => Some(Panel::Files),
                "nodes" => Some(Panel::Nodes),
                "mounts" => Some(Panel::Mounts),
                "jobs" => Some(Panel::Jobs),
                "config" => Some(Panel::Config),
                _ => None,
            }) {
                Some(p) => {
                    self.panel = p;
                    self.refresh_panel();
                }
                None => self.set_status(
                    "usage: panel <dashboard|files|nodes|mounts|jobs|config>",
                    true,
                ),
            },
            "help" => self.help_visible = true,
            other => self.set_status(format!("unknown command: {other}"), true),
        }
    }
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
    let progress = j.progress_string(false);
    JobRow {
        job_id: j.job_id,
        state: format!("{:?}", j.state),
        source: j.source_path,
        target: j.target_path,
        progress: parse_progress_pct(&progress),
        total_files: j.total_files,
        completed: j.completed_files,
    }
}

fn parse_progress_pct(s: &str) -> f64 {
    s.split('%')
        .next()
        .and_then(|n| n.trim().parse::<f64>().ok())
        .map(|v| (v / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0)
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
