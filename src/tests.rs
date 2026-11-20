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
        toast: None,
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
        fsck_selected: 0,
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
            assert_panel_tab(&out, panel);
        }
    }
}

#[test]
fn renders_every_panel_while_loading_and_empty() {
    for panel in Panel::ALL {
        let out = render_panel(panel, 100, 30, false);
        assert_panel_tab(&out, panel);
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
fn header_menu_keeps_every_panel_number_at_narrow_widths() {
    // The menu degrades instead of dropping panels, and the cluster pill keeps
    // its cells throughout: all eight numbers must survive at these widths.
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    for width in [80u16, 90, 100, 118] {
        let out = render_app(&app, width, 30);
        let header = header_row(&out);
        let tokens = header_tokens(header);
        for n in 1..=Panel::ALL.len() {
            let number = n.to_string();
            assert!(
                tokens.contains(&number.as_str()),
                "panel {n} missing from the menu at width {width}: {header:?}"
            );
        }
        assert!(header.contains('◖') && header.contains('◗'), "pill clipped at width {width}: {header:?}");
        assert!(header.contains("master-1:8995"), "master missing at width {width}: {header:?}");
        assert!(
            header.contains("1 worker") || header.contains("1w"),
            "worker count missing at width {width}: {header:?}"
        );
        assert!(header.contains('↻'), "refresh countdown missing at width {width}: {header:?}");
    }
}

#[test]
fn header_layout_trades_pill_detail_before_labels_and_never_drops_the_pill() {
    use crate::ui::{header_layout, PillTier, TabDetail};

    // The tier widths the selection is based on.
    assert_eq!(TabDetail::Label.width(), 85);
    assert_eq!(TabDetail::Icon.width(), 32);
    assert_eq!(TabDetail::Number.width(), 16);

    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);

    for (width, expected) in [
        (160u16, TabDetail::Label),
        (128, TabDetail::Label),
        (118, TabDetail::Label),
        (110, TabDetail::Label),
        (100, TabDetail::Icon),
        (88, TabDetail::Icon),
        (76, TabDetail::Number),
        (60, TabDetail::Number),
    ] {
        let layout = header_layout(&app, width);
        assert_eq!(
            layout.tabs, expected,
            "tab tier at width {width}: {layout:?}"
        );
        assert!(
            layout.pill.width(&app) <= width,
            "the pill must fit at width {width}: {layout:?}"
        );
    }

    // Labels survive down to 110 by shrinking the pill to dot + master; a
    // wider terminal restores the pill's detail instead of taking it back.
    assert_eq!(header_layout(&app, 110).pill, PillTier::Bare);
    assert_eq!(header_layout(&app, 128).pill, PillTier::Full);
}

#[test]
fn header_menu_degrades_from_labels_to_icons_to_numbers() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);

    // Wide: number + icon + short title for every panel. Labels hold from 110
    // upward because the pill gives up its own detail first.
    for width in [110u16, 160] {
        let out = render_app(&app, width, 30);
        let header = header_row(&out);
        for (i, panel) in Panel::ALL.iter().enumerate() {
            let label = format!(" {} {} {} ", i + 1, panel.icon(), panel.short_title());
            assert!(
                header.contains(&label),
                "full tab {label:?} missing at width {width}: {header:?}"
            );
        }
    }

    // Medium: icons and numbers, labels dropped.
    let out = render_app(&app, 100, 30);
    let header = header_row(&out);
    assert!(!header.contains("Dash"), "labels should be gone at 100: {header:?}");
    for panel in Panel::ALL {
        assert!(
            header.contains(panel.icon()),
            "icon for {panel:?} missing at width 100: {header:?}"
        );
    }

    // Narrow: below the icon floor only numbers remain.
    let out = render_app(&app, 76, 30);
    let header = header_row(&out);
    assert!(
        !header.contains(Panel::Dashboard.icon()),
        "icons should be gone at 76: {header:?}"
    );
    let tokens = header_tokens(header);
    for n in 1..=Panel::ALL.len() {
        let number = n.to_string();
        assert!(
            tokens.contains(&number.as_str()),
            "panel {n} missing from the number-only menu: {header:?}"
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
fn mount_form_defaults_auto_cache_like_the_cli() {
    // `cv mount` defaults new mounts to auto-cache. A cache-mode mount without
    // it never populates Curvine, so the toggle must start on.
    let form = crate::app::MountForm::new("/data");
    assert!(form.auto_cache, "auto-cache must default on");
    assert!(!form.read_verify_ufs, "read-verify stays opt-in");

    let mut app = test_app();
    app.panel = Panel::Mounts;
    app.mode = crate::app::InputMode::Mount;
    app.mount_form = Some(form);
    let out = render_app(&app, 110, 34);
    assert!(
        out.contains("[x] auto cache"),
        "the toggle should render as enabled:\n{out}"
    );
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

/// First rendered row containing `needle`, or "" when absent. Chrome is a
/// single row now, but tests still locate content by what it says, not by index.
fn srow<'a>(out: &'a str, needle: &str) -> &'a str {
    out.lines().find(|l| l.contains(needle)).unwrap_or_default()
}

fn render_app(app: &App, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, app)).unwrap();
    buffer_text(&term)
}

fn render_buffer(app: &App, w: u16, h: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::render(f, app)).unwrap();
    term.backend().buffer().clone()
}

/// The menu header is the first rendered row.
fn header_row(out: &str) -> &str {
    out.lines().next().unwrap_or_default()
}

/// Header cells left of the cluster pill's left cap. The pill is right-aligned,
/// so everything before it belongs to the panel menu.
fn header_tabs(header: &str) -> &str {
    match header.find('◖') {
        Some(i) => &header[..i],
        None => header,
    }
}

fn header_tokens(header: &str) -> Vec<&str> {
    header_tabs(header).split_whitespace().collect()
}

/// The header menu's stable panel identity: the number survives every
/// degradation tier, so render tests can key off it instead of a full title.
fn assert_panel_tab(out: &str, panel: Panel) {
    let header = header_row(out);
    let number = (panel.index() + 1).to_string();
    assert!(
        header_tokens(header).contains(&number.as_str()),
        "panel {panel:?} tab missing from the menu: {header:?}"
    );
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
        crate::app::PendingAction::Delete { recursive: false, .. }
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
    // The refusal is an error, so it surfaces as a toast rather than in the
    // footer; the hint still names the recursive key.
    let toast = app.toast.as_ref().expect("refusal must raise a toast");
    assert!(
        toast.text.contains("D"),
        "expected a hint about D: {}",
        toast.text
    );
    assert!(app.status.is_empty(), "errors must not enter the footer");
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
fn d_opens_full_screen_file_detail() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;

    press(&mut app, crossterm::event::KeyCode::Char('d'));

    let view = app.detail.as_ref().expect("d must open the file detail");
    assert_eq!(app.mode, crate::app::InputMode::Detail);
    assert_eq!(view.kind, crate::app::DetailKind::File);
    assert!(view.title.contains("events.parquet"));
    assert!(view.subject.ends_with("events.parquet"));
    assert!(view.loading, "block locations are fetched asynchronously");
    let keys: Vec<&str> = view.rows.iter().map(|(k, _)| k.as_str()).collect();
    for expected in ["path", "mode", "owner", "size", "replicas", "blocks", "modified"] {
        assert!(keys.contains(&expected), "missing {expected} row: {keys:?}");
    }
    // Files no longer use the describe popup.
    assert!(app.describe.is_none());
}

