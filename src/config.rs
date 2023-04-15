use std::path::Path;
use std::{cmp, fmt, thread};

use anyhow::format_err;
use serde::{Deserialize, Serialize};
use tracing::debug;
use url::Url;

fn default_configuration() -> String {
    "nixos".into()
}
fn default_max_sleep_secs() -> u64 {
    120
}

/// Persistent config (`/var/lib/npcnix/config.json`)
#[derive(Serialize, Default, Deserialize, Debug)]
pub struct Config {
    remote: Option<Url>,
    #[serde(default = "default_configuration")]
    configuration: String,
    last_reconfiguration: chrono::DateTime<chrono::Utc>,
    last_etag: String,
    #[serde(default = "default_max_sleep_secs")]
    max_sleep_secs: u64,
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

    pub fn with_updated_last_reconfiguration(self, etag: &str) -> Self {
        Self {
            last_etag: etag.to_owned(),
            last_reconfiguration: chrono::Utc::now(),
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

    pub fn cur_rng_sleep_time(&self) -> chrono::Duration {
        use rand::Rng;

        let since_last_update = cmp::max(
            chrono::Duration::seconds(1),
            chrono::Utc::now() - self.last_reconfiguration,
        );

        let secs_in_a_day = 24 * 60 * 60;
        let ratio =
            (since_last_update.num_seconds() as f32 / secs_in_a_day as f32).clamp(0f32, 1f32);
        assert!(0f32 <= ratio);

        let base_time = ratio * self.max_sleep_secs as f32;
        let rnd_time = rand::thread_rng().gen_range(base_time * 0.5..base_time * 1.5);
        assert!(0f32 <= rnd_time);

        chrono::Duration::seconds(rnd_time as i64)
    }

    pub fn rng_sleep(&self) {
        let duration = self.cur_rng_sleep_time();
        debug!(duration = %duration, "Sleeping");
        thread::sleep(duration.to_std().expect("Can't be negative"));
    }

    pub fn last_etag(&self) -> &str {
        &self.last_etag
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&serde_json::to_string_pretty(self).map_err(|_e| fmt::Error)?)
    }
}
