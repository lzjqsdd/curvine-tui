//! UI tests against a live Curvine cluster.
//!
//! These drive the real `App` reducer — the same key handling and state
//! transitions the TUI uses — so menus, multi-select, batch operations, detail
//! views and the command line are exercised end to end rather than only through
//! the hermetic render tests.
//!
//! Skipped unless `CURVINE_TUI_LIVE=1` is set, so `cargo test` stays hermetic:
//!
//! ```sh
//! CURVINE_TUI_LIVE=1 cargo test --bin curvine-tui live_tests \
//!     -- --test-threads=1 --nocapture
//! ```
//!
//! Every test uses a unique path under `/data/live-<pid>-<name>` (or a unique
//! temp dir), so a rerun never collides with leftovers.

use std::path::{Path as StdPath, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use tokio::sync::mpsc;

use crate::app::{App, InputMode, MountForm, Panel};
use crate::config::{AppConfig, ConfigOverrides};
use crate::event::AppEvent;
use crate::rpc::ClusterClient;
use curvine_runtime::runtime::RpcRuntime;

/// Live tests share one cluster, so they must not interleave.
static LIVE_CLUSTER: Mutex<()> = Mutex::new(());

/// One client/runtime for the whole test binary. Dropping a runtime whose
/// background tasks are still winding down panics on shutdown, so the harness
/// keeps a single runtime alive for the process instead of one per test.
static CLUSTER: std::sync::OnceLock<(AppConfig, ClusterClient)> = std::sync::OnceLock::new();

fn cluster() -> &'static (AppConfig, ClusterClient) {
    CLUSTER.get_or_init(|| {
        let cfg = AppConfig::load(ConfigOverrides::default()).expect("config");
        let client = ClusterClient::connect(&cfg).expect("client");
        (cfg, client)
    })
}

fn live() -> bool {
    std::env::var("CURVINE_TUI_LIVE").as_deref() == Ok("1")
}

macro_rules! skip_unless_live {
    () => {
        if !live() {
            eprintln!("skipped: set CURVINE_TUI_LIVE=1 with a running cluster");
            return;
        }
    };
}

/// Unique namespace for one test run.
fn run_root(name: &str) -> String {
    format!("/data/live-{}-{name}", std::process::id())
}

/// `App` plus the event receiver its background tasks report to.
struct Live {
    app: App,
    rx: mpsc::UnboundedReceiver<AppEvent>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Live {
    fn start() -> Option<Self> {
        if !live() {
            return None;
        }
        let guard = LIVE_CLUSTER.lock().unwrap_or_else(|e| e.into_inner());
        let (cfg, base) = cluster();
        // A fresh client per test isolates the mount cache; the runtime stays
        // shared so it is never dropped mid-flight.
        let client = base.fresh(cfg).expect("client");
        let (tx, rx) = mpsc::unbounded_channel();
        // Reduced motion keeps the rendered values at their targets, so
        // assertions do not race a spring.
        let app = App::new(cfg.clone(), client, tx, true, 60);
        Some(Self {
            app,
            rx,
            _guard: guard,
        })
    }

    /// Drain task results and advance one frame.
    fn tick(&mut self) {
        while let Ok(evt) = self.rx.try_recv() {
            self.app.on_event(evt);
        }
        self.app
            .on_event(AppEvent::Frame(Duration::from_millis(16)));
    }

    /// Pump events until `cond` holds, or `timeout` elapses.
    fn wait_for(&mut self, timeout: Duration, mut cond: impl FnMut(&App) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.tick();
            if cond(&self.app) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn key(&mut self, code: KeyCode) {
        self.app
            .on_event(AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c));
        }
    }

    /// Run a `:` command and wait until the command line closes.
    fn command(&mut self, line: &str) {
        self.key(KeyCode::Char(':'));
        self.text(line);
        self.key(KeyCode::Enter);
    }

