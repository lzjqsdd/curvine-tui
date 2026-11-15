//! Cluster configuration loading.
//!
//! Mirrors `curvine-cli`'s precedence rules:
//!   CLI args  >  config file  >  env vars  >  built-in defaults

use std::path::PathBuf;
use std::sync::Arc;

use curvine_config::ClusterConf;
use curvine_net::net::InetAddr;
use curvine_runtime::runtime::Runtime;

/// Where the effective configuration came from, shown in the UI status bar.
#[derive(Debug, Clone)]
pub struct ConfigSource(pub String);

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub cluster: ClusterConf,
    pub source: ConfigSource,
    #[allow(dead_code)]
    pub conf_path: Option<PathBuf>,
    /// Snapshot of master addresses for display before any RPC happens.
    pub master_addrs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    #[allow(dead_code)]
    pub conf_path: Option<PathBuf>,
    pub master_addrs: Option<Vec<String>>,
}

impl AppConfig {
    /// Load configuration with CLI-arg override on top of the discovered file.
    pub fn load(overrides: ConfigOverrides) -> anyhow::Result<Self> {
        let (mut cluster, source) = match &overrides.conf_path {
            Some(path) => {
                let conf = ClusterConf::from(path.display().to_string())
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                (conf, ConfigSource(format!("file:{}", path.display())))
            }
            None => match std::env::var("CURVINE_CONF_FILE").ok().map(PathBuf::from) {
                Some(path) if path.exists() => {
                    let conf = ClusterConf::from(path.display().to_string())
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                    (conf, ConfigSource(format!("env:{}", path.display())))
                }
                _ => (
                    ClusterConf::default(),
                    ConfigSource("defaults".to_string()),
                ),
            },
        };

        if let Some(addrs) = overrides.master_addrs {
            let parsed = InetAddr::parse_list(addrs.join(","))
                .map_err(|e| anyhow::anyhow!("invalid master address list: {e}"))?;
            cluster.client.master_addrs = parsed;
        }

        let master_addrs = cluster
            .client
            .master_addrs
            .iter()
            .map(|a| a.to_string())
            .collect();

        Ok(Self {
            cluster,
            source,
            conf_path: overrides.conf_path,
            master_addrs,
        })
    }

    /// Build the runtime used by curvine's async clients.
    pub fn build_runtime(&self) -> Arc<Runtime> {
        Arc::new(self.cluster.client_rpc_conf().create_runtime())
    }
}