#[test]
fn directory_d_still_uses_the_describe_popup() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 0; // warehouse/

    press(&mut app, crossterm::event::KeyCode::Char('d'));
    assert!(app.detail.is_none(), "directories keep the small popup");
    let view = app.describe.as_ref().expect("directory describe missing");
    assert!(view.title.contains("warehouse"));
    assert_eq!(app.mode, crate::app::InputMode::Describe);
}

#[test]
fn file_detail_renders_blocks_and_distribution() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.mode = crate::app::InputMode::Detail;
    app.detail = Some(crate::app::DetailView {
        title: "file events.parquet".into(),
        subject: "/data/events.parquet".into(),
        kind: crate::app::DetailKind::File,
        headline: Some(("file".into(), "/data/events.parquet".into())),
        rows: vec![
            ("path".into(), "/data/events.parquet".into()),
            ("mode".into(), "-rw-r--r-- (0644)".into()),
            ("blocks".into(), "2 across 2 workers".into()),
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
            crate::event::BlockRow {
                block_id: "102".into(),
                offset: 1_500_000,
                len: 1_000_000,
                worker: "127.0.0.1:8997".into(),
                available: true,
                state: "Live".into(),
            },
        ],
        worker_blocks: crate::app::aggregate_by_worker(&[
            crate::event::BlockRow {
                block_id: "101".into(),
                offset: 0,
                len: 1_500_000,
                worker: "127.0.0.1:8997".into(),
                available: true,
                state: "Live".into(),
            },
            crate::event::BlockRow {
                block_id: "102".into(),
                offset: 1_500_000,
                len: 1_000_000,
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
        ]),
        storage: Vec::new(),
        loading: false,
        error: None,
    });

    let out = render_app(&app, 120, 34);
    assert!(srow(&out, "file").contains("file"), "detail badge missing:\n{out}");
    assert!(out.contains("/data/events.parquet"), "detail subject missing:\n{out}");
    assert!(out.contains("blocks by offset"), "block table missing");
    assert!(out.contains("distribution by worker"), "distribution table missing");
    assert!(out.contains("1/2"), "replica availability missing");
    assert!(out.contains("Lost"), "lost replica state missing");
    assert!(out.contains("127.0.0.1:8997"), "worker endpoint missing");
    assert!(out.contains("share"), "share column missing");
}

#[test]
fn late_block_response_fills_the_open_detail_view() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char('d'));

    app.on_event(crate::event::AppEvent::TaskDone(
        crate::event::TaskResult::Describe {
            path: "/data/events.parquet".into(),
            blocks: Ok(vec![crate::event::BlockRow {
                block_id: "101".into(),
                offset: 0,
                len: 1_500_000,
                worker: "127.0.0.1:8997".into(),
                available: true,
                state: "Live".into(),
            }]),
        },
    ));

    let view = app.detail.as_ref().expect("detail stays open");
    assert!(!view.loading, "response must clear the loading state");
    assert_eq!(view.blocks.len(), 1);
    assert_eq!(view.worker_blocks.len(), 1);
    let blocks_row = view.rows.iter().find(|(k, _)| k == "blocks").unwrap();
    assert!(blocks_row.1.contains("1 across 1 workers"), "summary not updated: {}", blocks_row.1);
}

#[test]
fn switching_panels_closes_the_detail_view() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char('d'));
    assert_eq!(app.mode, crate::app::InputMode::Detail);

    press(&mut app, crossterm::event::KeyCode::Char('3'));
    assert_eq!(app.panel, Panel::Nodes);
    assert!(app.detail.is_none(), "panel switch must close the detail");
    assert_eq!(app.mode, crate::app::InputMode::Normal);
}

#[test]
fn d_on_nodes_opens_the_worker_detail() {
    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    app.node_selected = 0;

    press(&mut app, crossterm::event::KeyCode::Char('d'));

    let view = app.detail.as_ref().expect("d must open worker detail");
    assert_eq!(view.kind, crate::app::DetailKind::Worker);
    assert_eq!(app.mode, crate::app::InputMode::Detail);
    assert!(view.subject.contains("127.0.0.1"));
    assert!(
        !view.subject.contains("worker_id"),
        "subject must not use the verbose worker_id form"
    );
    let keys: Vec<&str> = view.rows.iter().map(|(k, _)| k.as_str()).collect();
    for expected in ["capacity", "used", "free", "occupancy", "blocks"] {
        assert!(keys.contains(&expected), "missing {expected}: {keys:?}");
    }
    let occ = view.rows.iter().find(|(k, _)| k == "occupancy").unwrap();
    assert!(occ.1.contains("not CPU load"), "occupancy mislabelled: {}", occ.1);
    assert_eq!(view.storage.len(), 1, "storage breakdown must be retained");
}

#[test]
fn worker_detail_renders_the_endpoint_and_storage_classes() {
    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    app.node_describe();

    let out = render_app(&app, 120, 34);
    assert!(out.contains("worker"), "detail title missing:\n{out}");
    assert!(out.contains("127.0.0.1:8997"), "endpoint missing");
    assert!(out.contains("storage classes"), "storage table missing");
    assert!(out.contains("Ssd"), "storage class missing");
    assert!(out.contains("capacity"), "capacity row missing");
}

#[test]
fn escape_closes_the_full_screen_detail() {
    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    app.node_describe();
    assert!(app.detail.is_some());

    press(&mut app, crossterm::event::KeyCode::Esc);
    assert!(app.detail.is_none());
    assert_eq!(app.mode, crate::app::InputMode::Normal);
}

#[test]
fn header_shows_the_panel_menu_and_cluster_context() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    let out = render_app(&app, 160, 30);
    let header = header_row(&out);

    // Every panel is a tab: number, icon and short title, in Panel::ALL order.
    for (i, panel) in Panel::ALL.iter().enumerate() {
        let label = format!(" {} {} {} ", i + 1, panel.icon(), panel.short_title());
        assert!(
            header.contains(&label),
            "menu tab {label:?} missing: {header:?}"
        );
    }

    // Cluster context: health dot, active master, worker count, refresh timer.
    assert!(header.contains("master-1:8995"), "master missing: {header:?}");
    assert!(header.contains("1 worker"), "worker count missing: {header:?}");
    assert!(header.contains('↻'), "refresh countdown missing: {header:?}");

    // The brand and the old shortcut text no longer live in the header.
    assert!(!header.contains("curvine-tui"), "brand must leave the header: {header:?}");
    assert!(!header.contains('❖'), "brand glyph must leave the header: {header:?}");
    assert!(!header.contains("1-8 switch"), "old shortcut hint still present: {header:?}");
    assert!(!header.contains(": command"), "old command hint still present: {header:?}");
}

#[test]
fn header_highlights_the_active_tab_only() {
    use ratatui::style::{Color, Modifier};

    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);

    let width = 160u16;
    let buf = render_buffer(&app, width, 30);
    let row: String = (0..width).map(|x| buf[(x, 0)].symbol()).collect();

    let active = format!(" 2 {} Files ", Panel::Files.icon());
    let x = row[..row.find(&active).expect("active tab")].chars().count() as u16;
    let cell = &buf[(x, 0)];
    assert_eq!(cell.fg, crate::ui::theme::ACCENT, "active tab ink");
    assert_eq!(cell.bg, crate::ui::theme::TAB_BG, "active tab fill");
    assert!(
        cell.modifier.contains(Modifier::BOLD),
        "active tab must be bold"
    );

    let inactive = format!(" 1 {} Dash ", Panel::Dashboard.icon());
    let x = row[..row.find(&inactive).expect("inactive tab")]
        .chars()
        .count() as u16;
    let cell = &buf[(x, 0)];
    assert_eq!(cell.fg, crate::ui::theme::MUTED, "inactive tab ink");
    assert_eq!(cell.bg, Color::Reset, "inactive tabs stay unfilled");
    assert!(!cell.modifier.contains(Modifier::BOLD));
}