    fn select(&mut self, name: &str) {
        let pos = self.app.files.visible().iter().position(|e| e.name == name);
        if let Some(pos) = pos {
            self.app.files.selected = pos;
        } else {
            panic!(
                "entry {name:?} not listed in {}: {:?}",
                self.app.files.cwd,
                self.app
                    .files
                    .visible()
                    .iter()
                    .map(|e| e.name.clone())
                    .collect::<Vec<_>>()
            );
        }
    }

    fn render(&self, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut term = Terminal::new(backend).expect("terminal");
        term.draw(|f| crate::ui::render(f, &self.app))
            .expect("draw");
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// Block on a client call; used for setup/verification outside the reducer.
    fn block_on<T>(&self, fut: impl std::future::Future<Output = T>) -> T {
        self.app.client.runtime().block_on(fut)
    }

    fn wait_cluster(&mut self) {
        assert!(
            self.wait_for(Duration::from_secs(60), |a| {
                a.cluster
                    .data
                    .as_ref()
                    .map(|c| !c.workers.is_empty())
                    .unwrap_or(false)
            }),
            "cluster never reported a live worker; status: {}",
            self.app.status
        );
    }
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("curvine-tui-live-ui")
        .join(format!("{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Mountable UFS directory with a short path. The Mounts table truncates long
/// values, so a `/var/folders/...` temp path would not be assertable on screen.
fn mount_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/cvui-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mount dir");
    dir
}

/// Locate `cv` (the upstream CLI). Mount resync lives in the CLI, not in the
/// client library, so the harness uses it to prepare fs-mode mount fixtures.
fn cv_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CURVINE_TUI_CV") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let candidate = dir.join("cv");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn run_cv(args: &[&str]) -> std::process::Output {
    let bin = cv_bin().expect("cv binary");
    std::process::Command::new(bin)
        .arg("--master-addrs")
        .arg("localhost:8995")
        .args(args)
        .output()
        .expect("run cv")
}

fn script_file(dir: &StdPath, name: &str, bytes: usize) -> PathBuf {
    let path = dir.join(name);
    let payload: Vec<u8> = (0..bytes).map(|i| ((i * 31 + 7) % 256) as u8).collect();
    std::fs::write(&path, payload).expect("write payload");
    path
}

/// Create a unique directory with one file, so a test never depends on data
/// seeded outside the suite.
fn seed(live: &Live, name: &str, bytes: usize) -> (String, String) {
    let root = run_root(name);
    let local = script_file(&tmp_dir(name), "seed.bin", bytes);
    let remote = format!("{root}/seed.bin");
    let client = live.app.client.clone();
    let dir = root.clone();
    let dest = remote.clone();
    live.block_on(async move {
        client.mkdir(&dir, true).await.expect("mkdir");
        crate::transfer::upload_file(
            &client,
            &local,
            &dest,
            crate::transfer::new_cancel_flag(),
            |_, _, _| {},
        )
        .await
        .expect("upload seed file");
    });
    (root, remote)
}

fn cleanup(live: &Live, path: &str) {
    let client = live.app.client.clone();
    let path = path.to_string();
    let _ = live.block_on(async move { client.delete(&path, true).await });
}

// ------------------------------------------------------------------ scenarios

#[test]
fn live_header_files_and_menus_work_against_the_cluster() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    assert!(
        live.wait_for(Duration::from_secs(60), |a| a
            .cluster
            .data
            .as_ref()
            .map(|c| !c.workers.is_empty())
            .unwrap_or(false)
            && !a.files.load.loading),
        "cluster/listing did not load"
    );

    // Header: identity, cluster endpoint, live worker count, refresh timer.
    // The band is a single row.
    let out = live.render(120, 30);
    let header = out
        .lines()
        .find(|l| l.contains("curvine-tui"))
        .unwrap_or_default();
    assert!(header.contains("curvine-tui"), "header: {header:?}");
    assert!(header.contains("worker"), "header: {header:?}");
    assert!(header.contains('↻'), "header: {header:?}");
    assert!(
        header.contains(crate::ui::components::DOT),
        "header health dot missing: {header:?}"
    );
    let footer = out
        .lines()
        .find(|l| l.contains("x menu"))
        .unwrap_or_default();
    assert!(!footer.is_empty(), "footer hints missing:\n{out}");

    // Browse a directory this test created itself.
    let (root, remote) = seed(&live, "header", 64 * 1024);
    let name = remote.rsplit('/').next().unwrap().to_string();
    live.command(&format!("cd {root}"));
    assert!(
        live.wait_for(Duration::from_secs(60), |a| {
            a.files.cwd == root && !a.files.load.loading && a.files.visible().len() == 1
        }),
        "listing did not load: {}",
        live.app.status
    );
    let out = live.render(120, 30);
    assert!(out.contains(&root), "breadcrumb path missing:\n{out}");
    assert!(out.contains(&name), "seeded file missing:\n{out}");
    assert!(out.contains("repl"), "replica column missing:\n{out}");
    let crumb = out.lines().find(|l| l.contains("1/1")).unwrap_or_default();
    assert!(crumb.contains("1/1"), "visible/total missing:\n{out}");

    // Every panel opens a menu with at least one executable action.
    for panel in Panel::ALL {
        live.app.panel = panel;
        live.key(KeyCode::Char('x'));
        assert_eq!(
            live.app.mode,
            InputMode::Menu,
            "x did not open a menu on {panel:?}"
        );
        let menu = live.app.menu.as_ref().expect("menu state");
        assert!(!menu.items.is_empty(), "menu for {panel:?} has no actions");
        live.key(KeyCode::Esc);
        assert_eq!(live.app.mode, InputMode::Normal);
    }

    cleanup(&live, &root);
}

#[test]
fn live_file_detail_loads_blocks_and_distribution() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    let (root, remote) = seed(&live, "detail", 512 * 1024);
    let name = remote.rsplit('/').next().unwrap().to_string();
    live.command(&format!("cd {root}"));
    assert!(
        live.wait_for(Duration::from_secs(60), |a| {
            a.files.cwd == root && !a.files.load.loading && a.files.visible().len() == 1
        }),
        "listing did not load: {}",
        live.app.status
    );

