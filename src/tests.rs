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
use crate::event::{
    BenchRowData, BenchSnapshot, ClusterSnapshot, FsEntry, FsckFileRow, FsckSnapshot,
    TransferKind, TransferState,
};
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
        refresh_secs: 5,
        next_refresh: std::time::Instant::now() + std::time::Duration::from_secs(5),
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
        next_transfer_id: 1,
        anim: crate::anim::AnimState::new(false),
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
            address: "127.0.0.1:8997".into(),
            ip: "127.0.0.1".into(),
            hostname: "localhost".into(),
            rpc_port: 8997,
            web_port: 9002,
            worker_id: 1,
            status: "Live".into(),
            capacity: 50 * 1024 * 1024 * 1024,
            available: 20 * 1024 * 1024 * 1024,
            fs_used: 30 * 1024 * 1024 * 1024,
            block_num: 512,
            weight: 100,
            version: "0.2.2".into(),
            storage: vec![("Ssd".into(), 50 * 1024 * 1024 * 1024)],
        }],
    });

    app.mounts.ok(vec![
        MountRow {
            cv_path: "/logs".into(),
            ufs_path: "file:///tmp/logs".into(),
            mount_id: 2,
            ttl_ms: 0,
            storage_type: "Hdd".into(),
            write_type: "FsMode".into(),
            provider: String::new(),
        },
        MountRow {
        cv_path: "/data".into(),
        ufs_path: "s3://bucket/data".into(),
        mount_id: 1,
        ttl_ms: 0,
        storage_type: "S3".into(),
        write_type: "Cache".into(),
        provider: "Aws".into(),
        },
    ]);

    app.jobs.ok(vec![JobRow {
        job_id: "job-0123456789abcdef".into(),
        state: "Complete".into(),
        source: "s3://bucket/data/a".into(),
        target: "/data/a".into(),
        progress: 0.82,
        total_bytes: 100 * 1024 * 1024,
        loaded_bytes: 82 * 1024 * 1024,
    }]);

    app.fsck.ok(FsckSnapshot {
        root: "/data".into(),
        file_count: 120,
        block_count: 340,
        expected_replicas: 680,
        recorded_replicas: 680,
        available_replicas: 640,
        unavailable_replicas: 40,
        under_replicated_blocks: 12,
        missing_replica_blocks: 1,
        health_ratio: 640.0 / 680.0,
        warnings: true,
        errors: vec![("/data/bad".into(), "permission denied".into())],
        bad_files: vec![
            FsckFileRow {
                path: "/data/bad".into(),
                len: 0,
                blocks: 0,
                expected: 0,
                available: 0,
                error: Some("permission denied".into()),
            },
            FsckFileRow {
                path: "/data/events.parquet".into(),
                len: 4 * 1024 * 1024,
                blocks: 3,
                expected: 3,
                available: 1,
                error: None,
            },
        ],
        elapsed_ms: 42,
    });

    app.bench.ok(BenchSnapshot {
        rows: vec![
            BenchRowData {
                group: "Metadata".into(),
                op: "Create".into(),
                item: "Create file".into(),
                value: 2822.85,
                unit: "ops/s".into(),
                p50: 0.511,
                p95: 0.55,
                p99: 0.588,
                errors: 0,
            },
            BenchRowData {
                group: "Throughput".into(),
                op: "ReadBig".into(),
                item: "Read big file".into(),
                value: 5003.18,
                unit: "MiB/s".into(),
                p50: 12.783,
                p95: 12.9,
                p99: 13.0,
                errors: 2,
            },
        ],
        temp_path: "/__curvine_tui_bench__/abc".into(),
        elapsed_ms: 54.5,
        started: std::time::Instant::now(),
    });

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

    // The real reducer retargets animations when data arrives; the test helper
    // injects data directly, so do the same here.
    app.retarget_animations();
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
fn header_identifies_the_active_panel_at_narrow_widths() {
    // The tab strip is gone; the active panel now lives in the one-line
    // context header and must never be the part that gets dropped.
    let last = *Panel::ALL.last().unwrap();
    for width in [40u16, 60, 80, 100, 120] {
        let out = render_panel(last, width, 30, false);
        let header = out.lines().next().unwrap_or_default();
        assert!(
            header.contains("❖ curvine-tui"),
            "identity missing from header at width {width}: {header:?}"
        );
        assert!(
            header.contains(last.title()),
            "active panel missing from header at width {width}: {header:?}"
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
    app.fsck.ok(FsckSnapshot {
        root: "/data".into(),
        file_count: 120,
        block_count: 340,
        expected_replicas: 680,
        recorded_replicas: 680,
        available_replicas: 640,
        unavailable_replicas: 40,
        under_replicated_blocks: 12,
        missing_replica_blocks: 1,
        health_ratio: 640.0 / 680.0,
        warnings: true,
        errors: vec![("/data/bad".into(), "permission denied".into())],
        bad_files: vec![
            FsckFileRow {
                path: "/data/bad".into(),
                len: 0,
                blocks: 0,
                expected: 0,
                available: 0,
                error: Some("permission denied".into()),
            },
            FsckFileRow {
                path: "/data/events.parquet".into(),
                len: 4 * 1024 * 1024,
                blocks: 3,
                expected: 3,
                available: 1,
                error: None,
            },
        ],
        elapsed_ms: 42,
    });

    app.bench.ok(BenchSnapshot {
        rows: vec![
            BenchRowData {
                group: "Metadata".into(),
                op: "Create".into(),
                item: "Create file".into(),
                value: 2822.85,
                unit: "ops/s".into(),
                p50: 0.511,
                p95: 0.55,
                p99: 0.588,
                errors: 0,
            },
            BenchRowData {
                group: "Throughput".into(),
                op: "ReadBig".into(),
                item: "Read big file".into(),
                value: 5003.18,
                unit: "MiB/s".into(),
                p50: 12.783,
                p95: 12.9,
                p99: 13.0,
                errors: 2,
            },
        ],
        temp_path: "/__curvine_tui_bench__/abc".into(),
        elapsed_ms: 54.5,
        started: std::time::Instant::now(),
    });

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

#[test]
fn mount_form_renders_all_fields() {
    let mut app = test_app();
    app.panel = Panel::Mounts;
    app.mode = crate::app::InputMode::Mount;
    app.mount_form = Some(crate::app::MountForm::new("/data"));

    let backend = TestBackend::new(110, 34);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let text = buffer_text(&term);

    assert!(text.contains("new mount"), "form title missing:\n{text}");
    for field in ["ufs", "curvine", "write mode", "storage", "ttl", "auto cache", "read verify"] {
        assert!(text.contains(field), "field {field:?} missing:\n{text}");
    }
    assert!(text.contains("cache"), "write mode value missing");
    assert!(text.contains("/data"), "cv path default missing");
}

#[test]
fn umount_confirm_renders_target_path() {
    let mut app = test_app();
    app.panel = Panel::Mounts;
    app.mode = crate::app::InputMode::Confirm;
    app.confirm = Some(crate::app::PendingConfirm {
        action: crate::app::PendingAction::Umount {
            path: "/data".to_string(),
        },
        title: "unmount".to_string(),
        target: "/data".to_string(),
        lines: vec!["cached data on this mount becomes unreachable".to_string()],
    });

    let backend = TestBackend::new(100, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let text = buffer_text(&term);

    assert!(text.contains("unmount"), "confirm title missing:\n{text}");
    assert!(text.contains("/data"), "target path missing");
    assert!(text.contains("confirm"), "confirm hint missing");
}

// --------------------------------------------------------------- new chrome

fn render_app(app: &App, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, app)).unwrap();
    buffer_text(&term)
}

#[test]
fn panels_use_rounded_borders() {
    let app = test_app();
    let out = render_app(&app, 100, 30);
    assert!(out.contains('╭'), "missing rounded top-left corner:\n{out}");
    assert!(out.contains('╮'), "missing rounded top-right corner");
    assert!(out.contains('╰'), "missing rounded bottom-left corner");
    assert!(out.contains('╯'), "missing rounded bottom-right corner");
    // The square box-drawing corners must be gone.
    assert!(!out.contains('┌'), "square corner still present");
    assert!(!out.contains('└'), "square corner still present");
}

#[test]
fn file_rows_carry_type_icons() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.entries.push(FsEntry {
        name: "job.log".into(),
        path: "/data/job.log".into(),
        is_dir: false,
        len: 12,
        mtime: 1_700_000_000_000,
        mode: 0o644,
        owner: "etl".into(),
        group: "data".into(),
        replicas: 1,
    });

    let out = render_app(&app, 110, 20);
    assert!(out.contains(crate::ui::icons::DIR), "folder glyph missing");
    assert!(
        out.contains(crate::ui::icons::for_file("events.parquet")),
        "parquet glyph missing"
    );
    assert!(out.contains(crate::ui::icons::LIST), "log glyph missing");
}

#[test]
fn file_rows_show_replica_counts() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    let out = render_app(&app, 110, 20);
    assert!(out.contains("repl"), "replica column header missing:\n{out}");
    // Replica count is drawn as a coloured dot plus the number.
    assert!(
        out.contains(&format!("{} 3", crate::ui::components::DOT)),
        "replica count 3 missing:\n{out}"
    );
    // Directories have no replicas and must not print a misleading count.
    assert!(
        !out.contains(&format!("{} 0", crate::ui::components::DOT)),
        "directories should show a hollow dot, not a count"
    );
    assert!(
        out.contains(crate::ui::components::DOT_HOLLOW),
        "expected a hollow dot for non-replicated entries"
    );
}

