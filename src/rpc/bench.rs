//! Benchmark execution, mirroring `curvine-cli bench`.
//!
//! Runs `CurvineBenchRunner` on the UI's runtime and returns a flattened,
//! table-friendly view of the report.

use curvine_bench::{BenchConfig, BenchProfile, CurvineBenchRunner, BenchMode, BenchTarget};

use super::{AppError, ClusterClient, Result};

/// One row of the results table.
#[derive(Debug, Clone)]
pub struct BenchRow {
    pub group: String,
    pub op: String,
    pub item: String,
    pub value: f64,
    pub value_unit: String,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub errors: u64,
}

#[derive(Debug, Clone)]
pub struct BenchSummary {
    pub rows: Vec<BenchRow>,
    pub temp_path: String,
    pub elapsed_ms: f64,
}

/// A deliberately small subset of the CLI's knobs: enough for interactive
/// tuning without mirroring every flag.
#[derive(Debug, Clone)]
pub struct BenchOptions {
    pub base_path: String,
    pub profile: BenchProfile,
    pub block_size: usize,
    pub big_file_size: usize,
    pub small_file_count: usize,
    pub duration_ms: Option<u64>,
    pub threads: usize,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            base_path: "cv://default/__curvine_tui_bench__".to_string(),
            profile: BenchProfile::Basic,
            block_size: 256 * 1024,
            big_file_size: 64 * 1024 * 1024,
            small_file_count: 100,
            duration_ms: Some(10_000),
            threads: 1,
        }
    }
}

impl BenchOptions {
    fn to_config(&self) -> BenchConfig {
        BenchConfig {
            base_path: self.base_path.clone(),
            mode: BenchMode::Client,
            profile: self.profile,
            target: BenchTarget::Curvine,
            threads: self.threads.max(1),
            block_size: self.block_size.max(1024),
            big_file_size: self.big_file_size,
            big_file_count: 1,
            small_file_size: 128 * 1024,
            small_file_count: self.small_file_count,
            keep_data: false,
            duration_ms: self.duration_ms,
            total_size: None,
            total_ops: None,
            working_set_files: None,
            files_per_dir: 100,
            workload: None,
        }
    }
}

impl ClusterClient {
    pub async fn run_bench(&self, opts: &BenchOptions) -> Result<BenchSummary> {
        let runner = CurvineBenchRunner::new(self.inner().clone(), opts.to_config());
        let report = runner.run().await.map_err(AppError::from_fs)?;

        let rows = report
            .results
            .iter()
            .map(|r| BenchRow {
                group: format!("{:?}", r.group),
                op: format!("{:?}", r.op),
                item: r.item.clone(),
                value: r.value,
                value_unit: r.value_unit.clone(),
                p50_ms: r.latency_p50_ms,
                p95_ms: r.latency_p95_ms,
                p99_ms: r.latency_p99_ms,
                errors: r.errors,
            })
            .collect();

        let elapsed_ms = report
            .results
            .iter()
            .map(|r| r.elapsed_ms)
            .fold(0.0_f64, f64::max);

        Ok(BenchSummary {
            rows,
            temp_path: report.temp_path,
            elapsed_ms,
        })
    }
}