    live.select(&name);
    live.key(KeyCode::Char('d'));
    assert_eq!(live.app.mode, InputMode::Detail);
    let opened = live.wait_for(Duration::from_secs(60), |a| {
        a.detail.as_ref().map(|d| !d.loading).unwrap_or(false)
    });
    assert!(opened, "block locations never loaded: {}", live.app.status);

    let view = live.app.detail.as_ref().expect("detail view");
    assert!(!view.blocks.is_empty(), "no replicas reported");
    assert!(!view.worker_blocks.is_empty(), "no worker distribution");
    assert!(
        view.rows
            .iter()
            .any(|(k, v)| k == "blocks" && v.contains("across")),
        "block summary not updated: {:?}",
        view.rows
    );
    let worker = view.worker_blocks[0].worker.clone();

    let out = live.render(120, 34);
    for expected in [
        "blocks by offset",
        "distribution by worker",
        &worker,
        "replica availability",
    ] {
        assert!(out.contains(expected), "detail missing {expected}:\n{out}");
    }

    live.key(KeyCode::Esc);
    assert!(live.app.detail.is_none(), "escape must close the detail");
    assert_eq!(live.app.mode, InputMode::Normal);

    cleanup(&live, &root);
}

#[test]
fn live_multi_select_marks_and_batch_deletes() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    let root = run_root("batch");
    let local = tmp_dir("batch");
    let files = ["a.bin", "b.bin"];
    for (idx, name) in files.iter().enumerate() {
        let src = script_file(&local, name, 64 * 1024);
        live.command(&format!("put {} {root}/{name}", src.display()));
        assert!(
            live.wait_for(Duration::from_secs(60), |a| a
                .transfers
                .iter()
                .filter(|t| t.state.is_terminal())
                .count()
                > idx),
            "upload of {name} did not finish: {}",
            live.app.status
        );
    }

    // Progress rows are real: the transfers list carries bytes and a terminal
    // state after the uploads above.
    assert!(
        live.app.transfers.iter().all(|t| t.total > 0),
        "transfers did not report sizes: {:?}",
        live.app
            .transfers
            .iter()
            .map(|t| (t.state, t.total))
            .collect::<Vec<_>>()
    );

    // Cancelling an already-finished transfer is rejected with a toast, not a
    // panic; clearing then removes the finished rows.
    live.app.panel = Panel::Transfers;
    live.app.transfer_selected = 0;
    live.key(KeyCode::Delete);
    let toast = live
        .app
        .toast
        .as_ref()
        .expect("finished-transfer cancel must raise a toast");
    assert!(
        toast.text.contains("already finished"),
        "expected a refusal, got {:?}",
        toast.text
    );
    live.key(KeyCode::Char('c'));
    assert!(live.app.transfers.is_empty(), "finished rows not cleared");

    // Mark both uploaded files.
    live.command(&format!("cd {root}"));
    assert!(
        live.wait_for(Duration::from_secs(30), |a| {
            a.files.cwd == root && !a.files.load.loading && a.files.visible().len() == 2
        }),
        "listing did not show both files: {}",
        live.app.status
    );
    live.select(files[0]);
    live.key(KeyCode::Char(' '));
    live.select(files[1]);
    live.key(KeyCode::Char(' '));
    assert_eq!(live.app.files.marked.len(), 2);
    assert!(
        live.render(120, 30).contains("2 marked"),
        "breadcrumb missing the mark count"
    );

    // x → Del must advertise and confirm the batch by size.
    live.key(KeyCode::Char('x'));
    let labels: Vec<String> = live
        .app
        .menu
        .as_ref()
        .expect("menu")
        .items
        .iter()
        .map(|i| i.label.clone())
        .collect();
    assert!(
        labels.iter().any(|l| l == "delete 2 marked items"),
        "marked menu: {labels:?}"
    );
    live.key(KeyCode::Delete);
    let confirm = live.app.confirm.as_ref().expect("batch confirmation");
    assert_eq!(confirm.target, "2 marked items");
    let screen = live.render(120, 30);
    assert!(
        screen.contains("delete 2 marked items?"),
        "confirmation text missing:\n{screen}"
    );

    live.key(KeyCode::Enter);
    let removed = live.wait_for(Duration::from_secs(60), |a| {
        !a.files.load.loading && a.files.visible().is_empty()
    });
    assert!(
        removed,
        "batch delete left entries behind: {:?} / status {}",
        live.app
            .files
            .visible()
            .iter()
            .map(|e| e.name.clone())
            .collect::<Vec<_>>(),
        live.app.status
    );
    assert!(
        live.app.files.marked.is_empty(),
        "marks must clear after delete"
    );
    assert!(
        live.app.status.contains("removed 2 items"),
        "delete status: {:?}",
        live.app.status
    );

    cleanup(&live, &root);
}

