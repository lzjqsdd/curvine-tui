//! curvine-tui — a terminal UI for the Curvine distributed file system.

#![recursion_limit = "256"]

mod app;
mod config;
mod event;
pub mod rpc;
#[cfg(test)]
mod tests;
mod transfer;
mod ui;

use std::time::Duration;

use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use app::App;
use curvine_runtime::runtime::RpcRuntime;
use config::{AppConfig, ConfigOverrides};
use rpc::ClusterClient;

#[derive(Parser, Debug)]
#[command(name = "curvine-tui", version, about = "Terminal UI for Curvine")]
struct Args {
    /// Configuration file path (optional)
    #[arg(long, global = true)]
    conf: Option<std::path::PathBuf>,

    /// Master address list (e.g. 'm1:8995,m2:8995')
    #[arg(long = "master-addrs", global = true)]
    master_addrs: Option<String>,

    /// Auto-refresh interval in seconds
    #[arg(long, default_value_t = 5)]
    refresh: u64,

    /// Connect, dump cluster state to stdout, and exit. Useful for checking
    /// connectivity without a terminal.
    #[arg(long)]
    probe: bool,

    /// Upload a local file to a remote path and exit (headless put).
    #[arg(long, num_args = 2, value_names = ["LOCAL", "REMOTE"])]
    put: Option<Vec<String>>,

    /// Download a remote path to a local file and exit (headless get).
    #[arg(long, num_args = 2, value_names = ["REMOTE", "LOCAL"])]
    get: Option<Vec<String>>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let overrides = ConfigOverrides {
        conf_path: args.conf.clone(),
        master_addrs: args
            .master_addrs
            .as_ref()
            .map(|s| s.split(',').map(|v| v.trim().to_string()).collect()),
    };
    let cfg = AppConfig::load(overrides)?;
    let client = ClusterClient::connect(&cfg)?;

    let rt = client.runtime();

    if args.probe {
        let code = rt.block_on(probe(&cfg, &client));
        drop(rt);
        std::process::exit(code);
    }

    if let Some(pair) = &args.put {
        let code = rt.block_on(headless_transfer(
            &client,
            event::TransferKind::Upload,
            &pair[0],
            &pair[1],
        ));
        drop(rt);
        std::process::exit(code);
    }

    if let Some(pair) = &args.get {
        let code = rt.block_on(headless_transfer(
            &client,
            event::TransferKind::Download,
            &pair[1],
            &pair[0],
        ));
        drop(rt);
        std::process::exit(code);
    }

    let (tx, mut rx) = mpsc::unbounded_channel();

    event::spawn_input_thread(tx.clone());
    event::spawn_tick_thread(tx.clone(), Duration::from_secs(args.refresh.max(1)));

    let mut app = App::new(cfg, client, tx.clone());

