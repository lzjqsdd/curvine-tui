//! Streaming file transfer between the local filesystem and Curvine.
//!
//! Transfers run on curvine's runtime and report progress through an
//! `mpsc` sender. Progress is throttled inside the loop so a 10 GiB upload
//! cannot flood the UI channel with millions of events.

use std::path::{Path as StdPath, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use curvine_fs_api::{Path, Reader, Writer};
use curvine_runtime::runtime::RpcRuntime;
use curvine_model::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::event::{AppEvent, TaskResult};
use crate::rpc::{AppError, ClusterClient, Result};

/// How often progress is pushed to the UI.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);

/// Shared cancellation flag; the UI holds one handle, the worker the other.
pub type CancelFlag = Arc<AtomicBool>;

pub fn new_cancel_flag() -> CancelFlag {
    Arc::new(AtomicBool::new(false))
}

/// Snapshot of a transfer's progress, used for speed calculation in the UI.
#[derive(Debug, Clone, Copy)]
struct ProgressReporter {
    total: u64,
    transferred: u64,
    last_emit: Instant,
    last_bytes: u64,
}

impl ProgressReporter {
    fn new(total: u64) -> Self {
        Self {
            total,
            transferred: 0,
            last_emit: Instant::now(),
            last_bytes: 0,
        }
    }

    /// Update counters and emit an event only if the throttle window elapsed.
    fn advance(&mut self, delta: u64, force: bool, emit: &mut dyn FnMut(u64, u64, u64)) {
        self.transferred += delta;
        let elapsed = self.last_emit.elapsed();
        if force || elapsed >= PROGRESS_INTERVAL {
            // Report the *actual* rate over the window, not a cumulative average,
            // so stalls are visible.
            let speed = if elapsed.as_secs_f64() > 0.0 {
                (self.transferred - self.last_bytes) as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            emit(self.transferred, self.total, speed as u64);
            self.last_emit = Instant::now();
            self.last_bytes = self.transferred;
        }
    }
}

// ------------------------------------------------------------------ upload

/// Upload a single local file to `remote`.
pub async fn upload_file(
    client: &ClusterClient,
    local: &StdPath,
    remote: &str,
    cancel: CancelFlag,
    on_progress: impl FnMut(u64, u64, u64) + Send,
) -> Result<u64> {
    let total = std::fs::metadata(local)
        .map_err(|e| AppError::other(format!("{}: {e}", local.display())))?
        .len();

    let mut file = tokio::fs::File::open(local)
        .await
        .map_err(|e| AppError::other(format!("{}: {e}", local.display())))?;

    let path = Path::new(remote).map_err(|e| AppError::other(e.to_string()))?;
    let cv = client.inner();
    let (owner, group, mode) = process_acl(cv);
    let opts = cv
        .cv()
        .create_opts_builder()
        .create_parent(true)
        .owner(owner)
        .group(group)
        .mode(mode)
        .build();
    let flags = OpenFlags::new_write_only()
        .set_create(true)
        .set_overwrite(true);

    let mut writer = client
        .open_writer(&path, opts, flags)
        .await
        .map_err(AppError::from_fs)?;

    let chunk_size = cv.conf().client.write_chunk_size.max(64 * 1024);
    let mut buf = vec![0u8; chunk_size];
    let mut reporter = ProgressReporter::new(total);
    let mut emit = on_progress;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::other("cancelled"));
        }
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| AppError::other(format!("read {}: {e}", local.display())))?;
        if n == 0 {
            break;
        }
        writer
            .write(&buf[..n])
            .await
            .map_err(AppError::from_fs)?;
        reporter.advance(n as u64, false, &mut emit);
    }

    writer.complete().await.map_err(AppError::from_fs)?;
    reporter.advance(0, true, &mut emit);
    Ok(reporter.transferred)
}

// ---------------------------------------------------------------- download