#[test]
fn live_error_toast_is_rendered_and_dismissed_by_a_key() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    // Listing a path that cannot exist fails in the client. The error must
    // surface as a toast and leave the footer alone.
    let missing = format!("{}/missing", run_root("toast"));
    live.command(&format!("ls {missing}"));
    assert!(
        live.wait_for(Duration::from_secs(60), |a| a.toast.is_some()),
        "missing-path listing produced no toast: status {:?}",
        live.app.status
    );
    let toast = live.app.toast.as_ref().expect("toast");
    assert!(
        toast.text.contains(&missing),
        "toast should name the path: {:?}",
        toast.text
    );

    let out = live.render(120, 30);
    assert!(out.contains(&missing), "toast not rendered:\n{out}");
    let footer = out.lines().last().unwrap_or_default();
    assert!(
        !footer.contains(&missing),
        "error leaked into the footer: {footer:?}"
    );

    // A key dismisses the toast and still performs its action.
    live.key(KeyCode::Char('r'));
    assert!(live.app.toast.is_none(), "r did not dismiss the toast");
    assert!(
        live.app.files.load.loading,
        "r's refresh was swallowed by the toast dismissal"
    );
}

#[test]
fn live_mount_form_creates_a_mount() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    let ufs_dir = mount_dir("form");
    std::fs::write(ufs_dir.join("hello.txt"), b"hello\n").expect("write");
    let cv_path = ufs_dir.display().to_string();
    let ufs = format!("file://{cv_path}");

    // Mount through the TUI's own form (cache mode is the form default).
    let mut form = MountForm::new("/");
    form.ufs_path = ufs;
    form.cv_path = cv_path.clone();
    live.app.mount_form = Some(form);
    live.app.mode = InputMode::Mount;
    live.app.panel = Panel::Mounts;
    live.key(KeyCode::Enter);
    assert!(
        live.wait_for(Duration::from_secs(90), |a| a
            .mounts
            .data
            .as_ref()
            .map(|l| l.iter().any(|m| m.cv_path == cv_path))
            .unwrap_or(false)),
        "mount never appeared: {}",
        live.app.status
    );

    let out = live.render(120, 30);
    assert!(out.contains("mounts"), "mounts panel missing:\n{out}");
    assert!(
        out.contains(&cv_path),
        "mount row missing from the panel:\n{out}"
    );
    assert!(
        out.contains("CacheMode"),
        "write-mode badge missing:\n{out}"
    );

    // Unmount through the menu → confirmation.
    let idx = live
        .app
        .mounts
        .data
        .as_ref()
        .and_then(|l| l.iter().position(|m| m.cv_path == cv_path))
        .expect("mount row");
    live.app.mount_selected = idx;
    live.key(KeyCode::Char('x'));
    live.key(KeyCode::Char('u'));
    assert!(live.app.confirm.as_ref().is_some(), "unmount must confirm");
    live.key(KeyCode::Enter);
    assert!(
        live.wait_for(Duration::from_secs(60), |a| a
            .mounts
            .data
            .as_ref()
            .map(|l| l.iter().all(|m| m.cv_path != cv_path))
            .unwrap_or(false)),
        "unmount did not take effect: {}",
        live.app.status
    );
}