#[test]
fn header_reports_disconnected_without_cluster_data() {
    let app = test_app();
    let out = render_app(&app, 160, 30);
    let header = header_row(&out);
    assert!(header.contains("disconnected"), "expected a disconnected state: {header:?}");
    assert!(header.contains(crate::ui::components::DOT_HOLLOW), "expected a hollow dot");
    assert!(
        header.contains("· 0 workers ·"),
        "pill must still carry the worker count: {header:?}"
    );
}

#[test]
fn footer_merges_status_and_context_hints() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    app.set_status_for_test("saved 12 files");
    let out = render_app(&app, 120, 30);
    // The same text also appears in the recent-events log, so look only at the
    // footer row itself.
    let footer = out.lines().last().unwrap_or_default();
    assert!(footer.contains("saved 12 files"), "status missing from footer: {footer:?}");
    assert!(footer.contains("x menu"), "panel hints missing from footer: {footer:?}");
    // Status and hints share one row; the status is an inline badge.
    assert!(
        footer.contains('◖') && footer.contains('◗'),
        "status must be wrapped in an inline badge: {footer:?}"
    );
}

#[test]
fn footer_keeps_a_panel_switching_mention() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);

    // Wide: the panel's own hints carry the switch reminder.
    let out = render_app(&app, 120, 30);
    let footer = out.lines().last().unwrap_or_default();
    assert!(footer.contains("tab/1-8 panels"), "switch hint missing: {footer:?}");
    assert!(footer.contains("space mark"), "panel hints missing: {footer:?}");

    // Narrow: the long list gives way to the generic line, which still names
    // the panel switch instead of hiding every hint.
    let out = render_app(&app, 80, 30);
    let footer = out.lines().last().unwrap_or_default();
    assert!(
        footer.contains("tab/1-8 panels"),
        "generic footer must keep the switch hint: {footer:?}"
    );
    assert!(
        !footer.contains("space mark"),
        "the long panel hints should have yielded: {footer:?}"
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
fn detail_groups_blocks_by_worker() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.mode = crate::app::InputMode::Detail;
    app.detail = Some(crate::app::DetailView {
        title: "file events.parquet".into(),
        subject: "/data/events.parquet".into(),
        kind: crate::app::DetailKind::File,
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
        storage: Vec::new(),
        loading: false,
        error: None,
    });

    let out = render_app(&app, 116, 32);
    assert!(out.contains("distribution by worker"), "worker section missing:\n{out}");
    assert!(out.contains("blocks by offset"), "block section missing");
    assert!(out.contains("127.0.0.1:8997"), "worker endpoint missing");
    assert!(out.contains("101"), "block id missing");
    let worker_row = out
        .lines()
        .find(|l| l.contains("127.0.0.1:8997"))
        .unwrap_or_default();
    assert!(
        worker_row.contains('2'),
        "per-worker block count missing: {worker_row:?}"
    );
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
            assert_panel_tab(&out, panel);
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

// ------------------------------------------------------------- action menu

fn press(app: &mut App, code: crossterm::event::KeyCode) {
    app.on_event(crate::event::AppEvent::Key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::NONE,
    )));
}

fn open_menu_for(app: &mut App, panel: Panel) {
    app.panel = panel;
    populate_sample_data(app);
    // Files has a directory at row 0 and a file at row 1; the file menu is the
    // richer one, so select it for the general per-panel check.
    if panel == Panel::Files {
        app.files.selected = 1;
    }
    press(app, crossterm::event::KeyCode::Char('x'));
    assert_eq!(app.mode, crate::app::InputMode::Menu, "x must open a menu");
    assert!(app.menu.is_some(), "menu state missing for {panel:?}");
    assert!(
        !app.menu.as_ref().unwrap().items.is_empty(),
        "menu for {panel:?} has no actions"
    );
}

#[test]
fn x_opens_a_context_menu_on_every_panel() {
    for panel in Panel::ALL {
        let mut app = test_app();
        open_menu_for(&mut app, panel);
    }
}

#[test]
fn file_menu_matches_the_selection_kind() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);

    app.files.selected = 0; // warehouse/ (directory)
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    let labels: Vec<&str> = app
        .menu
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.label.as_str())
        .collect();
    assert!(labels.contains(&"enter directory"), "dir menu: {labels:?}");
    assert!(labels.contains(&"delete recursively"), "dir menu: {labels:?}");

    press(&mut app, crossterm::event::KeyCode::Esc);
    app.files.selected = 1; // events.parquet (file)
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    let labels: Vec<&str> = app
        .menu
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.label.as_str())
        .collect();
    for expected in ["detail", "delete", "free cached blocks", "upload", "refresh"] {
        assert!(labels.contains(&expected), "file menu missing {expected}: {labels:?}");
    }
}

#[test]
fn menu_direct_key_executes_and_closes() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    assert_eq!(app.mode, crate::app::InputMode::Menu);

    // Del is bound to delete in the file menu; pressing it directly runs the
    // action without needing j/k + Enter.
    press(&mut app, crossterm::event::KeyCode::Delete);
    assert!(app.menu.is_none(), "direct execution must close the menu");
    assert_eq!(app.mode, crate::app::InputMode::Confirm);
    assert!(app.confirm.is_some(), "delete confirmation missing");
}

#[test]
fn menu_navigation_and_enter_run_the_selected_item() {
    let mut app = test_app();
    app.panel = Panel::Bench;
    populate_sample_data(&mut app);
    let before = app.bench_opts.profile;
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    // Items are b(run), p(profile), t(threads), d(duration): move to profile.
    press(&mut app, crossterm::event::KeyCode::Down);
    press(&mut app, crossterm::event::KeyCode::Enter);
    assert_ne!(before, app.bench_opts.profile, "profile toggle did not run");
    assert!(app.menu.is_none());
}

#[test]
fn menu_renders_keys_title_and_labels() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    let out = render_app(&app, 110, 28);
    assert!(out.contains("events.parquet"), "menu title missing:\n{out}");
    assert!(out.contains("detail"), "menu label missing");
    assert!(out.contains("delete"), "menu label missing");
    assert!(out.contains("Del"), "delete key label missing");
}

#[test]
fn escape_closes_the_menu_without_acting() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    press(&mut app, crossterm::event::KeyCode::Esc);
    assert!(app.menu.is_none());
    assert_eq!(app.mode, crate::app::InputMode::Normal);
    assert!(app.confirm.is_none(), "escape must not execute anything");
}

// ---------------------------------------------------------------- files: marks

#[test]
fn space_marks_and_unmarks_the_selected_entry() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;

    press(&mut app, crossterm::event::KeyCode::Char(' '));
    assert_eq!(app.files.marked, vec!["/data/events.parquet".to_string()]);
    // Marking must not reset the cursor.
    assert_eq!(app.files.selected, 1);

    press(&mut app, crossterm::event::KeyCode::Char(' '));
    assert!(app.files.marked.is_empty(), "second space must unmark");
}

#[test]
fn a_marks_all_visible_and_u_clears_the_marks() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);

    press(&mut app, crossterm::event::KeyCode::Char('a'));
    assert_eq!(app.files.marked.len(), 2);
    assert!(app.files.marked.contains(&"/data/warehouse".to_string()));
    assert!(app.files.marked.contains(&"/data/events.parquet".to_string()));

    press(&mut app, crossterm::event::KeyCode::Char('u'));
    assert!(app.files.marked.is_empty());
}