/// Download `remote` to the local path `local`.
pub async fn download_file(
    client: &ClusterClient,
    remote: &str,
    local: &StdPath,
    cancel: CancelFlag,
    on_progress: impl FnMut(u64, u64, u64) + Send,
) -> Result<u64> {
    let path = Path::new(remote).map_err(|e| AppError::other(e.to_string()))?;
    let mut reader = client
        .open_reader(&path)
        .await
        .map_err(AppError::from_fs)?;

    let total = reader.len().max(0) as u64;
    let chunk_size = client.inner().conf().client.read_chunk_size.max(64 * 1024);

    if let Some(parent) = local.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
    }
    let mut out = tokio::fs::File::create(local)
        .await
        .map_err(|e| AppError::other(format!("{}: {e}", local.display())))?;

    let mut buf = vec![0u8; chunk_size];
    let mut reporter = ProgressReporter::new(total);
    let mut emit = on_progress;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::other("cancelled"));
        }
        let n = reader.read(&mut buf).await.map_err(AppError::from_fs)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .await
            .map_err(|e| AppError::other(format!("write {}: {e}", local.display())))?;
        reporter.advance(n as u64, false, &mut emit);
    }

    out.flush().await.map_err(AppError::from_fs)?;
    reader.complete().await.map_err(AppError::from_fs)?;
    reporter.advance(0, true, &mut emit);
    Ok(reporter.transferred)
}

// --------------------------------------------------------------- dispatch

/// Spawn a transfer on the client runtime, reporting into `tx`.
///
/// `local` is a `PathBuf` for uploads and a destination for downloads; both
/// directions take the same shape so the caller can stay direction-agnostic.
pub fn spawn_transfer(
    client: ClusterClient,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
    id: u64,
    kind: crate::event::TransferKind,
    local: PathBuf,
    remote: String,
    cancel: CancelFlag,
) {
    let rt = client.runtime();
    rt.spawn(async move {
        let progress_tx = tx.clone();
        let emit = move |transferred: u64, total: u64, speed: u64| {
            let _ = progress_tx.send(AppEvent::TaskDone(TaskResult::TransferProgress {
                id,
                transferred,
                total,
                speed,
            }));
        };

        let result = match kind {
            crate::event::TransferKind::Upload => {
                upload_file(&client, &local, &remote, cancel.clone(), emit).await
            }
            crate::event::TransferKind::Download => {
                download_file(&client, &remote, &local, cancel.clone(), emit).await
            }
        };

        let _ = tx.send(AppEvent::TaskDone(TaskResult::TransferDone {
            id,
            result: result.map_err(|e| e.to_string()),
        }));
    });
}

// ------------------------------------------------------------------ helpers

/// Owner / group / mode for files created by this process, matching the CLI.
fn process_acl(cv: &curvine_unified_fs::UnifiedFileSystem) -> (String, String, u32) {
    let uid = curvine_sys::get_uid();
    let gid = curvine_sys::get_gid();
    (
        curvine_sys::get_username_by_uid(uid).unwrap_or_else(|| uid.to_string()),
        curvine_sys::get_groupname_by_gid(gid).unwrap_or_else(|| gid.to_string()),
        cv.conf().client.get_mode(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the reporter through `(delta, force)` steps and collect emissions.
    fn run(reporter: &mut ProgressReporter, steps: &[(u64, bool)]) -> Vec<(u64, u64, u64)> {
        let mut seen = Vec::new();
        let mut emit = |t: u64, total: u64, s: u64| seen.push((t, total, s));
        for (delta, force) in steps {
            reporter.advance(*delta, *force, &mut emit);
        }
        seen
    }

    #[test]
    fn progress_is_throttled_between_emits() {
        let mut r = ProgressReporter::new(1000);
        // The first call happens well within the throttle window, so nothing
        // should be emitted even though bytes were transferred.
        let out = run(&mut r, &[(10, false), (10, false), (10, false)]);
        assert!(out.is_empty(), "throttle leaked {} events", out.len());
        assert_eq!(r.transferred, 30, "bytes must still be counted");
    }

    #[test]
    fn forced_advance_always_emits() {
        let mut r = ProgressReporter::new(1000);
        let out = run(&mut r, &[(500, true)]);
        assert_eq!(out.len(), 1);
        let (transferred, total, _speed) = out[0];
        assert_eq!(transferred, 500);
        assert_eq!(total, 1000);
    }

    #[test]
    fn total_is_reported_so_downloads_can_show_a_ratio() {
        let mut r = ProgressReporter::new(42);
        let out = run(&mut r, &[(1, true)]);
        assert_eq!(out[0].1, 42);
    }

    #[test]
    fn transferred_accumulates_across_forced_emits() {
        let mut r = ProgressReporter::new(100);
        let out = run(&mut r, &[(25, true), (25, true)]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, 25);
        assert_eq!(out[1].0, 50);
    }
}