#[test]
fn live_cached_file_detail_free_and_unmount() {
    skip_unless_live!();
    if cv_bin().is_none() {
        eprintln!("skipped: `cv` not found; set CURVINE_TUI_CV to run the cache test");
        return;
    }

    // Prepare a cache-mode mount with a populated cache. The cache fill comes
    // from `cv fs cat` (the same client read path the TUI uses); the TUI then
    // has to display those blocks, free them, and unmount.
    let ufs_dir = mount_dir("cache");
    let blob = script_file(&ufs_dir, "blob.bin", 256 * 1024);
    let cv_path = ufs_dir.display().to_string();
    let remote = format!("{cv_path}/blob.bin");
    let ufs = format!("file://{cv_path}");

    let (cfg, _) = cluster();
    let mount_client = ClusterClient::connect(cfg).expect("mount client");
    let opts = curvine_model::MountOptions::builder()
        .write_type(curvine_model::WriteType::CacheMode)
        .storage_type(curvine_model::StorageType::Disk)
        .ttl_ms(0)
        .ttl_action(curvine_model::TtlAction::Delete)
        .auto_cache(true)
        .build();
    mount_client
        .runtime()
        .block_on(mount_client.mount(&ufs, &cv_path, opts))
        .expect("mount");

    let cat = run_cv(&["fs", "cat", &remote]);
    assert!(
        cat.status.success(),
        "cv fs cat failed: {}{}",
        String::from_utf8_lossy(&cat.stdout),
        String::from_utf8_lossy(&cat.stderr)
    );

    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    // The cached blocks are visible through the TUI's file detail.
    live.command(&format!("cd {cv_path}"));
    assert!(
        live.wait_for(Duration::from_secs(60), |a| {
            a.files.cwd == cv_path
                && !a.files.load.loading
                && a.files.visible().iter().any(|e| e.name == "blob.bin")
        }),
        "mounted listing did not load: {}",
        live.app.status
    );
    live.select("blob.bin");
    live.key(KeyCode::Char('d'));
    assert!(
        live.wait_for(Duration::from_secs(60), |a| {
            a.detail.as_ref().map(|d| !d.loading).unwrap_or(false)
        }),
        "block detail did not load: {}",
        live.app.status
    );
    let detail = live.app.detail.as_ref().expect("detail");
    assert!(
        detail.blocks.iter().any(|b| b.available),
        "cached replicas missing from the detail"
    );
    assert!(
        live.render(120, 34).contains(&cv_path),
        "detail should name the mounted path"
    );
    live.key(KeyCode::Esc);

    // Files → x → F → confirm releases the cache but keeps the entry.
    live.key(KeyCode::Char('x'));
    live.key(KeyCode::Char('F'));
    assert!(live.app.confirm.is_some(), "free must ask first");
    live.key(KeyCode::Enter);

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut freed = false;
    let mut last_status = live.app.status.clone();
    while Instant::now() < deadline {
        let client = live.app.client.clone();
        let remote = remote.clone();
        let blocks =
            live.block_on(async move { client.file_blocks(&remote).await.unwrap_or_default() });
        if blocks.iter().all(|b| b.replicas.is_empty()) {
            freed = true;
            break;
        }
        last_status = live.app.status.clone();
        live.tick();
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        freed,
        "free did not release the cached replicas: {last_status}"
    );
    let still_listed = live
        .block_on({
            let client = live.app.client.clone();
            let remote = remote.clone();
            async move { client.ls(&crate::app::parent_path(&remote)).await }
        })
        .unwrap_or_default()
        .iter()
        .any(|e| e.name == "blob.bin");
    assert!(still_listed, "free must keep the entry itself");

    // Unmount through the menu → confirmation.
    live.app.panel = Panel::Mounts;
    live.key(KeyCode::Char('r'));
    assert!(
        live.wait_for(Duration::from_secs(60), |a| a
            .mounts
            .data
            .as_ref()
            .map(|l| l.iter().any(|m| m.cv_path == cv_path))
            .unwrap_or(false)),
        "mount not listed: {}",
        live.app.status
    );
    let idx = live
        .app
        .mounts
        .data
        .as_ref()
        .and_then(|l| l.iter().position(|m| m.cv_path == cv_path))
        .expect("mount row");
    live.app.mount_selected = idx;
    live.key(KeyCode::Char('x'));
    live.key(KeyCode::Char('u'));
    live.key(KeyCode::Enter);
    assert!(
        live.wait_for(Duration::from_secs(60), |a| a
            .mounts
            .data
            .as_ref()
            .map(|l| l.iter().all(|m| m.cv_path != cv_path))
            .unwrap_or(false)),
        "unmount did not take effect: {}",
        live.app.status
    );
    let _ = std::fs::remove_file(&blob);
}

