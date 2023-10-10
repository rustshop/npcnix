use std::path::Path;
use std::{cmp, fmt, thread};

use anyhow::format_err;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::debug;
use url::Url;

fn default_min_sleep_secs() -> u64 {
    5
}

fn default_max_sleep_secs() -> u64 {
    120
}

fn default_max_sleep_after_hours() -> u64 {
    24
}
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ConfigPaused {
    Indefinitely,
    Until {
        until: chrono::DateTime<chrono::Utc>,
    },
}
impl ConfigPaused {
    pub fn combine(self, other: Self) -> ConfigPaused {
        match (self, other) {
            (ConfigPaused::Indefinitely, _) | (_, ConfigPaused::Indefinitely) => {
                ConfigPaused::Indefinitely
            }
            (ConfigPaused::Until { until: until1 }, ConfigPaused::Until { until: until2 }) => {
                Self::Until {
                    until: cmp::max(until1, until2),
                }
            }
        }
    }

    fn is_expired(self) -> bool {
        match self {
            ConfigPaused::Indefinitely => false,
            ConfigPaused::Until { until } => until <= Utc::now(),
        }
    }
}

/// Persistent config (`/var/lib/npcnix/config.json`)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    remote: Option<Url>,
    remote_region: Option<String>,
    configuration: Option<String>,
    last_reconfiguration: chrono::DateTime<chrono::Utc>,
    last_etag: String,
    last_configuration: String,
    #[serde(default = "default_min_sleep_secs")]
    min_sleep_secs: u64,
    #[serde(default = "default_max_sleep_secs")]
    max_sleep_secs: u64,
    #[serde(default = "default_max_sleep_after_hours")]
    max_sleep_after_hours: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    paused: Option<ConfigPaused>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            remote: None,
            remote_region: None,
            configuration: None,
            last_reconfiguration: chrono::Utc::now(),
            last_etag: "".into(),
            last_configuration: "".into(),
            min_sleep_secs: default_min_sleep_secs(),
            max_sleep_secs: default_max_sleep_secs(),
            max_sleep_after_hours: default_max_sleep_after_hours(),
            paused: None,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_reader::<_, Self>(std::fs::File::open(path)?)?.expire_paused())
    }

    pub fn store(&self, path: &Path) -> anyhow::Result<()> {
        crate::misc::store_json_pretty_to_file(path, &self.clone().expire_paused())
    }

    pub fn expire_paused(self) -> Self {
        if self.is_paused() {
            self
        } else {
            Self {
                paused: None,
                ..self
            }
        }
    }

    pub fn with_configuration(self, configuration: &str) -> Self {
        Self {
            configuration: Some(configuration.into()),
            ..self
        }
    }

    /// Like [`Self::with_configuration`] but if `init` is `true` will not
    /// overwrite the existing value
    pub fn with_configuration_maybe_init(self, configuration: &str, init: bool) -> Self {
        if !init || self.configuration.is_none() {
            self.with_configuration(configuration)
        } else {
            self
        }
    }

    pub fn with_remote(self, remote: &Url) -> Self {
        Self {
            remote: Some(remote.clone()),
            ..self
        }
    }

    pub fn with_remote_region(self, remote_region: Option<&str>) -> Self {
        Self {
            remote_region: remote_region.map(ToString::to_string),
            ..self
        }
    }

    pub fn with_paused_until(self, until: chrono::DateTime<chrono::Utc>) -> Self {
        let until = ConfigPaused::Until { until };
        Self {
            paused: Some(
                self.paused
                    .map(|current| current.combine(until))
                    .unwrap_or(until),
            ),
            ..self
        }
    }

    pub fn with_paused_indefinitely(self) -> Self {
        Self {
            paused: Some(ConfigPaused::Indefinitely),
            ..self
        }
    }

    pub fn with_unpaused(self) -> Self {
        Self {
            paused: None,
            ..self
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
            .map(|paused| !paused.is_expired())
            .unwrap_or(false)
    }

    /// Like [`Self:with_remote`] but if `init` is `true` will not overwrite the
    /// existing value
    pub fn with_remote_maybe_init(self, remote: &Url, init: bool) -> Self {
        if !init || self.remote.is_none() {
            self.with_remote(remote)
        } else {
            self
        }
    }

    pub fn with_updated_last_reconfiguration(self, configuration: &str, etag: &str) -> Self {
        Self {
            last_configuration: configuration.to_owned(),
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

    pub fn region_opt(&self) -> Option<&str> {
        self.remote_region.as_deref()
    }

    pub fn configuration(&self) -> anyhow::Result<&str> {
        self.configuration
            .as_deref()
            .ok_or_else(|| format_err!("configuration not set"))
    }

    pub fn cur_rng_sleep_time(&self) -> chrono::Duration {
        use rand::Rng;

        let since_last_update = cmp::max(
            chrono::Duration::seconds(1),
            chrono::Utc::now() - self.last_reconfiguration,
        );

        let duration_ratio = (since_last_update.num_seconds() as f32
            / self.max_sleep_after_hours.saturating_mul(60 * 60) as f32)
            .clamp(0f32, 1f32);
        assert!(0f32 <= duration_ratio);

        let avg_duration_secs = (self.min_sleep_secs as f32
            + duration_ratio * self.max_sleep_secs.saturating_sub(self.min_sleep_secs) as f32)
            .clamp(0.01, 60f32 * 60f32);
        let rnd_time =
            rand::thread_rng().gen_range(avg_duration_secs * 0.5..=avg_duration_secs * 1.5);
        assert!(0f32 < rnd_time);

        chrono::Duration::seconds(cmp::max(self.min_sleep_secs as i64, rnd_time as i64))
    }

    pub fn rng_sleep(&self) {
        let duration = self.cur_rng_sleep_time();
        debug!(duration = %duration, "Sleeping");
        thread::sleep(duration.to_std().expect("Can't be negative"));
    }

    pub fn last_configuration(&self) -> &str {
        &self.last_configuration
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
