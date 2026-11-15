//! Unit + render smoke tests.
//!
//! The render tests build an `App` directly (without `App::new`, which fires
//! network refreshes) and draw every panel into a `TestBackend`. This catches
//! layout underflows and out-of-bounds splits at awkward terminal sizes.

use ratatui::{backend::TestBackend, Terminal};
use tokio::sync::mpsc;

use crate::app::{
    join_path, normalize_path, parent_path, App, FilesState, Loadable, Panel, TransferEntry,
};
use crate::event::{TransferKind, TransferState};
use crate::transfer::new_cancel_flag;
use crate::config::{AppConfig, ConfigOverrides};
use crate::rpc::ClusterClient;

fn test_app() -> App {
    let cfg = AppConfig::load(ConfigOverrides::default()).expect("config");
    let client = ClusterClient::connect(&cfg).expect("client");
    let (tx, rx) = mpsc::unbounded_channel();
    // Keep the receiver alive for the duration of the test.
    std::mem::forget(rx);

    App {
        cfg,
        client,
        tx,
        panel: Panel::Dashboard,
        mode: crate::app::InputMode::Normal,
        command: String::new(),
        should_quit: false,
        status: String::new(),
        status_is_error: false,
        files: FilesState::default(),
        cluster: Loadable::default(),
        mounts: Loadable::default(),
        jobs: Loadable::default(),
        config: Loadable::default(),
        transfers: Vec::new(),
        transfer_selected: 0,
        form: None,
        next_transfer_id: 1,
        log: vec!["test".to_string()],
        help_visible: false,
    }
}

fn render_panel(panel: Panel, width: u16, height: u16, populate: bool) -> String {
    let mut app = test_app();
    app.panel = panel;
    if populate {
        populate_sample_data(&mut app);
    }

    let backend = TestBackend::new(width, height);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();

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

fn populate_sample_data(app: &mut App) {
    use crate::event::{ClusterSnapshot, FsEntry, JobRow, MountRow, WorkerRow};

    app.files.cwd = "/data".to_string();
    app.files.entries = vec![
        FsEntry {
            name: "warehouse".into(),
            path: "/data/warehouse".into(),
            is_dir: true,
            len: 0,
            mtime: 1_700_000_000_000,
            mode: 0o755,
            owner: "root".into(),
            group: "root".into(),
            replicas: 2,
        },
        FsEntry {
            name: "events.parquet".into(),
            path: "/data/events.parquet".into(),
            is_dir: false,
            len: 4 * 1024 * 1024,
            mtime: 1_700_000_000_000,
            mode: 0o644,
            owner: "etl".into(),
            group: "data".into(),
            replicas: 3,
        },
    ];

    app.cluster.ok(ClusterSnapshot {
        active_master: "master-1:8995".into(),
        journal_nodes: vec!["j1".into(), "j2".into()],
        inode_dir_num: 12,
        inode_file_num: 340,
        block_num: 1024,
        capacity: 100 * 1024 * 1024 * 1024,
        available: 40 * 1024 * 1024 * 1024,
        fs_used: 60 * 1024 * 1024 * 1024,
        non_fs_used: 1024,
        workers: vec![WorkerRow {
            address: "worker-1:8996".into(),
            status: "Live".into(),
            capacity: 50 * 1024 * 1024 * 1024,
            available: 20 * 1024 * 1024 * 1024,
            fs_used: 30 * 1024 * 1024 * 1024,
            block_num: 512,
            weight: 100,
            version: "0.2.2".into(),
        }],
    });

    app.mounts.ok(vec![MountRow {
        cv_path: "/data".into(),
        ufs_path: "s3://bucket/data".into(),
        mount_id: 1,
        ttl_ms: 0,
        storage_type: "S3".into(),
        write_type: "Cache".into(),
        provider: "Aws".into(),
    }]);

    app.jobs.ok(vec![JobRow {
        job_id: "job-0123456789abcdef".into(),
        state: "Complete".into(),
        source: "s3://bucket/data/a".into(),
        target: "/data/a".into(),
        progress: 0.82,
        total_files: 100,
        completed: 82,
    }]);

    app.config.ok(vec![crate::event::ConfigRow {
        key: "master.worker_timeout_ms".into(),
        value: "60000".into(),
    }]);

    app.transfers = vec![
        TransferEntry {
            id: 1,
            kind: TransferKind::Upload,
            local: "/tmp/events.parquet".into(),
            remote: "/data/events.parquet".into(),
            total: 8 * 1024 * 1024,
            transferred: 3 * 1024 * 1024,
            speed: 2 * 1024 * 1024,
            state: TransferState::Running,
            error: None,
            cancel: new_cancel_flag(),
        },
        TransferEntry {
            id: 2,
            kind: TransferKind::Download,
            local: "/tmp/out.csv".into(),
            remote: "/data/out.csv".into(),
            total: 1024,
            transferred: 1024,
            speed: 0,
            state: TransferState::Done,
            error: None,
            cancel: new_cancel_flag(),
        },
        TransferEntry {
            id: 3,
            kind: TransferKind::Upload,
            local: "/tmp/broken".into(),
            remote: "/data/broken".into(),
            total: 100,
            transferred: 40,
            speed: 0,
            state: TransferState::Failed,
            error: Some("no space left".into()),
            cancel: new_cancel_flag(),
        },
    ];
}

#[test]
fn renders_every_panel_with_data_at_common_sizes() {
    let sizes = [(120u16, 40u16), (80, 24), (200, 60)];
    for panel in Panel::ALL {
        for (w, h) in sizes {
            let out = render_panel(panel, w, h, true);
            assert!(
                out.contains(panel.title()),
                "panel {:?} title missing at {w}x{h}",
                panel
            );
        }
    }
}

#[test]
fn renders_every_panel_while_loading_and_empty() {
    for panel in Panel::ALL {
        let out = render_panel(panel, 100, 30, false);
        assert!(
            out.contains(panel.title()),
            "panel {:?} title missing when empty",
            panel
        );
    }
}

#[test]
fn renders_help_popup() {
    let mut app = test_app();
    app.help_visible = true;
    let backend = TestBackend::new(120, 40);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().to_string())
        .collect();
    assert!(text.contains("Help"));
    assert!(text.contains("switch panel"));
}