#[test]
fn live_load_job_is_tracked_and_shown_in_jobs() {
    skip_unless_live!();
    if cv_bin().is_none() {
        eprintln!("skipped: `cv` not found; set CURVINE_TUI_CV to run the load test");
        return;
    }
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    // fs-mode mounts need a CLI-side resync to turn UFS objects into Curvine
    // inodes; `load` only accepts UFS-backed metadata. The CLI prepares the
    // fixture, the TUI drives the job.
    let ufs_dir = mount_dir("load");
    script_file(&ufs_dir, "load.bin", 128 * 1024);
    let cv_path = ufs_dir.display().to_string();
    let remote = format!("{cv_path}/load.bin");
    let ufs = format!("file://{cv_path}");

    let mount = run_cv(&["mount", &ufs, &cv_path]);
    assert!(
        mount.status.success(),
        "cv mount failed: {}{}",
        String::from_utf8_lossy(&mount.stdout),
        String::from_utf8_lossy(&mount.stderr)
    );

    // Wait until the resynced file is visible in Curvine metadata.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut visible = false;
    while Instant::now() < deadline {
        let client = live.app.client.clone();
        let path = cv_path.clone();
        let listing = live
            .block_on(async move { client.ls(&path).await })
            .unwrap_or_default();
        if listing.iter().any(|e| e.name == "load.bin") {
            visible = true;
            break;
        }
        live.tick();
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(visible, "resync never registered the mounted file");

    // `load` must return a job id to track; the panel polls by id.
    live.command(&format!("load {remote}"));
    let tracked = live.wait_for(Duration::from_secs(60), |a| !a.tracked_jobs.is_empty());
    assert!(
        tracked,
        "load was not tracked: status {:?}",
        live.app.status
    );
    assert!(
        live.app
            .tracked_jobs
            .iter()
            .all(|id| id.starts_with("job_")),
        "tracked_jobs must hold job ids, not paths: {:?}",
        live.app.tracked_jobs
    );
    assert!(
        live.app.status.contains("job_"),
        "status should name the job: {:?}",
        live.app.status
    );

    live.app.panel = Panel::Jobs;
    live.app.refresh_jobs();
    assert!(
        live.wait_for(Duration::from_secs(60), |a| a
            .jobs
            .data
            .as_ref()
            .map(|l| !l.is_empty())
            .unwrap_or(false)),
        "jobs list stayed empty: {}",
        live.app.status
    );
    let out = live.render(120, 30);
    assert!(
        out.contains("job_"),
        "job id missing from the panel:\n{out}"
    );
    assert!(out.contains("source"), "job columns missing:\n{out}");
    assert!(out.contains("load.bin"), "job path missing:\n{out}");

    // Selection and the conditional cancel entry are wired to the same state.
    live.key(KeyCode::Down);
    assert_eq!(
        live.app.job_selected, 0,
        "one job: selection stays in bounds"
    );
    live.key(KeyCode::Char('c'));
    assert!(
        live.app.status.contains("cancel") || live.app.status.contains("job"),
        "cancel produced no status: {:?}",
        live.app.status
    );

    let umount = run_cv(&["umount", &cv_path]);
    assert!(
        umount.status.success(),
        "cv umount failed: {}{}",
        String::from_utf8_lossy(&umount.stdout),
        String::from_utf8_lossy(&umount.stderr)
    );
}

#[test]
fn live_command_completion_history_and_panel_jumps() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    // Completion row (directly above the bottom footer row) + Tab completes
    // the first prefix match.
    live.key(KeyCode::Char(':'));
    live.text("l");
    let screen = live.render(120, 30);
    let hint = screen.lines().nth_back(1).unwrap_or_default();
    assert!(hint.contains("match: ls"), "completion row: {hint:?}");
    live.key(KeyCode::Tab);
    assert_eq!(live.app.command, "ls ");
    live.key(KeyCode::Esc);

    // A real command, then history recall.
    let (root, _) = seed(&live, "cmd", 4096);
    let cd = format!("cd {root}");
    live.command(&cd);
    assert!(
        live.wait_for(Duration::from_secs(30), |a| {
            a.files.cwd == root && !a.files.load.loading
        }),
        "cd did not land: {}",
        live.app.status
    );
    assert_eq!(live.app.command_history.last(), Some(&cd));
    live.key(KeyCode::Char(':'));
    live.key(KeyCode::Up);
    assert_eq!(live.app.command, cd);
    live.key(KeyCode::Esc);

    // Panel switch by command, rendered from live data.
    live.command("panel nodes");
    assert_eq!(live.app.panel, Panel::Nodes);
    live.wait_cluster();
    let endpoint = live.app.cluster.data.as_ref().unwrap().workers[0]
        .address
        .clone();
    let version = live.app.cluster.data.as_ref().unwrap().workers[0]
        .version
        .clone();
    let out = live.render(130, 30);
    assert!(out.contains(&endpoint), "node endpoint:\n{out}");
    assert!(out.contains("disk used"), "node occupancy label:\n{out}");
    let expected = crate::ui::nodes::short_version(&version);
    assert!(
        out.contains(&expected),
        "short version {expected:?} missing for {version:?}:\n{out}"
    );
    assert!(!out.contains("commit:"), "raw build string leaked:\n{out}");

    live.key(KeyCode::Char('d'));
    let view = live.app.detail.as_ref().expect("worker detail");
    assert!(
        view.rows.iter().any(|(k, _)| k == "capacity"),
        "worker detail rows missing"
    );
    assert!(
        live.render(120, 34).contains("storage classes"),
        "worker storage breakdown missing"
    );
    live.key(KeyCode::Char('3'));
    assert!(live.app.detail.is_none());
    assert_eq!(live.app.panel, Panel::Nodes);

    cleanup(&live, &root);
}