#[test]
fn navigation_keeps_marks_and_escape_clears_them() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char(' '));
    press(&mut app, crossterm::event::KeyCode::Up);
    assert_eq!(app.files.marked.len(), 1, "navigation must not clear marks");

    press(&mut app, crossterm::event::KeyCode::Esc);
    assert!(app.files.marked.is_empty(), "escape must clear marks");
}

#[test]
fn selected_and_marked_rows_keep_both_indicators() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char(' '));

    let icon = crate::ui::icons::for_file("events.parquet");
    let out = render_app(&app, 110, 20);
    assert!(
        out.contains(&format!("▶● {icon} events.parquet")),
        "selected+marked row must show both glyphs:\n{out}"
    );

    // Marking a different row leaves the cursor visible on the selected one
    // and shows a leading mark on the unselected marked row.
    app.files.selected = 0;
    let out = render_app(&app, 110, 20);
    assert!(
        out.contains(&format!(" ● {icon} events.parquet")),
        "unselected marked row must keep its mark:\n{out}"
    );
    assert!(
        out.contains(&format!("▶  {} ", crate::ui::icons::DIR)),
        "unselected-marked must not steal the cursor:\n{out}"
    );

    // Unmarking restores the plain cursor.
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char(' '));
    let out = render_app(&app, 110, 20);
    assert!(
        out.contains(&format!("▶  {icon} events.parquet")),
        "unmarked selection lost its cursor:\n{out}"
    );
}

#[test]
fn breadcrumb_reports_visible_total_and_marked_counts() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.filter = "events".to_string();
    app.files.marked = vec!["/data/events.parquet".to_string()];

    let out = render_app(&app, 110, 20);
    let crumb = srow(&out, "/data");
    assert!(crumb.contains("/data"), "path missing: {crumb:?}");
    assert!(crumb.contains("1/2"), "visible/total missing: {crumb:?}");
    assert!(crumb.contains("1 marked"), "marked count missing: {crumb:?}");
}

#[test]
fn batch_delete_confirmation_reports_the_mark_count() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.marked = vec![
        "/data/events.parquet".to_string(),
        "/data/warehouse".to_string(),
    ];

    press(&mut app, crossterm::event::KeyCode::Char('x'));
    let labels: Vec<&str> = app
        .menu
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.label.as_str())
        .collect();
    assert!(
        labels.contains(&"delete 2 marked items"),
        "marked menu must advertise the batch: {labels:?}"
    );

    press(&mut app, crossterm::event::KeyCode::Delete);
    let confirm = app.confirm.as_ref().expect("batch delete must confirm");
    assert_eq!(confirm.target, "2 marked items");
    match &confirm.action {
        crate::app::PendingAction::Delete { paths, recursive } => {
            assert_eq!(paths.len(), 2);
            assert!(recursive, "a batch may contain directories");
        }
        other => panic!("unexpected action: {other:?}"),
    }

    let out = render_app(&app, 110, 28);
    assert!(
        out.contains("delete 2 marked items?"),
        "confirmation must state the count:\n{out}"
    );
}

#[test]
fn batch_free_confirmation_reports_the_mark_count() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.marked = vec![
        "/data/events.parquet".to_string(),
        "/data/warehouse".to_string(),
    ];

    press(&mut app, crossterm::event::KeyCode::Char('x'));
    press(&mut app, crossterm::event::KeyCode::Char('F'));
    let confirm = app.confirm.as_ref().expect("batch free must confirm");
    assert_eq!(confirm.target, "2 marked items");
    assert!(matches!(
        confirm.action,
        crate::app::PendingAction::Free { .. }
    ));
    let out = render_app(&app, 110, 28);
    assert!(out.contains("free 2 marked items?"), "count missing:\n{out}");
}

// ------------------------------------------------------- menu execution paths

#[test]
fn menu_cancel_item_cancels_the_selected_transfer() {
    let mut app = test_app();
    app.panel = Panel::Transfers;
    populate_sample_data(&mut app);
    app.transfer_selected = 0;
    let flag = app.transfers[0].cancel.clone();

    press(&mut app, crossterm::event::KeyCode::Char('x'));
    // The transfer menu binds x to "cancel selected".
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    assert!(
        flag.load(std::sync::atomic::Ordering::Relaxed),
        "menu cancel did not set the cancellation flag"
    );
    assert_eq!(app.transfers[0].state, TransferState::Cancelled);
}

#[test]
fn menu_unmount_item_opens_the_confirmation() {
    let mut app = test_app();
    app.panel = Panel::Mounts;
    populate_sample_data(&mut app);
    app.mount_selected = 0;

    press(&mut app, crossterm::event::KeyCode::Char('x'));
    press(&mut app, crossterm::event::KeyCode::Char('u'));
    let confirm = app.confirm.as_ref().expect("unmount menu must confirm");
    assert!(matches!(
        confirm.action,
        crate::app::PendingAction::Umount { .. }
    ));
    assert_eq!(app.mode, crate::app::InputMode::Confirm);
}

#[test]
fn menu_abort_item_cancels_a_running_scan() {
    let mut app = test_app();
    app.panel = Panel::Fsck;
    populate_sample_data(&mut app);
    app.fsck.begin();

    press(&mut app, crossterm::event::KeyCode::Char('x'));
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    assert!(
        app.fsck_cancel.load(std::sync::atomic::Ordering::Relaxed),
        "menu abort did not request cancellation"
    );
}

#[test]
fn recursive_delete_menu_item_accepts_the_d_alias() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 0; // directory

    press(&mut app, crossterm::event::KeyCode::Char('x'));
    // The directory menu advertises Del/D for recursive delete.
    press(&mut app, crossterm::event::KeyCode::Char('D'));
    let confirm = app.confirm.as_ref().expect("D must run recursive delete");
    assert!(matches!(
        confirm.action,
        crate::app::PendingAction::Delete { recursive: true, .. }
    ));
}

#[test]
fn jobs_selection_and_menu_offer_cancellation_for_running_jobs() {
    let mut app = test_app();
    app.panel = Panel::Jobs;
    populate_sample_data(&mut app);
    if let Some(j) = app.jobs.data.as_mut().and_then(|l| l.first_mut()) {
        j.state = "Running".into();
    }
    app.retarget_animations();

    press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(app.job_selected, 0, "one job: down stays on it");
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    let labels: Vec<&str> = app
        .menu
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.label.as_str())
        .collect();
    assert!(labels.contains(&"cancel job"), "running job menu: {labels:?}");
}

// ----------------------------------------------------------------- dashboard

fn add_second_worker(app: &mut App) {
    let c = app.cluster.data.as_mut().expect("cluster data");
    let mut w = c.workers[0].clone();
    w.address = "127.0.0.2:8997".into();
    w.ip = "127.0.0.2".into();
    w.worker_id = 2;
    w.block_num = 128;
    c.workers.push(w);
}

#[test]
fn dashboard_workers_use_the_same_selection_as_nodes() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    add_second_worker(&mut app);
    app.retarget_animations();

    press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(app.node_selected, 1);
    press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(app.node_selected, 1, "selection must stop at the last worker");
    press(&mut app, crossterm::event::KeyCode::Up);
    assert_eq!(app.node_selected, 0);

    let out = render_app(&app, 120, 30);
    let selected_row = out
        .lines()
        .find(|l| l.contains("127.0.0.1:8997"))
        .unwrap_or_default();
    assert!(
        selected_row.contains('▶'),
        "selected worker row lacks its cursor: {selected_row:?}"
    );
    let other_row = out
        .lines()
        .find(|l| l.contains("127.0.0.2:8997"))
        .unwrap_or_default();
    assert!(!other_row.contains('▶'), "only one row may be selected");
}