    // Terminal setup / teardown guard.
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run(&mut terminal, &mut app, &mut rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Give in-flight tasks a moment to observe cancellation.
    drop(rt);

    result
}

/// One-shot transfer with progress printed to stderr. Returns an exit code.
///
/// Shares the exact code path the TUI uses, so this doubles as an integration
/// harness for the streaming engine.
async fn headless_transfer(
    client: &ClusterClient,
    kind: event::TransferKind,
    local: &str,
    remote: &str,
) -> i32 {
    let cancel = transfer::new_cancel_flag();
    let last_pct = std::sync::atomic::AtomicU64::new(u64::MAX);
    let local_path = std::path::PathBuf::from(local);

    let report = |transferred: u64, total: u64, _speed: u64| {
        let pct = (transferred * 100).checked_div(total).unwrap_or(0);
        // Only print on whole-percent changes to keep the output readable.
        if last_pct.swap(pct, std::sync::atomic::Ordering::Relaxed) != pct {
            eprint!("\r  {pct:>3}%  {}", ui::human_bytes(transferred as i64));
        }
    };

    let result = match kind {
        event::TransferKind::Upload => {
            transfer::upload_file(client, &local_path, remote, cancel, report).await
        }
        event::TransferKind::Download => {
            transfer::download_file(client, remote, &local_path, cancel, report).await
        }
    };

    eprintln!();
    match result {
        Ok(n) => {
            println!("ok: {n} bytes");
            0
        }
        Err(e) => {
            println!("failed: {e}");
            1
        }
    }
}

/// One-shot connectivity dump. Returns a process exit code.
async fn probe(cfg: &AppConfig, client: &ClusterClient) -> i32 {
    println!("config   : {}", cfg.source);
    println!(
        "masters  : {}",
        if cfg.master_addrs.is_empty() {
            "(none configured)".to_string()
        } else {
            cfg.master_addrs.join(", ")
        }
    );

    let mut failed = 0;

    match client.filesystem_info().await {
        Ok(info) => {
            println!("cluster  : OK");
            println!("  active master : {}", info.active_master);
            println!(
                "  capacity      : {} available, {} used",
                ui::human_bytes(info.available),
                ui::human_bytes(info.fs_used)
            );
            println!(
                "  inodes        : {} dirs, {} files, {} blocks",
                info.inode_dir_num, info.inode_file_num, info.block_num
            );
            println!(
                "  workers       : {} live, {} lost",
                info.live_workers.len(),
                info.lost_workers.len()
            );
            for w in info.live_workers.iter().take(5) {
                println!(
                    "    endpoint    : {}:{} (host {}, id {}, {:?})",
                    w.address.ip_addr, w.address.rpc_port, w.address.hostname, w.address.worker_id, w.status
                );
            }
        }
        Err(e) => {
            println!("cluster  : FAILED {e}");
            failed += 1;
        }
    }

    let ls_path = std::env::var("CURVINE_TUI_PROBE_LS").unwrap_or_else(|_| "/".to_string());
    match client.ls(&ls_path).await {
        Ok(list) => {
            println!("ls {ls_path} : OK ({} entries)", list.len());
            for f in list.iter().take(10) {
                println!(
                    "  {} {:>10}  repl={:<3} {}",
                    if f.is_dir { "d" } else { "-" },
                    if f.is_dir {
                        "-".to_string()
                    } else {
                        ui::human_bytes(f.len)
                    },
                    f.replicas,
                    f.name
                );
            }
        }
        Err(e) => {
            println!("ls {ls_path} : FAILED {e}");
            failed += 1;
        }
    }

    match client.mounts().await {
        Ok(m) => println!("mounts   : OK ({} entries)", m.len()),
        Err(e) => {
            println!("mounts   : FAILED {e}");
            failed += 1;
        }
    }

    match client.jobs(10).await {
        Ok(j) => println!("jobs     : OK ({} entries)", j.len()),
        Err(e) => {
            println!("jobs     : FAILED {e}");
            failed += 1;
        }
    }

    {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let started = std::time::Instant::now();
        match client
            .fsck("/", rpc::fsck::ScanLimits::default(), &cancel, |_, _| {})
            .await
        {
            Ok(r) => {
                println!(
                    "fsck     : OK in {:?} — {} files, {} blocks",
                    started.elapsed(),
                    r.file_count,
                    r.block_count
                );
                println!(
                    "  replicas      : {}/{} available ({} recorded, {} under-replicated)",
                    r.available_replicas,
                    r.expected_replicas,
                    r.recorded_replicas,
                    r.under_replicated_blocks
                );
                println!("  health        : {:.1}%", r.health_ratio() * 100.0);
                if r.has_warnings() {
                    println!("  WARN          : {} errors", r.errors.len());
                }
            }
            Err(e) => {
                println!("fsck     : FAILED {e}");
                failed += 1;
            }
        }
    }

    if std::env::var("CURVINE_TUI_PROBE_BENCH").as_deref() == Ok("1") {
        let opts = rpc::bench::BenchOptions {
            duration_ms: Some(3000),
            small_file_count: 20,
            ..Default::default()
        };
        let started = std::time::Instant::now();
        match client.run_bench(&opts).await {
            Ok(s) => {
                println!("bench    : OK in {:?} ({} rows)", started.elapsed(), s.rows.len());
                for r in s.rows.iter().take(8) {
                    println!(
                        "  {:<10} {:<24} {:>10.2} {:<6} p50={:.3}ms p99={:.3}ms",
                        r.op, r.item, r.value, r.value_unit, r.p50_ms, r.p99_ms
                    );
                }
            }
            Err(e) => {
                println!("bench    : FAILED {e}");
                failed += 1;
            }
        }
    }

    if let Ok(path) = std::env::var("CURVINE_TUI_PROBE_DESCRIBE") {
        match client.file_blocks(&path).await {
            Ok(blocks) => {
                println!("describe : OK {} ({} blocks)", path, blocks.len());
                for b in &blocks {
                    let replicas: Vec<String> = b
                        .replicas
                        .iter()
                        .map(|r| format!("{}{}", r.address, if r.available { "✓" } else { "✗" }))
                        .collect();
                    println!(
                        "  block {:>18}  off {:>10}  len {:>10}  {}",
                        b.block_id,
                        b.offset,
                        b.len,
                        replicas.join("  ")
                    );
                }
            }
            Err(e) => {
                println!("describe : FAILED {e}");
                failed += 1;
            }
        }
    }

    if let Ok(spec) = std::env::var("CURVINE_TUI_PROBE_MOUNT") {
        // format: ufs_path|cv_path
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 {
            let opts = curvine_model::MountOptions::builder()
                .write_type(curvine_model::WriteType::CacheMode)
                .ttl_action(curvine_model::TtlAction::Delete)
                .build();
            match client.mount(parts[0], parts[1], opts).await {
                Ok(_) => println!("mount    : OK {} -> {}", parts[0], parts[1]),
                Err(e) => {
                    println!("mount    : FAILED {e}");
                    failed += 1;
                }
            }
        }
    }

    if let Ok(path) = std::env::var("CURVINE_TUI_PROBE_UMOUNT") {
        match client.umount(&path).await {
            Ok(_) => println!("umount   : OK {path}"),
            Err(e) => {
                println!("umount   : FAILED {e}");
                failed += 1;
            }
        }
    }

    if std::env::var("CURVINE_TUI_PROBE_FSCK_PATH").ok().is_some() {
        let target = std::env::var("CURVINE_TUI_PROBE_FSCK_PATH").unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        match client
            .fsck(&target, rpc::fsck::ScanLimits::default(), &cancel, |_, _| {})
            .await
        {
            Ok(r) => {
                println!("fsck {target} : {} files, {} blocks", r.file_count, r.block_count);
                println!(
                    "  replicas {}/{} available ({} under-replicated, {} empty)",
                    r.available_replicas,
                    r.expected_replicas,
                    r.under_replicated_blocks,
                    r.missing_replica_blocks
                );
                println!("  warnings: {}", r.has_warnings());
                for e in r.errors.iter().take(5) {
                    println!("  error {}: {}", e.0, e.1);
                }
                for f in r.files.iter().take(20) {
                    let un = f.blocks.iter().filter(|b| b.available_replicas() < f.expected_replicas).count();
                    if un > 0 || f.error.is_some() {
                        println!(
                            "  bad {} — {} blocks, {}/{} replicas, err={:?}",
                            f.path, f.block_count, f.available_replicas, f.expected_replicas, f.error
                        );
                    }
                }
            }
            Err(e) => println!("fsck {target} : FAILED {e}"),
        }
    }

    match client.runtime_config(None).await {
        Ok(c) => println!("config   : OK ({} keys)", c.len()),
        Err(e) => {
            println!("config   : FAILED {e}");
            failed += 1;
        }
    }

    if failed == 0 {
        println!("\nresult   : all checks passed");
        0
    } else {
        println!("\nresult   : {failed} check(s) failed");
        1
    }
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    rx: &mut mpsc::UnboundedReceiver<event::AppEvent>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;

        // Block for the next event, but redraw on resize/keystroke promptly.
        match rx.blocking_recv() {
            Some(evt) => app.on_event(evt),
            None => break,
        }

        // Drain anything already queued to avoid redraw starvation.
        while let Ok(evt) = rx.try_recv() {
            app.on_event(evt);
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