#[test]
fn live_fsck_scan_and_bad_file_selection() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    let (root, _) = seed(&live, "fsck", 64 * 1024);
    live.app.panel = Panel::Fsck;
    live.app.fsck_path = root.clone();
    live.key(KeyCode::Char('s'));
    let done = live.wait_for(Duration::from_secs(180), |a| {
        !a.fsck.loading && a.fsck.data.is_some()
    });
    assert!(done, "fsck did not finish: {}", live.app.status);

    let snap = live.app.fsck.data.as_ref().expect("fsck data");
    assert_eq!(snap.root, root);
    assert!(
        snap.file_count >= 1,
        "unexpected file count: {}",
        snap.file_count
    );
    assert!(
        snap.block_count >= 1,
        "unexpected block count: {}",
        snap.block_count
    );

    let out = live.render(120, 34);
    for expected in [
        format!("fsck {root}").as_str(),
        "files",
        "blocks",
        "replicas",
        "unhealthy files",
    ] {
        assert!(
            out.contains(expected),
            "fsck panel missing {expected}:\n{out}"
        );
    }

    // Selection stays in bounds whatever the health of the scan.
    let bad = snap.bad_files.len();
    live.key(KeyCode::Down);
    assert!(live.app.fsck_selected < bad.max(1));
    // With no unhealthy rows, Enter falls back to a scan instead of opening an
    // empty detail view.
    if bad == 0 {
        live.key(KeyCode::Enter);
        assert!(live.app.detail.is_none());
    }

    cleanup(&live, &root);
}

