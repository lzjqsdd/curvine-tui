//! Replica-health scanning, mirroring `curvine-cli fsck`.
//!
//! The scan is recursive and can be long-running, so it reports progress
//! through a callback and honours a cancellation flag between files.

use std::collections::VecDeque;

use curvine_fs_api::Path;
use curvine_model::{FileStatus, FileType, ListOptions, WorkerStatus};

use super::{AppError, ClusterClient, Result};

/// Aggregated health of a subtree. `Add`-style accumulation keeps the scan
/// loop free of bookkeeping.
#[derive(Debug, Clone, Default)]
pub struct FsckReport {
    pub root: String,
    pub is_dir: bool,
    pub file_count: usize,
    pub block_count: usize,
    pub expected_replicas: usize,
    pub recorded_replicas: usize,
    pub available_replicas: usize,
    pub unavailable_replicas: usize,
    pub under_replicated_blocks: usize,
    pub missing_replica_blocks: usize,
    pub errors: Vec<(String, String)>,
    /// Per-file detail, retained so the UI can drill into an unhealthy file.
    pub files: Vec<FileHealth>,
}

#[derive(Debug, Clone)]
pub struct FileHealth {
    pub path: String,
    pub len: i64,
    pub expected_replicas: usize,
    pub block_count: usize,
    pub available_replicas: usize,
    pub recorded_replicas: usize,
    pub blocks: Vec<BlockHealth>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BlockHealth {
    pub block_id: i64,
    pub len: i64,
    pub offset: i64,
    pub replicas: Vec<ReplicaHealth>,
}

#[derive(Debug, Clone)]
pub struct ReplicaHealth {
    pub worker_id: u32,
    pub address: String,
    pub state: String,
    pub available: bool,
}

impl BlockHealth {
    pub fn available_replicas(&self) -> usize {
        self.replicas.iter().filter(|r| r.available).count()
    }
}

impl FsckReport {
    pub fn has_warnings(&self) -> bool {
        self.under_replicated_blocks > 0 || self.unavailable_replicas > 0 || !self.errors.is_empty()
    }

    /// Fraction of expected replicas that are actually readable, 0..=1.
    ///
    /// Returns 1.0 for an empty scan so an untouched tree shows healthy — but
    /// only when nothing failed. A scan that produced errors and observed no
    /// replicas at all is not "100% healthy"; it is unreadable, and reporting a
    /// perfect ratio would contradict the error list right next to it.
    pub fn health_ratio(&self) -> f64 {
        if self.expected_replicas == 0 {
            return if self.errors.is_empty() { 1.0 } else { 0.0 };
        }
        (self.available_replicas as f64 / self.expected_replicas as f64).clamp(0.0, 1.0)
    }

    fn observe(&mut self, health: FileHealth) {
        let expected = health.expected_replicas;

        if let Some(err) = &health.error {
            self.file_count += 1;
            self.errors.push((health.path.clone(), err.clone()));
            return;
        }

        for block in &health.blocks {
            let available = block.available_replicas();
            self.block_count += 1;
            self.expected_replicas += expected;
            self.recorded_replicas += block.replicas.len();
            self.available_replicas += available;
            self.unavailable_replicas += block.replicas.len().saturating_sub(available);
            if available < expected {
                self.under_replicated_blocks += 1;
            }
            if available == 0 && expected > 0 {
                self.missing_replica_blocks += 1;
            }
        }
        self.file_count += 1;
        self.files.push(health);
    }
}

/// Skip reasons match the CLI so both tools report the same numbers.
fn skip_reason(status: &FileStatus) -> bool {
    status.file_type != FileType::File || status.storage_policy.ufs_only()
}

pub struct ScanLimits {
    pub page_size: usize,
    /// Stop after this many files (0 = unlimited). Guards against a pathological
    /// tree locking up the UI.
    pub max_files: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            page_size: 256,
            max_files: 20_000,
        }
    }
}