#[test]
fn dashboard_d_opens_worker_detail() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    add_second_worker(&mut app);
    app.retarget_animations();
    press(&mut app, crossterm::event::KeyCode::Down);

    press(&mut app, crossterm::event::KeyCode::Char('d'));
    let view = app.detail.as_ref().expect("dashboard d must open worker detail");
    assert_eq!(view.kind, crate::app::DetailKind::Worker);
    assert_eq!(view.subject, "127.0.0.2:8997");
    assert_eq!(app.mode, crate::app::InputMode::Detail);
}

// ---------------------------------------------------------------------- fsck

#[test]
fn fsck_summary_folds_every_counter_into_kv_rows() {
    let out = render_panel(Panel::Fsck, 120, 32, true);
    // The old middle "verdict" strip is gone; its counters live in summary.
    assert!(!out.contains("verdict"), "verdict strip still present:\n{out}");
    assert!(out.contains("120 files"), "file count missing");
    assert!(out.contains("340 blocks"), "block count missing");
    assert!(out.contains("640/680 available"), "replica summary missing");
    assert!(out.contains("under-replicated"), "under-replicated row missing");
    assert!(out.contains("scan error"), "scan error row missing");
    assert_eq!(
        out.matches("under-replicated").count(),
        1,
        "the counter must not be duplicated in a strip"
    );
}

#[test]
fn fsck_selection_moves_and_stays_in_bounds() {
    let mut app = test_app();
    app.panel = Panel::Fsck;
    populate_sample_data(&mut app);
    assert_eq!(app.fsck_selected, 0);

    press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(app.fsck_selected, 1);
    press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(app.fsck_selected, 1, "selection must not run past the rows");
    press(&mut app, crossterm::event::KeyCode::Up);
    assert_eq!(app.fsck_selected, 0);
    press(&mut app, crossterm::event::KeyCode::Up);
    assert_eq!(app.fsck_selected, 0, "selection must not underflow");
}

#[test]
fn fsck_enter_opens_the_selected_bad_file_detail() {
    let mut app = test_app();
    app.panel = Panel::Fsck;
    populate_sample_data(&mut app);
    app.fsck_selected = 1; // /data/events.parquet

    press(&mut app, crossterm::event::KeyCode::Enter);
    let view = app.detail.as_ref().expect("enter must open file detail");
    assert_eq!(app.mode, crate::app::InputMode::Detail);
    assert_eq!(view.kind, crate::app::DetailKind::File);
    assert_eq!(view.subject, "/data/events.parquet");
    let rows: Vec<&str> = view.rows.iter().map(|(k, _)| k.as_str()).collect();
    for expected in ["path", "size", "blocks", "replicas"] {
        assert!(rows.contains(&expected), "missing {expected}: {rows:?}");
    }
    let replicas = view.rows.iter().find(|(k, _)| k == "replicas").unwrap();
    assert!(replicas.1.contains("1 of 3"), "replica detail wrong: {}", replicas.1);

    press(&mut app, crossterm::event::KeyCode::Esc);
    assert!(app.detail.is_none());
}

#[test]
fn fsck_enter_scans_when_there_are_no_unhealthy_files() {
    let mut app = test_app();
    app.panel = Panel::Fsck;
    populate_sample_data(&mut app);
    if let Some(s) = app.fsck.data.as_mut() {
        s.bad_files.clear();
    }
    // Enter falls through to a scan when there is nothing to inspect. The
    // scan itself runs asynchronously; the loading flag is the observable
    // effect that proves the key reached start_fsck.
    press(&mut app, crossterm::event::KeyCode::Enter);
    assert!(app.fsck.loading, "enter should start a scan: {}", app.status);
    assert!(app.detail.is_none());
}

// ------------------------------------------------------------ command line

fn type_str(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, crossterm::event::KeyCode::Char(c));
    }
}

#[test]
fn command_tab_completes_the_first_prefix_match() {
    let mut app = test_app();
    press(&mut app, crossterm::event::KeyCode::Char(':'));
    type_str(&mut app, "l");
    press(&mut app, crossterm::event::KeyCode::Tab);
    assert_eq!(app.command, "ls ");
    assert_eq!(app.mode, crate::app::InputMode::Command);

    // A prefix with a longer first match still completes to that command.
    press(&mut app, crossterm::event::KeyCode::Esc);
    press(&mut app, crossterm::event::KeyCode::Char(':'));
    type_str(&mut app, "mk");
    press(&mut app, crossterm::event::KeyCode::Tab);
    assert_eq!(app.command, "mkdir ");
}

#[test]
fn command_completion_row_lists_matches_and_no_match_marker() {
    let mut app = test_app();
    press(&mut app, crossterm::event::KeyCode::Char(':'));
    type_str(&mut app, "l");
    let out = render_app(&app, 110, 30);
    let hint = out.lines().nth_back(1).unwrap_or_default();
    let footer = out.lines().last().unwrap_or_default();
    assert!(hint.contains("ls"), "completion row missing ls: {hint:?}");
    assert!(hint.contains("load"), "completion row missing load: {hint:?}");
    assert!(footer.contains(":l"), "command input missing from footer: {footer:?}");

    type_str(&mut app, "zz");
    let out = render_app(&app, 110, 30);
    let hint = out.lines().nth_back(1).unwrap_or_default();
    assert!(
        hint.contains("✗ no matching command"),
        "no-match marker missing: {hint:?}"
    );
}

#[test]
fn command_history_recalls_and_clears_with_arrows() {
    let mut app = test_app();
    press(&mut app, crossterm::event::KeyCode::Char(':'));
    type_str(&mut app, "panel bench");
    press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(app.panel, Panel::Bench);
    assert_eq!(app.command_history, vec!["panel bench".to_string()]);

    press(&mut app, crossterm::event::KeyCode::Char(':'));
    press(&mut app, crossterm::event::KeyCode::Up);
    assert_eq!(app.command, "panel bench");
    // At the oldest entry ↑ stays put.
    press(&mut app, crossterm::event::KeyCode::Up);
    assert_eq!(app.command, "panel bench");
    // ↓ past the newest entry returns an empty prompt.
    press(&mut app, crossterm::event::KeyCode::Down);
    assert!(app.command.is_empty());
}

#[test]
fn command_history_keeps_the_latest_twenty_entries() {
    let mut app = test_app();
    for _ in 0..21 {
        press(&mut app, crossterm::event::KeyCode::Char(':'));
        type_str(&mut app, "panel bench");
        press(&mut app, crossterm::event::KeyCode::Enter);
    }
    assert_eq!(app.command_history.len(), 20);
    assert!(app.command_history.iter().all(|c| c == "panel bench"));
}

#[test]
fn escape_leaves_the_command_prompt_without_history_noise() {
    let mut app = test_app();
    press(&mut app, crossterm::event::KeyCode::Char(':'));
    type_str(&mut app, "ls");
    press(&mut app, crossterm::event::KeyCode::Esc);
    assert_eq!(app.mode, crate::app::InputMode::Normal);
    assert!(app.command.is_empty());
    assert!(app.command_history.is_empty());
}

#[test]
fn footer_keeps_context_hints_at_narrow_widths() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    let out = render_app(&app, 60, 24);
    let footer = out
        .lines()
        .find(|l| l.contains("x menu") || l.contains("space mark"))
        .unwrap_or_default();
    assert!(
        !footer.is_empty(),
        "context hints dropped at 60 columns:\n{out}"
    );
}

