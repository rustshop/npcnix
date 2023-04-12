use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use url::Url;

use crate::config;

#[derive(Parser, Debug, Clone)]
pub struct Common {
    #[arg(long, env = "NPCNIX_DATA_DIR", default_value = "/var/lib/npcnix")]
    data_dir: PathBuf,
}

impl Common {
    /// Load currently configured `remote` from config if not overriden
    pub fn get_current_remote_with_opt_override(
        &self,
        remote: Option<&Url>,
    ) -> anyhow::Result<Url> {
        remote
            .cloned()
            .ok_or(())
            .or_else(|_| -> anyhow::Result<Url> { Ok(self.load_config()?.remote()?.clone()) })
    }

    /// Load currently configured `configuration` if not overriden
    pub fn get_current_configuration_with_opt_override(
        &self,
        configuration: Option<&str>,
    ) -> anyhow::Result<String> {
        configuration
            .map(ToOwned::to_owned)
            .ok_or(())
            .or_else(|_| -> anyhow::Result<String> {
                Ok(self.load_config()?.configuration().to_owned())
            })
    }

    fn config_file_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
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
        fs::create_dir_all(&self.data_dir).with_context(|| {
            format!(
                "Failed to create data directory: {}",
                self.data_dir.display()
            )
        })?;
        config
            .store(&self.config_file_path())
            .context("Failed to store config")
    }
}