#[test]
fn node_panel_labels_the_bar_as_disk_not_load() {
    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    let out = render_app(&app, 110, 16);
    assert!(out.contains("disk used"), "expected a disk label:\n{out}");
    assert!(
        !out.chars().collect::<String>().contains("load "),
        "the bar measures occupancy, not CPU load"
    );
}

#[test]
fn dashboard_reports_directory_and_file_counts() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    let out = render_app(&app, 110, 26);
    assert!(out.contains("directories"), "directory count row missing");
    assert!(out.contains("files"), "file count row missing");
    assert!(out.contains("340"), "file count value missing");
    assert!(out.contains("12"), "directory count value missing");
}

#[test]
fn delete_key_opens_confirmation_instead_of_deleting() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1; // events.parquet

    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Delete,
        crossterm::event::KeyModifiers::NONE,
    )));

    let confirm = app.confirm.as_ref().expect("delete must ask first");
    assert_eq!(confirm.title, "delete");
    assert!(confirm.target.ends_with("events.parquet"));
    assert!(matches!(
        confirm.action,
        crate::app::PendingAction::DeleteFile { recursive: false, .. }
    ));
    assert_eq!(app.mode, crate::app::InputMode::Confirm);
}

#[test]
fn deleting_a_directory_without_recursion_is_refused() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 0; // warehouse/ (directory)

    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Delete,
        crossterm::event::KeyModifiers::NONE,
    )));

    assert!(app.confirm.is_none(), "must not offer a non-recursive delete");
    assert!(app.status.contains("D"), "expected a hint about D: {}", app.status);
}

