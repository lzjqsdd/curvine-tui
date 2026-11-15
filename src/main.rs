//! curvine-tui — a terminal UI for the Curvine distributed file system.

#![recursion_limit = "256"]

mod app;
mod config;
mod event;
mod rpc;
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