#[test]
fn live_bench_run_reports_metrics() {
    skip_unless_live!();
    let Some(mut live) = Live::start() else {
        return;
    };
    live.wait_cluster();

    // Keep the run short; the point is that the panel is wired to the runner.
    live.app.bench_opts.duration_ms = Some(2_000);
    live.app.bench_opts.small_file_count = 4;
    live.app.bench_opts.threads = 2;
    live.app.panel = Panel::Bench;
    live.key(KeyCode::Char('b'));

    let done = live.wait_for(Duration::from_secs(240), |a| {
        a.bench_running.is_none() && a.bench.data.is_some()
    });
    assert!(done, "bench did not finish: {}", live.app.status);

    let data = live.app.bench.data.as_ref().expect("bench data");
    assert!(!data.rows.is_empty(), "bench returned no metrics");
    assert!(
        data.rows.iter().any(|r| r.value > 0.0),
        "bench metrics are all zero"
    );
    let out = live.render(130, 30);
    assert!(out.contains("results"), "bench results missing:\n{out}");
    assert!(
        out.contains("ops/s") || out.contains("MiB/s"),
        "bench units missing:\n{out}"
    );
    assert!(out.contains("basic"), "profile knob missing:\n{out}");

    // Leave the cluster as we found it: the runner creates its base directory.
    let base = live.app.bench_opts.base_path.clone();
    cleanup(&live, &base);
}