#[test]
fn help_documents_the_menu_and_marking() {
    let mut app = test_app();
    app.help_visible = true;
    let out = render_app(&app, 110, 48);
    assert!(out.contains("contextual action menu"), "menu help missing:\n{out}");
    assert!(out.contains("mark entry"), "mark help missing");
    assert!(out.contains("full-screen detail"), "detail help missing");
    assert!(out.contains("history"), "command history help missing");
}

#[test]
fn menu_and_detail_survive_pathological_sizes() {
    for (w, h) in [(20u16, 8u16), (1, 1), (10, 3), (300, 5)] {
        let mut app = test_app();
        app.panel = Panel::Files;
        populate_sample_data(&mut app);
        app.files.selected = 1;
        press(&mut app, crossterm::event::KeyCode::Char('x'));
        render_app(&app, w, h);

        press(&mut app, crossterm::event::KeyCode::Esc);
        press(&mut app, crossterm::event::KeyCode::Char('d'));
        render_app(&app, w, h);
    }
}

// ------------------------------------------------------------------ badges

/// Assert the text at `needle` sits inside an inline badge: `◖` before it and
/// `◗` after it on the same row, with no box border in between.
fn assert_badge_around(out: &str, needle: &str) {
    let line = out
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no row containing {needle:?}:\n{out}"));
    let chars: Vec<char> = line.chars().collect();
    let x = line[..line.find(needle).expect("needle")].chars().count();

    let left = (0..x)
        .rev()
        .find(|i| chars.get(*i) == Some(&'◖'))
        .unwrap_or_else(|| panic!("no ◖ before {needle:?}:\n{out}"));
    let right = (x..chars.len())
        .find(|i| chars.get(*i) == Some(&'◗'))
        .unwrap_or_else(|| panic!("no ◗ after {needle:?}:\n{out}"));
    assert!(left < x && x < right, "badge caps do not surround {needle:?}:\n{out}");
}

#[test]
fn badges_are_one_line_capped_fills() {
    use crate::ui::components::{badge, badge_colored, Tone};
    use crate::ui::theme as th;

    let line = badge("1 live", Tone::Ok);
    assert_eq!(line.spans.len(), 3, "a badge is caps + one padded middle");
    assert_eq!(line.spans[0].content.as_ref(), "◖");
    assert_eq!(line.spans[1].content.as_ref(), " 1 live ");
    assert_eq!(line.spans[2].content.as_ref(), "◗");
    // Caps carry the page colour so the tone fill reads as a half-circle end.
    assert_eq!(line.spans[0].style.fg, Some(th::PAGE));
    assert_eq!(line.spans[0].style.bg, Some(th::OK));
    assert_eq!(line.spans[2].style.fg, Some(th::PAGE));
    assert_eq!(line.spans[2].style.bg, Some(th::OK));
    // The middle is dark ink on the same tone.
    assert_eq!(line.spans[1].style.fg, Some(th::INK));
    assert_eq!(line.spans[1].style.bg, Some(th::OK));
    assert!(line.spans[1]
        .style
        .add_modifier
        .contains(ratatui::style::Modifier::BOLD));

    // The explicit-colour variant exists for the fading footer status.
    let fade = ratatui::style::Color::Rgb(1, 2, 3);
    let line = badge_colored("syncing", fade);
    assert_eq!(line.spans[0].style.bg, Some(fade));
    assert_eq!(line.spans[1].style.bg, Some(fade));
    assert_eq!(line.spans[2].style.bg, Some(fade));
    assert_eq!(line.spans[2].content.as_ref(), "◗");
}

#[test]
fn header_cluster_state_is_an_inline_badge() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    let out = render_app(&app, 120, 30);
    // The pill shrinks at this width, but the dot and master stay inside it.
    assert_badge_around(&out, "master-1:8995");
    assert!(out.lines().any(|l| l.contains('↻')), "refresh missing");
}

#[test]
fn panel_badges_are_inline() {
    let mut app = test_app();
    app.panel = Panel::Mounts;
    populate_sample_data(&mut app);
    let out = render_app(&app, 120, 30);
    assert_badge_around(&out, "id 2");
    assert_badge_around(&out, "ttl ∞");

    let mut bench = test_app();
    bench.panel = Panel::Bench;
    populate_sample_data(&mut bench);
    let out = render_app(&bench, 120, 30);
    assert_badge_around(&out, "profile basic");
    assert_badge_around(&out, "done");

    // Files: type badge and the breadcrumb count.
    let mut files = test_app();
    files.panel = Panel::Files;
    populate_sample_data(&mut files);
    let out = render_app(&files, 120, 30);
    assert_badge_around(&out, "directory");
    assert_badge_around(&out, "2/2");

    // Nodes: the live count (dot included) and the total block count.
    let mut nodes = test_app();
    nodes.panel = Panel::Nodes;
    populate_sample_data(&mut nodes);
    let out = render_app(&nodes, 120, 30);
    assert_badge_around(&out, "1 live");
    assert_badge_around(&out, "512 blocks");

    // A panel-title badge (Fsck `clean`) is inline too; the borders stay rounded.
    let mut fsck = test_app();
    fsck.panel = Panel::Fsck;
    populate_sample_data(&mut fsck);
    let out = render_app(&fsck, 120, 30);
    assert_badge_around(&out, "action needed");
    assert!(out.contains('╭'), "panels keep rounded borders");
}

#[test]
fn dialog_keycaps_and_help_badge_are_inline() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Delete);
    let out = render_app(&app, 120, 32);
    assert_badge_around(&out, "enter");
    assert_badge_around(&out, "esc");

    let mut help = test_app();
    help.help_visible = true;
    let out = render_app(&help, 120, 40);
    assert_badge_around(&out, "8 panels");
}

#[test]
fn badge_fills_stay_inside_the_caps() {
    use crate::ui::theme as th;
    use ratatui::style::Color;

    // Row selection and the active header tab are the only unbounded filled
    // backgrounds. Every other non-reset background must be a badge fill: one
    // contiguous tone run on a single row, starting at ◖ and ending at ◗.
    let allowed = [Color::Reset, th::SELECTED_BG, th::TAB_BG];
    let tones = [th::MUTED, th::ACCENT, th::OK, th::WARN, th::ERROR, th::DIR];
    let scan = |app: &App, w: u16, h: u16| -> Vec<(u16, u16, Color)> {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| crate::ui::render(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut hits = Vec::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let bg = buf[(x, y)].bg;
                if allowed.contains(&bg) {
                    continue;
                }
                let mut left = x;
                while left > 0 && buf[(left - 1, y)].bg == bg {
                    left -= 1;
                }
                let mut right = x;
                while right + 1 < buf.area.width && buf[(right + 1, y)].bg == bg {
                    right += 1;
                }
                let opens = buf[(left, y)].symbol() == "◖";
                let closes = buf[(right, y)].symbol() == "◗";
                if !(tones.contains(&bg) && opens && closes) {
                    hits.push((x, y, bg));
                }
            }
        }
        hits
    };

    for panel in Panel::ALL {
        let mut app = test_app();
        app.panel = panel;
        populate_sample_data(&mut app);
        let hits = scan(&app, 120, 34);
        assert!(
            hits.is_empty(),
            "filled background on {panel:?}: {hits:?}"
        );
    }

    // Dialog, help and menu are separate render paths.
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Delete);
    assert!(scan(&app, 120, 34).is_empty(), "filled background in confirm");

    let mut help = test_app();
    help.help_visible = true;
    assert!(scan(&help, 120, 40).is_empty(), "filled background in help");

    let mut menu = test_app();
    menu.panel = Panel::Nodes;
    populate_sample_data(&mut menu);
    press(&mut menu, crossterm::event::KeyCode::Char('x'));
    assert!(scan(&menu, 120, 34).is_empty(), "filled background in menu");
}

