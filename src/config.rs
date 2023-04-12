use std::fmt;
use std::path::Path;

use anyhow::format_err;
use serde::{Deserialize, Serialize};
use url::Url;

fn default_configuration() -> String {
    "nixos".into()
}

/// Persistent config (`/var/lib/npcnix/config.json`)
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    remote: Option<Url>,
    #[serde(default = "default_configuration")]
    configuration: String,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_reader(std::fs::File::open(path)?)?)
    }

    pub fn store(&self, path: &Path) -> anyhow::Result<()> {
        crate::misc::store_json_pretty_to_file(path, self)
    }

    pub fn with_configuration(self, configuration: &str) -> Self {
        Self {
            configuration: configuration.into(),
            ..self
        }
    }

    pub fn with_remote(self, remote: &Url) -> Self {
        Self {
            remote: Some(remote.clone()),
            ..self
        }
    }

    pub fn remote(&self) -> anyhow::Result<&Url> {
        self.remote
            .as_ref()
            .ok_or_else(|| format_err!("Remote not set"))
    }

    pub fn configuration(&self) -> &str {
        &self.configuration
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&serde_json::to_string_pretty(self).map_err(|_e| fmt::Error)?)
    }
}