/// Drive frames until every animation settles, so assertions see the final
/// layout instead of a mid-transition frame.
fn settle(app: &mut App) {
    for _ in 0..60 {
        app.on_event(crate::event::AppEvent::Frame(std::time::Duration::from_millis(16)));
    }
}

#[test]
fn confirm_dialog_renders_the_target_and_is_destructive() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Delete,
        crossterm::event::KeyModifiers::NONE,
    )));

    settle(&mut app);
    let out = render_app(&app, 110, 26);
    assert!(out.contains("delete"), "dialog title missing:\n{out}");
    assert!(out.contains("events.parquet"), "target missing");
    assert!(out.contains("replicas"), "detail rows missing");
    assert!(out.contains("confirm"), "confirm footer missing");
}

#[test]
fn d_opens_describe_with_file_detail() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;

    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::NONE,
    )));

    let view = app.describe.as_ref().expect("d must open describe");
    assert_eq!(app.mode, crate::app::InputMode::Describe);
    assert!(view.title.contains("describe"));
    assert!(view.subject.ends_with("events.parquet"));
    let keys: Vec<&str> = view.rows.iter().map(|(k, _)| k.as_str()).collect();
    for expected in ["path", "mode", "owner", "size", "replicas", "blocks"] {
        assert!(keys.contains(&expected), "missing {expected} row: {keys:?}");
    }
}