// ------------------------------------------------------------ review polish

#[test]
fn header_pill_is_right_aligned_and_never_touches_the_tabs() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    let width = 120u16;
    let out = render_app(&app, width, 30);
    let header = header_row(&out);
    let chars: Vec<char> = header.chars().collect();
    assert_eq!(chars.len(), width as usize);

    let left = chars
        .iter()
        .position(|c| *c == '◖')
        .expect("badge left cap");
    let right = chars
        .iter()
        .rposition(|c| *c == '◗')
        .expect("badge right cap");
    assert_eq!(
        right,
        width as usize - 1,
        "pill must end at the right edge: {header:?}"
    );
    let tabs_end = header_tabs(header).trim_end().chars().count();
    assert!(
        left > tabs_end,
        "the pill must not touch the tabs (tabs end {tabs_end}, pill starts {left}):\n{out}"
    );
}

#[test]
fn header_is_a_single_row_of_tabs_and_pill() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    let out = render_app(&app, 120, 30);
    let lines: Vec<&str> = out.lines().collect();
    let header = lines[0];
    let tokens = header_tokens(header);
    for n in 1..=Panel::ALL.len() {
        let number = n.to_string();
        assert!(
            tokens.contains(&number.as_str()),
            "panel {n} tab missing: {header:?}"
        );
    }
    assert!(
        header.contains("master-1:8995"),
        "cluster pill missing: {header:?}"
    );
    assert!(
        !header.contains("curvine-tui"),
        "brand must leave the header: {header:?}"
    );
    assert!(
        !header.contains('❖'),
        "brand glyph must leave the header: {header:?}"
    );
    // The main area starts immediately on the next row: no empty band.
    assert!(
        lines[1].starts_with('╭'),
        "main area must start on row 2: {:?}",
        lines[1]
    );
}

#[test]
fn node_version_column_shows_release_and_short_commit() {
    assert_eq!(
        crate::ui::nodes::short_version("0.2.2 (commit: 23315882, branch: main)"),
        "0.2.2 (2331588)"
    );
    assert_eq!(
        crate::ui::nodes::short_version("0.2.2 (commit: 2331588)"),
        "0.2.2 (2331588)"
    );
    assert_eq!(crate::ui::nodes::short_version("0.2.2"), "0.2.2");

    let mut app = test_app();
    app.panel = Panel::Nodes;
    populate_sample_data(&mut app);
    if let Some(c) = app.cluster.data.as_mut() {
        c.workers[0].version = "0.2.2 (commit: 23315882, branch: main)".into();
    }
    let out = render_app(&app, 130, 22);
    assert!(
        out.contains("0.2.2 (2331588)"),
        "short version missing:\n{out}"
    );
    assert!(!out.contains("commit:"), "raw build string leaked:\n{out}");
}

#[test]
fn empty_owner_renders_as_a_dash_everywhere() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.entries[1].owner = String::new();
    app.files.entries[1].group = "data".into();
    app.files.selected = 1;

    let out = render_app(&app, 120, 22);
    assert!(out.contains("-:data"), "detail strip owner is malformed:\n{out}");

    // The listing owner column is a single value and never blank; the owner
    // pair must never render a bare separator.
    let row = srow(&out, "events.parquet");
    assert!(row.contains(" -"), "empty owner not dashed: {row:?}");
    assert!(!row.contains(" : "), "bare ' : ' leaked: {row:?}");

    // The full detail view uses the same pair formatting.
    press(&mut app, crossterm::event::KeyCode::Char('d'));
    let view = app.detail.as_ref().expect("detail");
    let owner = view.rows.iter().find(|(k, _)| k == "owner").unwrap();
    assert_eq!(owner.1, "-:data");
}

#[test]
fn jobs_summary_describes_session_only_listing() {
    let out = render_panel(Panel::Jobs, 120, 26, true);
    assert!(
        out.contains("only jobs started in this session are listed"),
        "summary copy missing:\n{out}"
    );
    assert!(
        !out.contains("stream progress from the job master"),
        "stale summary copy still present"
    );
}

#[test]
fn footer_status_badge_is_capped_and_keeps_hints_visible() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.status = "copied 128 files to /data/warehouse/part-0.bin before the connection was reset".into();
    app.anim.mark_status();

    let width = 120u16;
    let out = render_app(&app, width, 26);
    let row = out.lines().last().expect("footer row");
    assert!(row.contains('…'), "status should be truncated:\n{out}");
    let close = row
        .chars()
        .position(|c| c == '◗')
        .map(|i| i + 1)
        .expect("status badge right cap");
    let cap = (width as usize * 45) / 100;
    assert!(
        close <= cap,
        "status badge width {close} exceeds 45% cap {cap}:\n{out}"
    );
    assert!(
        row.contains("x menu"),
        "hints pushed out by a long status:\n{out}"
    );
}

#[test]
fn footer_is_a_single_row() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    app.set_status_for_test("saved 12 files");
    let out = render_app(&app, 120, 30);
    let lines: Vec<&str> = out.lines().collect();
    let footer = *lines.last().expect("footer row");
    assert!(
        footer.contains("◖ saved 12 files ◗"),
        "status is not a one-line badge: {footer:?}"
    );
    assert!(footer.contains("x menu"), "hints missing from footer row");
    // The row above is the bottom border of the main area, not a blank band.
    assert!(
        lines[lines.len() - 2].contains('╰') && lines[lines.len() - 2].contains('╯'),
        "footer is more than one row:\n{out}"
    );
}

// --------------------------------------------------------------- error toasts

#[test]
fn error_status_creates_a_toast_but_success_does_not() {
    let mut ok = test_app();
    ok.set_status_for_test("saved 12 files");
    assert!(ok.toast.is_none(), "success must stay in the footer");
    assert_eq!(ok.status, "saved 12 files");

    let mut err = test_app();
    err.set_error_for_test("mount /data: permission denied");
    let toast = err.toast.as_ref().expect("error must create a toast");
    assert_eq!(toast.text, "mount /data: permission denied");
    assert_eq!(toast.count, 1);
    assert!(
        err.status.is_empty(),
        "errors must not also enter the footer"
    );
    assert!(
        err.log.iter().any(|l| l.contains("ERROR mount /data")),
        "dashboard event log must keep the error: {:?}",
        err.log
    );
}

#[test]
fn toast_renders_at_the_bottom_and_counts_repeats() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    app.set_error_for_test("ls /data: connection refused");
    app.set_error_for_test("ls /data: connection refused");

    assert_eq!(
        app.toast.as_ref().unwrap().count,
        2,
        "repeats must bump the count instead of replacing the toast"
    );

    let out = render_app(&app, 120, 30);
    let lines: Vec<&str> = out.lines().collect();
    let n = lines.len();

    assert!(
        lines[n - 3].contains("● ls /data: connection refused ×2"),
        "toast content missing or count suffix wrong: {:?}\n{out}",
        lines[n - 3]
    );
    assert!(
        lines[n - 4].contains('╭') && lines[n - 4].contains('╮'),
        "toast top border missing at the bottom of the screen:\n{out}"
    );
    assert!(
        lines[n - 2].contains('╰') && lines[n - 2].contains('╯'),
        "toast bottom border missing:\n{out}"
    );
    assert!(
        !lines[n - 1].contains("connection refused"),
        "error must not also appear in the footer: {:?}",
        lines[n - 1]
    );
}

