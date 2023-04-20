use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use url::Url;

use crate::config;

#[derive(Debug, Clone)]
pub struct DataDir {
    path: PathBuf,
}

impl DataDir {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
        }
    }

    pub fn lock(&self) -> anyhow::Result<Option<fd_lock::RwLock<fs::File>>> {
        if self.config_exist()? {
            Ok(Some(fd_lock::RwLock::new(fs::File::open(
                self.config_file_path(),
            )?)))
        } else {
            Ok(None)
        }
    }

    /// Load currently configured `remote` from config if not overridden
    pub fn get_current_remote_with_opt_override(
        &self,
        remote: Option<&Url>,
    ) -> anyhow::Result<Url> {
        remote
            .cloned()
            .ok_or(())
            .or_else(|_| -> anyhow::Result<Url> { Ok(self.load_config()?.remote()?.clone()) })
    }

    /// Load currently configured `configuration` if not overridden
    pub fn get_current_configuration_with_opt_override(
        &self,
        configuration: Option<&str>,
    ) -> anyhow::Result<String> {
        configuration
            .map(ToOwned::to_owned)
            .ok_or(())
            .or_else(|_| -> anyhow::Result<String> {
                Ok(self.load_config()?.configuration()?.to_owned())
            })
    }

    fn config_file_path(&self) -> PathBuf {
        self.path.join("config.json")
    }

    pub fn config_exist(&self) -> anyhow::Result<bool> {
        Ok(self.config_file_path().try_exists()?)
    }

    pub fn load_config(&self) -> anyhow::Result<config::Config> {
        let config_path = self.config_file_path();
        if config_path.exists() {
            config::Config::load(&self.config_file_path()).context("Failed to load config")
        } else {
            Ok(Default::default())
        }
    }

    pub fn store_config(&self, config: &config::Config) -> anyhow::Result<()> {
        fs::create_dir_all(&self.path)
            .with_context(|| format!("Failed to create data directory: {}", self.path.display()))?;
        config
            .store(&self.config_file_path())
            .context("Failed to store config")
    }

    pub fn update_last_reconfiguration(
        &self,
        configuration: &str,
        etag: &str,
    ) -> anyhow::Result<()> {
        self.store_config(
            &self
                .load_config()?
                .with_updated_last_reconfiguration(configuration, etag),
        )
    }
}