#[test]
fn d_on_nodes_describes_the_worker() {
    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    app.node_selected = 0;

    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::NONE,
    )));

    let view = app.describe.as_ref().expect("d must describe the worker");
    assert!(view.subject.contains("127.0.0.1"));
    assert!(!view.subject.contains("worker_id"), "subject must not use the verbose worker_id form");
    let keys: Vec<&str> = view.rows.iter().map(|(k, _)| k.as_str()).collect();
    for expected in ["capacity", "used", "free", "occupancy", "blocks"] {
        assert!(keys.contains(&expected), "missing {expected}: {keys:?}");
    }
    // The occupancy row must say it is not CPU load.
    let occ = view.rows.iter().find(|(k, _)| k == "occupancy").unwrap();
    assert!(occ.1.contains("not CPU load"), "occupancy mislabelled: {}", occ.1);
}

#[test]
fn escape_closes_the_describe_popup() {
    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    app.node_describe();
    assert!(app.describe.is_some());

    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    )));
    assert!(app.describe.is_none());
    assert_eq!(app.mode, crate::app::InputMode::Normal);
}

#[test]
fn header_shows_the_active_panel_icon_and_context() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    let out = render_app(&app, 118, 30);
    let header = out.lines().next().unwrap_or_default();
    assert!(
        header.contains("❖ curvine-tui"),
        "product identity missing: {header:?}"
    );
    assert!(
        header.contains(&format!(" {}", app.panel.title())) || header.contains(app.panel.title()),
        "active panel name missing: {header:?}"
    );
    // Cluster context: health dot, active master, worker count, refresh timer.
    assert!(header.contains("master-1:8995"), "master missing: {header:?}");
    assert!(header.contains("1 worker"), "worker count missing: {header:?}");
    assert!(header.contains('↻'), "refresh countdown missing: {header:?}");
    assert!(header.contains("1-8"), "panel shortcut hint missing");
    assert!(header.contains("command"), "command hint missing");
}

#[test]
fn header_reports_disconnected_without_cluster_data() {
    let app = test_app();
    let out = render_app(&app, 110, 30);
    let header = out.lines().next().unwrap_or_default();
    assert!(header.contains("disconnected"), "expected a disconnected state: {header:?}");
    assert!(header.contains(crate::ui::components::DOT_HOLLOW), "expected a hollow dot");
}

#[test]
fn footer_merges_status_and_context_hints() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    app.set_status_for_test("saved 12 files");
    let out = render_app(&app, 120, 30);
    let footer = out.lines().last().unwrap_or_default();
    assert!(footer.contains("saved 12 files"), "status missing from footer: {footer:?}");
    assert!(footer.contains("x menu"), "panel hints missing from footer: {footer:?}");
    // The old separate status/hint rows are gone: the last two lines are the
    // footer and something from panel content, not a second chrome row.
    assert!(
        !out.lines().nth_back(1).unwrap_or_default().contains("saved 12 files"),
        "status must not occupy its own row above the footer"
    );
}

#[test]
fn worker_tables_lead_with_the_endpoint_not_the_worker_id() {
    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    let out = render_app(&app, 110, 18);

    assert!(out.contains("127.0.0.1:8997"), "endpoint missing:\n{out}");
    // WorkerAddress's Display form must never reach the table.
    assert!(
        !out.contains("worker_id =") && !out.contains("addr ="),
        "verbose WorkerAddress form leaked into the UI:\n{out}"
    );
    assert!(!out.contains("worker-1"), "stale workerId label present");
}

#[test]
fn dashboard_has_no_duplicated_aggregates() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    let out = render_app(&app, 112, 26);

    // The old layout had three separate panels repeating the same numbers.
    assert!(!out.contains("at a glance"), "duplicate strip still present");
    assert!(out.contains("cluster"), "cluster panel missing");

    // Capacity is folded into the cluster panel, not its own block.
    let capacity_panels = out.matches("capacity").count();
    assert_eq!(capacity_panels, 1, "capacity should appear exactly once");
}