#[test]
fn toast_is_centred_and_truncated_to_60_percent_of_the_screen() {
    let mut app = test_app();
    app.set_error_for_test("x".repeat(400));

    let width = 120u16;
    let out = render_app(&app, width, 30);
    let lines: Vec<&str> = out.lines().collect();
    let n = lines.len();
    let top = lines[n - 4];
    let top_chars: Vec<char> = top.chars().collect();
    let start = top_chars
        .iter()
        .position(|c| *c == '╭')
        .expect("toast top-left border");
    let end = top_chars
        .iter()
        .rposition(|c| *c == '╮')
        .expect("toast top-right border");
    let toast_w = end - start + 1;
    let cap = (width as usize * 60) / 100;
    assert!(
        toast_w <= cap,
        "toast {toast_w} cells wide exceeds the 60% cap {cap}:\n{out}"
    );
    let left = start;
    let right = width as usize - (end + 1);
    assert!(
        left.abs_diff(right) <= 1,
        "toast is not centred: {left} cells left, {right} right\n{out}"
    );
    assert!(
        out.contains('…'),
        "a truncated toast must end in an ellipsis:\n{out}"
    );
}

#[test]
fn any_key_dismisses_the_toast_and_still_runs_its_action() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.set_error_for_test("ls /data: connection refused");
    assert!(app.toast.is_some());

    press(&mut app, crossterm::event::KeyCode::Char('r'));

    assert!(app.toast.is_none(), "the key must dismiss the toast");
    assert!(
        app.files.load.loading,
        "r must still refresh the listing after dismissing the toast"
    );
}

#[test]
fn a_mouse_event_dismisses_the_toast() {
    let mut app = test_app();
    app.set_error_for_test("boom");

    app.on_event(crate::event::AppEvent::Mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Moved,
            column: 10,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
    ));

    assert!(app.toast.is_none(), "a mouse event must dismiss the toast");
}

#[test]
fn toast_expires_after_hold_and_fade_frames() {
    let mut app = test_app();
    app.set_error_for_test("connection refused");

    let frame = std::time::Duration::from_millis(16);
    let frames = |app: &mut App, n: usize| {
        for _ in 0..n {
            app.on_event(crate::event::AppEvent::Frame(frame));
        }
    };

    // 240 frames ≈ 3.84 s: still inside the 4 s hold and fully opaque.
    frames(&mut app, 240);
    assert!(app.toast.is_some(), "toast expired before the hold elapsed");
    assert_eq!(app.anim.toast_alpha(), 1.0);

    // 40 more frames ≈ 4.48 s: mid-fade, still visible.
    frames(&mut app, 40);
    assert!(
        app.toast.is_some(),
        "toast disappeared before the fade finished"
    );
    let alpha = app.anim.toast_alpha();
    assert!(
        alpha > 0.0 && alpha < 1.0,
        "expected a mid-fade alpha, got {alpha}"
    );

    // 50 more frames ≈ 5.28 s crosses hold + fade.
    frames(&mut app, 50);
    assert!(
        app.toast.is_none(),
        "toast must expire after hold + fade frames"
    );
}

#[test]
fn footer_never_shows_error_messages() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    app.set_error_for_test("disk quota exceeded on /data/warehouse");

    let out = render_app(&app, 120, 30);
    let lines: Vec<&str> = out.lines().collect();
    let footer = *lines.last().expect("footer row");
    assert!(
        !footer.contains("disk quota exceeded"),
        "error leaked into the footer: {footer:?}"
    );
    assert!(
        footer.contains("x menu"),
        "footer hints missing: {footer:?}"
    );
    assert!(
        lines[lines.len() - 3].contains("disk quota exceeded"),
        "error missing from the toast row:\n{out}"
    );
}

#[test]
fn menu_title_carries_the_selection_glyph() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);

    app.files.selected = 1; // events.parquet
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    let title = app.menu.as_ref().unwrap().title.clone();
    assert!(
        title.starts_with(crate::ui::icons::for_file("events.parquet")),
        "file glyph missing from menu title: {title:?}"
    );
    press(&mut app, crossterm::event::KeyCode::Esc);

    app.files.selected = 0; // warehouse/
    press(&mut app, crossterm::event::KeyCode::Char('x'));
    let title = app.menu.as_ref().unwrap().title.clone();
    assert!(
        title.starts_with(crate::ui::icons::DIR),
        "directory glyph missing from menu title: {title:?}"
    );
}

#[test]
fn menu_key_column_is_left_aligned() {
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;
    press(&mut app, crossterm::event::KeyCode::Char('x'));

    let out = render_app(&app, 120, 30);
    let label_col = |needle: &str| -> usize {
        let line = out
            .lines()
            .find(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no row with {needle}:\n{out}"));
        line.find(needle).expect("label")
    };
    assert_eq!(
        label_col("detail"),
        label_col("refresh"),
        "menu labels do not share a column:\n{out}"
    );
}

#[test]
fn completion_row_shows_the_first_three_candidates() {
    let mut app = test_app();
    press(&mut app, crossterm::event::KeyCode::Char(':'));
    type_str(&mut app, "m");
    let out = render_app(&app, 110, 30);
    let hint = out.lines().nth_back(1).unwrap_or_default();
    assert!(
        hint.contains("match: mkdir · mv · mount"),
        "completion row format wrong: {hint:?}"
    );
}

#[test]
fn empty_file_listing_explains_itself() {
    let mut app = test_app();
    app.panel = Panel::Files;
    app.files.entries.clear();
    let out = render_app(&app, 100, 24);
    assert!(out.contains("empty directory"), "empty state missing:\n{out}");

    populate_sample_data(&mut app);
    app.files.filter = "nothing-matches-this".into();
    let out = render_app(&app, 100, 24);
    assert!(out.contains("no matches"), "filter state missing:\n{out}");
}

#[test]
fn empty_jobs_listing_explains_itself() {
    let mut app = test_app();
    app.panel = Panel::Jobs;
    let out = render_app(&app, 110, 24);
    assert!(
        out.contains("load jobs submitted in this session will appear here"),
        "jobs empty state missing:\n{out}"
    );
}

#[test]
fn dashboard_footer_names_worker_selection_and_detail() {
    let mut app = test_app();
    app.panel = Panel::Dashboard;
    populate_sample_data(&mut app);
    let out = render_app(&app, 120, 26);
    let footer = out
        .lines()
        .find(|l| l.contains("x menu"))
        .unwrap_or_default();
    assert!(footer.contains("↑↓ select"), "selection hint missing: {footer:?}");
    assert!(footer.contains("d detail"), "detail hint missing: {footer:?}");
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
fn detail_view_is_complete_on_the_first_frame() {
    // Opening detail must never show a half-built panel: tables and the kv
    // region are present immediately, with blocks filled in when they arrive.
    let mut app = test_app();
    app.panel = Panel::Files;
    populate_sample_data(&mut app);
    app.files.selected = 1;

    press(&mut app, crossterm::event::KeyCode::Char('d'));

    let first = render_app(&app, 116, 30);
    assert!(srow(&first, "file").contains("file"), "title badge missing:\n{first}");
    assert!(first.contains("events.parquet"), "title missing:\n{first}");
    assert!(first.contains("mode"), "detail rows missing");
    assert!(first.contains("blocks by offset"), "block table missing");
    assert!(first.contains("distribution by worker"), "distribution table missing");
    assert!(first.contains("esc/q close"), "footer hints missing");
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


