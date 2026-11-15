//! Data access layer.
//!
//! Wraps `UnifiedFileSystem` + `JobMasterClient` behind a small, UI-friendly
//! facade. Everything is async and returns `Result<T, AppError>` so the UI can
//! render failures instead of crashing.

use std::sync::Arc;

use curvine_fs_api::{FileSystem, Path};
use curvine_job_client::JobMasterClient;
use curvine_model::{
    FileStatus, FilesystemInfo, JobStatus, MountInfo, MountOptions, RuntimeConfigEntry,
};
use curvine_runtime::runtime::Runtime;
use curvine_unified_fs::UnifiedFileSystem;

use crate::config::AppConfig;

pub mod error;
pub mod bench;
pub mod fsck;
pub use error::AppError;

pub type Result<T> = std::result::Result<T, AppError>;

fn p(path: &str) -> Result<Path> {
    Path::new(path).map_err(|e| AppError::Other(e.to_string()))
}

/// Thin, cloneable handle to the cluster. Cheap to pass into background tasks.
#[derive(Clone)]
pub struct ClusterClient {
    fs: Arc<UnifiedFileSystem>,
    job: Arc<JobMasterClient>,
    rt: Arc<Runtime>,
}

impl ClusterClient {
    pub fn connect(cfg: &AppConfig) -> Result<Self> {
        let rt = cfg.build_runtime();
        let fs = UnifiedFileSystem::with_rt(cfg.cluster.clone(), rt.clone())
            .map_err(AppError::from_fs)?;
        let fs = Arc::new(fs);
        let job = Arc::new(JobMasterClient::new(fs.fs_client()));
        Ok(Self { fs, job, rt })
    }

    pub fn runtime(&self) -> Arc<Runtime> {
        self.rt.clone()
    }

    /// Escape hatch for callers that need the raw filesystem (option builders,
    /// config). Kept narrow on purpose.
    pub fn inner(&self) -> &UnifiedFileSystem {
        &self.fs
    }

    /// The lower-level client, needed for block-location and paged-listing
    /// RPCs that `UnifiedFileSystem` does not expose directly.
    pub(crate) fn fs(&self) -> std::sync::Arc<curvine_client_core::file::FsClient> {
        self.fs.fs_client()
    }

    pub async fn open_writer(
        &self,
        path: &Path,
        opts: curvine_model::CreateFileOpts,
        flags: curvine_model::OpenFlags,
    ) -> std::result::Result<curvine_unified_fs::UnifiedWriter, curvine_error::FsError> {
        self.fs.open_with_opts(path, opts, flags).await
    }

    pub async fn open_reader(
        &self,
        path: &Path,
    ) -> std::result::Result<curvine_unified_fs::UnifiedReader, curvine_error::FsError> {
        self.fs.open(path).await
    }

    // ---------------------------------------------------------------- fs ops

    pub async fn ls(&self, path: &str) -> Result<Vec<FileStatus>> {
        let path = p(path)?;
        self.fs
            .list_status(&path)
            .await
            .map_err(AppError::from_fs)
    }

    #[allow(dead_code)]
    pub async fn stat(&self, path: &str) -> Result<FileStatus> {
        let path = p(path)?;
        self.fs.get_status(&path).await.map_err(AppError::from_fs)
    }

    pub async fn mkdir(&self, path: &str, parents: bool) -> Result<bool> {
        let path = p(path)?;
        self.fs
            .mkdir(&path, parents)
            .await
            .map_err(AppError::from_fs)
    }

    pub async fn delete(&self, path: &str, recursive: bool) -> Result<()> {
        let path = p(path)?;
        self.fs
            .delete(&path, recursive)
            .await
            .map(|_| ())
            .map_err(AppError::from_fs)
    }

    #[allow(dead_code)]
    pub async fn rename(&self, src: &str, dst: &str) -> Result<()> {
        let src = p(src)?;
        let dst = p(dst)?;
        self.fs
            .rename(&src, &dst)
            .await
            .map(|_| ())
            .map_err(AppError::from_fs)
    }

    #[allow(dead_code)]
    pub async fn free(&self, path: &str, recursive: bool) -> Result<()> {
        let path = p(path)?;
        self.fs
            .free(&path, recursive)
            .await
            .map(|_| ())
            .map_err(AppError::from_fs)
    }

    // ------------------------------------------------------------ cluster ops

    pub async fn filesystem_info(&self) -> Result<FilesystemInfo> {
        self.fs
            .get_filesystem_info()
            .await
            .map_err(AppError::from_fs)
    }

    pub async fn mounts(&self) -> Result<Vec<MountInfo>> {
        self.fs.get_mount_table().await.map_err(AppError::from_fs)
    }

    pub async fn mount(&self, ufs: &str, cv: &str, opts: MountOptions) -> Result<()> {
        let ufs = p(ufs)?;
        let cv = p(cv)?;
        self.fs
            .mount(&ufs, &cv, opts)
            .await
            .map_err(AppError::from_fs)
    }

    /// Block-level detail for a single file, reusing the fsck types so the
    /// describe popup and the health scan cannot disagree.
    pub async fn file_blocks(&self, path: &str) -> Result<Vec<fsck::BlockHealth>> {
        let p = p(path)?;
        let details = self
            .fs()
            .get_file_block_details(&p)
            .await
            .map_err(AppError::from_fs)?;

        let mut blocks: Vec<fsck::BlockHealth> = details
            .blocks
            .into_iter()
            .map(|b| fsck::BlockHealth {
                block_id: b.block_id,
                len: b.len,
                offset: b.offset,
                replicas: b
                    .replicas
                    .into_iter()
                    .map(|r| fsck::ReplicaHealth {
                        worker_id: r.worker_id,
                        address: r
                            .address
                            .map(|a| a.connect_addr())
                            .unwrap_or_else(|| format!("<worker {}>", r.worker_id)),
                        state: format!("{:?}", r.state),
                        available: matches!(r.state, curvine_model::WorkerStatus::Live),
                    })
                    .collect(),
            })
            .collect();
        blocks.sort_by_key(|b| b.offset);
        Ok(blocks)
    }

    pub async fn umount(&self, cv: &str) -> Result<()> {
        let cv = p(cv)?;
        self.fs.umount(&cv).await.map_err(AppError::from_fs)
    }

    pub async fn runtime_config(&self, key: Option<&str>) -> Result<Vec<RuntimeConfigEntry>> {
        self.fs
            .get_runtime_config(key)
            .await
            .map_err(AppError::from_fs)
    }

    // ---------------------------------------------------------------- job ops

    pub async fn jobs(&self, limit: usize) -> Result<Vec<JobStatus>> {
        self.job
            .list_job_statuses(None, limit, true)
            .await
            .map_err(AppError::from_fs)
    }

    #[allow(dead_code)]
    pub async fn job_status(&self, id: &str) -> Result<JobStatus> {
        self.job
            .get_job_status(id)
            .await
            .map_err(AppError::from_fs)
    }

    #[allow(dead_code)]
    pub async fn cancel_job(&self, id: &str) -> Result<()> {
        self.job.cancel_job(id).await.map_err(AppError::from_fs)
    }

    pub async fn submit_load(&self, path: &str) -> Result<()> {
        self.job
            .submit_load(path)
            .await
            .map(|_| ())
            .map_err(AppError::from_fs)
    }
}