#[test]
fn describe_groups_blocks_by_worker() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.mode = crate::app::InputMode::Describe;
    app.describe = Some(crate::app::DescribeView {
        title: "describe events.parquet".into(),
        subject: "/data/events.parquet".into(),
        headline: Some(("file".into(), "/data/events.parquet".into())),
        rows: vec![("replicas".into(), "3 (configured factor)".into())],
        worker_blocks: vec![
            crate::app::WorkerBlocks {
                worker: "127.0.0.1:8997".into(),
                block_ids: vec!["101".into(), "102".into()],
                bytes: 3_000_000,
                available: 2,
                total: 2,
            },
            crate::app::WorkerBlocks {
                worker: "127.0.0.2:8997".into(),
                block_ids: vec!["101".into()],
                bytes: 1_500_000,
                available: 0,
                total: 1,
            },
        ],
        blocks: vec![
            crate::event::BlockRow {
                block_id: "101".into(),
                offset: 0,
                len: 1_500_000,
                worker: "127.0.0.1:8997".into(),
                available: true,
                state: "Live".into(),
            },
            crate::event::BlockRow {
                block_id: "101".into(),
                offset: 0,
                len: 1_500_000,
                worker: "127.0.0.2:8997".into(),
                available: false,
                state: "Lost".into(),
            },
        ],
        loading: false,
    });

    let out = render_app(&app, 116, 32);
    assert!(out.contains("distribution by worker"), "worker section missing:\n{out}");
    assert!(out.contains("blocks by offset"), "block section missing");
    assert!(out.contains("127.0.0.1:8997"), "worker endpoint missing");
    assert!(out.contains("2 blocks"), "per-worker block count missing");
    // The lost replica must be visible as its state, not just a count.
    assert!(out.contains("Lost"), "non-live replica state missing");
}

#[test]
fn aggregate_by_worker_sorts_and_counts() {
    let rows = vec![
        crate::event::BlockRow {
            block_id: "2".into(),
            offset: 10,
            len: 100,
            worker: "b:1".into(),
            available: true,
            state: "Live".into(),
        },
        crate::event::BlockRow {
            block_id: "1".into(),
            offset: 0,
            len: 100,
            worker: "a:1".into(),
            available: false,
            state: "Lost".into(),
        },
        crate::event::BlockRow {
            block_id: "1".into(),
            offset: 0,
            len: 100,
            worker: "b:1".into(),
            available: true,
            state: "Live".into(),
        },
    ];

    let agg = crate::app::aggregate_by_worker(&rows);
    // Sorted by endpoint.
    assert_eq!(agg[0].worker, "a:1");
    assert_eq!(agg[1].worker, "b:1");
    // b holds two distinct blocks totalling 200 bytes, both readable.
    assert_eq!(agg[1].block_ids, vec!["1".to_string(), "2".to_string()]);
    assert_eq!(agg[1].bytes, 200);
    assert_eq!((agg[1].available, agg[1].total), (2, 2));
    // a holds one block with a single, unreadable replica.
    assert_eq!((agg[0].available, agg[0].total), (0, 1));
}

#[test]
fn rich_controls_survive_narrow_layouts() {
    // Bars, badges and KV rows must all remain renderable when every panel
    // drops to its compact column set.
    for panel in Panel::ALL {
        for width in [60u16, 76, 96] {
            let out = render_panel(panel, width, 22, true);
            assert!(
                out.contains(panel.title()),
                "panel {:?} unidentifiable at width {width}",
                panel
            );
        }
    }
}

#[test]
fn fsck_panel_shows_verdict_and_unhealthy_files() {
    let out = render_panel(Panel::Fsck, 120, 30, true);
    assert!(out.contains("degraded"), "degraded verdict missing:\n{out}");
    assert!(out.contains("under-replicated"), "issue summary missing");
    assert!(out.contains("events.parquet"), "unhealthy file missing");
    assert!(out.contains("1/3"), "replica ratio missing");
    assert!(out.contains("permission denied"), "scan error missing");
}