#[test]
fn renders_at_pathological_sizes_without_panicking() {
    for (w, h) in [(20u16, 8u16), (1, 1), (10, 3), (300, 5)] {
        for panel in Panel::ALL {
            render_panel(panel, w, h, true);
        }
    }
}

#[test]
fn every_panel_tab_is_reachable_at_narrow_widths() {
    // Regression: ratatui's Tabs adds 1 cell of padding on each side, which
    // used to push the trailing tabs off-screen at 80 columns.
    for width in [60u16, 80, 100, 120] {
        let out = render_panel(Panel::Config, width, 30, false);
        let header = out.lines().nth(1).unwrap_or_default();
        assert!(
            header.contains("7"),
            "selected tab index 7 missing from header at width {width}: {header:?}"
        );
    }
}

#[test]
fn transfer_rows_render_progress_and_state() {
    let out = render_panel(Panel::Transfers, 120, 20, true);
    assert!(out.contains("running"), "running state missing");
    assert!(out.contains("done"), "done state missing");
    assert!(out.contains("failed"), "failed state missing");
    assert!(out.contains("2.0 MiB/s"), "speed missing");
    // 3.0/8.0 MiB ≈ 38%
    assert!(out.contains("38%"), "progress percentage missing");
}

#[test]
fn transfer_form_renders_both_fields() {
    let mut app = test_app();
    app.panel = Panel::Transfers;
    app.mode = crate::app::InputMode::Transfer;
    app.form = Some(crate::app::TransferForm::upload("/data"));

    let backend = TestBackend::new(100, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().to_string())
        .collect();

    assert!(text.contains("upload"), "form title missing");
    assert!(text.contains("local"), "local field label missing");
    assert!(text.contains("remote"), "remote field label missing");
    assert!(text.contains("/data/"), "cwd-derived remote path missing");
}

#[test]
fn transfer_ratio_is_clamped_and_zero_safe() {
    let mk = |total: u64, transferred: u64, state: TransferState| TransferEntry {
        id: 1,
        kind: TransferKind::Upload,
        local: "/a".into(),
        remote: "/b".into(),
        total,
        transferred,
        speed: 0,
        state,
        error: None,
        cancel: new_cancel_flag(),
    };

    assert_eq!(mk(0, 0, TransferState::Running).ratio(), 0.0);
    assert_eq!(mk(0, 0, TransferState::Done).ratio(), 1.0);
    assert_eq!(mk(100, 50, TransferState::Running).ratio(), 0.5);
    // Over-reporting must not exceed 1.0 (gauge would panic).
    assert_eq!(mk(100, 150, TransferState::Running).ratio(), 1.0);
}

#[test]
fn cancelling_a_running_transfer_sets_terminal_state() {
    let mut app = test_app();
    app.transfers = vec![TransferEntry {
        id: 9,
        kind: TransferKind::Upload,
        local: "/a".into(),
        remote: "/b".into(),
        total: 100,
        transferred: 10,
        speed: 0,
        state: TransferState::Running,
        error: None,
        cancel: new_cancel_flag(),
    }];
    app.transfer_selected = 0;
    let flag = app.transfers[0].cancel.clone();

    app.cancel_selected_transfer();

    assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(app.transfers[0].state, TransferState::Cancelled);
}

// ---------------------------------------------------------------- pure helpers

#[test]
fn normalize_path_resolves_relative_and_dotdot() {
    assert_eq!(normalize_path("/a/b", "/"), "/a/b");
    assert_eq!(normalize_path("c", "/a/b"), "/a/b/c");
    assert_eq!(normalize_path("..", "/a/b/c"), "/a/b");
    assert_eq!(normalize_path("../..", "/a/b/c"), "/a");
    assert_eq!(normalize_path("../../..", "/a"), "/");
    assert_eq!(normalize_path("./x/./y/", "/a"), "/a/x/y");
}

#[test]
fn path_helpers_round_trip() {
    assert_eq!(join_path("/", "x"), "/x");
    assert_eq!(join_path("/a", "x"), "/a/x");
    assert_eq!(join_path("/a/", "x"), "/a/x");
    assert_eq!(parent_path("/a/b"), "/a");
    assert_eq!(parent_path("/a"), "/");
    assert_eq!(parent_path("/"), "/");
}

#[test]
fn human_bytes_scales() {
    assert_eq!(crate::ui::human_bytes(0), "0 B");
    assert_eq!(crate::ui::human_bytes(512), "512 B");
    assert_eq!(crate::ui::human_bytes(1024), "1.0 KiB");
    assert_eq!(crate::ui::human_bytes(1024 * 1024), "1.0 MiB");
    assert_eq!(crate::ui::human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
}

#[test]
fn format_mode_matches_ls_layout() {
    assert_eq!(crate::ui::format_mode(0o755, true), "drwxr-xr-x");
    assert_eq!(crate::ui::format_mode(0o644, false), "-rw-r--r--");
    assert_eq!(crate::ui::format_mode(0o000, false), "----------");
}