impl ClusterClient {
    pub async fn fsck(
        &self,
        root: &str,
        limits: ScanLimits,
        cancel: &std::sync::atomic::AtomicBool,
        mut on_progress: impl FnMut(usize, usize),
    ) -> Result<FsckReport> {
        use std::sync::atomic::Ordering;

        let path = Path::new(root).map_err(|e| AppError::other(e.to_string()))?;
        let status = self.fs().file_status(&path).await.map_err(AppError::from_fs)?;

        let mut report = FsckReport {
            root: status.path.clone(),
            is_dir: status.is_dir,
            ..Default::default()
        };

        if !status.is_dir {
            let health = self.file_health(status).await;
            report.observe(health);
            return Ok(report);
        }

        let page = limits.page_size.max(1);
        let mut directories = VecDeque::from([status.path.clone()]);
        let mut scanned = 0usize;

        while let Some(directory) = directories.pop_front() {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::other("cancelled"));
            }
            let dir_path = Path::new(&directory).map_err(|e| AppError::other(e.to_string()))?;
            let mut start_after: Option<String> = None;

            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(AppError::other("cancelled"));
                }
                let entries = self
                    .fs()
                    .list_options(
                        &dir_path,
                        ListOptions {
                            limit: Some(page),
                            start_after: start_after.clone(),
                        },
                    )
                    .await
                    .map_err(AppError::from_fs)?;

                let count = entries.len();
                start_after = entries.last().map(|e| e.name.clone());

                for entry in entries {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(AppError::other("cancelled"));
                    }
                    if entry.is_dir {
                        directories.push_back(entry.path);
                        continue;
                    }
                    if limits.max_files > 0 && scanned >= limits.max_files {
                        report.errors.push((
                            entry.path.clone(),
                            format!("scan stopped after {} files", limits.max_files),
                        ));
                        return Ok(report);
                    }
                    scanned += 1;
                    let health = self.file_health(entry).await;
                    report.observe(health);
                    on_progress(scanned, report.block_count);
                }

                if count < page {
                    break;
                }
            }
        }

        report.files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(report)
    }

    async fn file_health(&self, status: FileStatus) -> FileHealth {
        let expected = status.replicas.max(0) as usize;

        if skip_reason(&status) {
            return FileHealth {
                path: status.path,
                len: status.len,
                expected_replicas: expected,
                block_count: 0,
                available_replicas: 0,
                recorded_replicas: 0,
                blocks: Vec::new(),
                error: None,
            };
        }

        let path = match Path::new(&status.path) {
            Ok(p) => p,
            Err(e) => {
                return FileHealth {
                    path: status.path,
                    len: status.len,
                    expected_replicas: expected,
                    block_count: 0,
                    available_replicas: 0,
                    recorded_replicas: 0,
                    blocks: Vec::new(),
                    error: Some(e.to_string()),
                }
            }
        };

        match self.fs().get_file_block_details(&path).await {
            Ok(details) => {
                // A non-empty regular file with no block locations means the
                // server could not tell us where the data lives (protocol skew,
                // or metadata damage). Reporting that as "healthy" — which is
                // what a zero-block aggregate looks like — would be a lie.
                if details.blocks.is_empty() && details.status.len > 0 {
                    return FileHealth {
                        path: details.status.path,
                        len: details.status.len,
                        expected_replicas: expected,
                        block_count: 0,
                        available_replicas: 0,
                        recorded_replicas: 0,
                        blocks: Vec::new(),
                        error: Some("no block locations reported for a non-empty file".into()),
                    };
                }

                let mut blocks: Vec<BlockHealth> = details
                    .blocks
                    .into_iter()
                    .map(|b| BlockHealth {
                        block_id: b.block_id,
                        len: b.len,
                        offset: b.offset,
                        replicas: b
                            .replicas
                            .into_iter()
                            .map(|r| ReplicaHealth {
                                worker_id: r.worker_id,
                                // WorkerAddress's Display is the verbose
                                // "worker_id = .., addr = host/ip:port" form;
                                // the endpoint is what the UI needs.
                                address: r
                                    .address
                                    .map(|a| a.connect_addr())
                                    .unwrap_or_else(|| format!("<worker {}>", r.worker_id)),
                                state: format!("{:?}", r.state),
                                available: matches!(r.state, WorkerStatus::Live),
                            })
                            .collect(),
                    })
                    .collect();
                blocks.sort_by_key(|b| b.offset);

                let available = blocks.iter().map(|b| b.available_replicas()).sum();
                let recorded = blocks.iter().map(|b| b.replicas.len()).sum();
                FileHealth {
                    path: details.status.path,
                    len: details.status.len,
                    expected_replicas: expected,
                    block_count: blocks.len(),
                    available_replicas: available,
                    recorded_replicas: recorded,
                    blocks,
                    error: None,
                }
            }
            Err(e) => FileHealth {
                path: status.path,
                len: status.len,
                expected_replicas: expected,
                block_count: 0,
                available_replicas: 0,
                recorded_replicas: 0,
                blocks: Vec::new(),
                error: Some(e.to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(expected: usize, available: usize, errors: usize) -> FsckReport {
        FsckReport {
            expected_replicas: expected,
            available_replicas: available,
            errors: (0..errors).map(|i| (format!("/f{i}"), "boom".to_string())).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn empty_clean_scan_is_healthy() {
        assert_eq!(report(0, 0, 0).health_ratio(), 1.0);
        assert!(!report(0, 0, 0).has_warnings());
    }

    #[test]
    fn empty_scan_with_errors_is_not_healthy() {
        // This is the failure mode that matters: a scan that could read no
        // replica data at all must not render as a green 100%.
        let r = report(0, 0, 3);
        assert_eq!(r.health_ratio(), 0.0);
        assert!(r.has_warnings());
    }

    #[test]
    fn partial_availability_scales() {
        assert_eq!(report(10, 5, 0).health_ratio(), 0.5);
        assert_eq!(report(10, 10, 0).health_ratio(), 1.0);
        assert_eq!(report(10, 0, 0).health_ratio(), 0.0);
    }

    #[test]
    fn observed_errors_are_recorded() {
        let mut r = FsckReport::default();
        r.observe(FileHealth {
            path: "/x".into(),
            len: 10,
            expected_replicas: 2,
            block_count: 0,
            available_replicas: 0,
            recorded_replicas: 0,
            blocks: Vec::new(),
            error: Some("no block locations".into()),
        });
        assert_eq!(r.file_count, 1);
        assert_eq!(r.errors.len(), 1);
        assert!(r.has_warnings());
    }
}