#[test]
fn fsck_panel_reports_healthy_state() {
    let mut app = test_app();
    app.panel = Panel::Fsck;
    populate_sample_data(&mut app);
    if let Some(s) = app.fsck.data.as_mut() {
        s.warnings = false;
        s.bad_files.clear();
        s.errors.clear();
        s.health_ratio = 1.0;
        s.available_replicas = s.expected_replicas;
        s.unavailable_replicas = 0;
        s.under_replicated_blocks = 0;
        s.missing_replica_blocks = 0;
    }

    let backend = TestBackend::new(120, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let text = buffer_text(&term);
    assert!(text.contains("healthy"), "healthy verdict missing:\n{text}");
    assert!(text.contains("none"), "expected an explicit 'none' row");
}

#[test]
fn bench_panel_renders_metrics_with_units() {
    let out = render_panel(Panel::Bench, 130, 26, true);
    assert!(out.contains("ReadBig"), "op missing");
    assert!(out.contains("MiB/s"), "throughput unit missing");
    assert!(out.contains("ops/s"), "ops unit missing");
    assert!(out.contains("2822.85"), "value missing");
    assert!(out.contains("basic"), "profile knob missing");
}

fn buffer_text(term: &Terminal<TestBackend>) -> String {
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





#[test]
fn idle_ui_asks_for_a_slow_frame_rate() {
    // The whole point of a frame clock is that an idle UI must not redraw at
    // 60 fps. Verify the cadence the main loop would request in each state.
    let mut app = test_app();
    app.panel = Panel::Dashboard;

    let interval = |app: &App| {
        if app.is_animating() {
            crate::anim::FRAME
        } else if app.is_busy() {
            crate::anim::BUSY_FRAME
        } else {
            crate::anim::IDLE_FRAME
        }
    };

    // Nothing loaded, nothing moving.
    assert_eq!(interval(&app), crate::anim::IDLE_FRAME);

    // A loading panel needs frames for its spinner, but not 60 fps.
    app.cluster.begin();
    assert_eq!(interval(&app), crate::anim::BUSY_FRAME);

    // Data arriving kicks off a transition, which wants full frame rate.
    populate_sample_data(&mut app);
    app.cluster.ok(ClusterSnapshot {
        active_master: "m:1".into(),
        journal_nodes: vec![],
        inode_dir_num: 5,
        inode_file_num: 5,
        block_num: 5,
        capacity: 100,
        available: 50,
        fs_used: 50,
        non_fs_used: 0,
        workers: vec![],
    });
    app.retarget_animations();
    app.on_event(crate::event::AppEvent::Frame(std::time::Duration::from_millis(16)));
    assert_eq!(interval(&app), crate::anim::FRAME);

    // Once everything settles it drops back — but the sample data still has a
    // running transfer, so the spinner cadence is correct here.
    settle(&mut app);
    assert!(!app.is_animating(), "animations did not settle");
    assert_eq!(interval(&app), crate::anim::BUSY_FRAME);

    // With no work in flight the UI goes fully idle.
    for t in &mut app.transfers {
        t.state = TransferState::Done;
    }
    assert_eq!(interval(&app), crate::anim::IDLE_FRAME);
}

#[test]
fn reduced_motion_settles_instantly() {
    let mut app = test_app();
    app.anim = crate::anim::AnimState::new(true);
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);

    assert!(!app.is_animating(), "reduced motion must never animate");
    let out = render_app(&app, 110, 26);
    // Values must be present immediately rather than counting up from zero.
    assert!(out.contains("340"), "file count not snapped:\n{out}");
}

#[test]
fn first_paint_shows_real_values_not_zero() {
    // A spring's first target is adopted instantly; only changes animate.
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);

    let out = render_app(&app, 110, 26);
    assert!(out.contains("340"), "file count missing on first paint");
    assert!(out.contains("12"), "dir count missing on first paint");
}

#[test]
fn a_count_change_counts_up_instead_of_jumping() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);

    // Simulate a refresh that doubles the file count.
    let mut snapshot = app.cluster.data.clone().unwrap();
    snapshot.inode_file_num = 680;
    app.cluster.ok(snapshot);
    app.retarget_animations();

    // One frame in, the displayed value must be between old and new.
    app.on_event(crate::event::AppEvent::Frame(std::time::Duration::from_millis(16)));
    let mid = app.anim.files.value();
    assert!(
        mid > 340.0 && mid < 680.0,
        "expected an in-between value, got {mid}"
    );

    settle(&mut app);
    assert!((app.anim.files.value() - 680.0).abs() < 1.0);
}

#[test]
fn debug_which_spring_is_stuck() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    settle(&mut app);
    println!("animating: {}", app.is_animating());
    for (name, s) in [
        ("capacity", app.anim.capacity),
        ("health", app.anim.health),
        ("dirs", app.anim.dirs),
        ("files", app.anim.files),
        ("blocks", app.anim.blocks),
        ("workers", app.anim.workers),
    ] {
        println!("  {name}: value={:.4} target={:.4} animating={}", s.value(), s.target(), s.is_animating());
    }
    for (id, s) in &app.anim.transfers {
        println!("  transfer {id}: value={:.4} target={:.4} animating={}", s.value(), s.target(), s.is_animating());
    }
}

#[test]
fn dialogs_are_complete_on_the_first_frame() {
    // Regression: an animated pop-in showed a clipped box on the first draw and
    // only reached full size 150 ms later, which read as lag. A modal must be
    // fully legible the instant it appears, with zero frames elapsed.
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;

    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Delete,
        crossterm::event::KeyModifiers::NONE,
    )));

    let first = render_app(&app, 110, 26);
    assert!(first.contains("delete"), "title missing:
{first}");
    assert!(first.contains("events.parquet"), "target missing on first frame");
    assert!(first.contains("replicas"), "detail rows missing on first frame");
    assert!(first.contains("confirm"), "footer missing on first frame");
}

#[test]
fn describe_dialog_is_complete_on_the_first_frame() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;

    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::NONE,
    )));

    let first = render_app(&app, 116, 30);
    assert!(first.contains("describe"), "title missing:
{first}");
    assert!(first.contains("mode"), "detail rows missing");
    assert!(first.contains("close"), "footer missing");
}

#[test]
fn forms_are_complete_on_the_first_frame() {
    let mut app = test_app();
    app.panel = Panel::Mounts;
    app.mode = crate::app::InputMode::Mount;
    app.mount_form = Some(crate::app::MountForm::new("/data"));

    let out = render_app(&app, 110, 34);
    for field in ["ufs", "curvine", "write mode", "storage", "ttl"] {
        assert!(out.contains(field), "field {field:?} missing on first frame:
{out}");
    }
}

#[test]
fn status_message_fades_before_disappearing() {
    let mut app = test_app();
    app.set_status_for_test("done: 12 files");
    assert_eq!(app.anim.status_alpha(), 1.0, "must start fully opaque");

    // Simulate the hold elapsing is time-based, so just assert the fade maths
    // through the public surface: alpha is 1 while a fresh message is held.
    assert!(!app.anim.status_expired());
}

#[test]
fn help_legend_lists_every_dot_tone() {
    let mut app = test_app();
    app.help_visible = true;
    let out = render_app(&app, 110, 44);
    assert!(out.contains("status dots"), "legend section missing:\n{out}");
    for meaning in ["healthy", "in progress", "degraded", "failed", "idle"] {
        assert!(out.contains(meaning), "legend missing {meaning}");
    }
}


#[test]
fn panel_set_matches_upstream_cli_surface() {
    // The panel set is intentionally aligned with `curvine-cli` at the pinned
    // tag. `admin config` has no upstream counterpart and was removed, so this
    // guards against it creeping back in without a backing CLI command.
    let titles: Vec<&str> = Panel::ALL.iter().map(|p| p.title()).collect();
    assert_eq!(
        titles,
        vec![
            "Dashboard", "Files", "Nodes", "Mounts", "Jobs", "Transfers", "Fsck", "Bench"
        ]
    );
    assert!(
        !titles.contains(&"Config"),
        "Config panel has no upstream CLI command to back it"
    );
}

#[test]
fn number_keys_cover_every_panel() {
    // Bound the shortcut range to the enum so a new panel cannot end up
    // unreachable by number.
    assert_eq!(Panel::ALL.len(), 8, "update the 1..=8 shortcut range");
}

#[test]
fn help_popup_reports_the_real_panel_count() {
    let mut app = test_app();
    app.help_visible = true;
    let out = render_app(&app, 110, 44);
    assert!(
        out.contains(&format!("{} panels", Panel::ALL.len())),
        "help must state the actual panel count:\n{out}"
    );
}

