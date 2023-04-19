#![doc = include_str!("../../README.md")]
use std::io::Write as _;
use std::path::PathBuf;
use std::{ffi::OsString, io};

use clap::{Parser, Subcommand};
use npcnix::data_dir::DataDir;
use tracing::trace;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use url::Url;

#[derive(Parser, Debug, Clone)]
struct Opts {
    #[clap(flatten)]
    common: npcnix::opts::Common,

    #[command(subcommand)]
    command: Command,
}

impl Opts {
    pub fn data_dir(&self) -> DataDir {
        self.common.data_dir()
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Configuration options
    Config {
        #[command(subcommand)]
        command: Option<ConfigOpts>,
    },
    /// Activate a NixOS configuration from a Nix Flake in a local directory
    Activate(ActivateOpts),
    /// Pack a Nix Flake in a local directory into a remote-like packed Nix Flake file
    Pack(PackOpts),
    /// Pull a packed Nix Flake from a remote and extra to a directory
    Pull(PullOpts),
    /// Pack a Nix Flake in a local directory into a packed Nix Flake file and upload to a remote
    Push(PushOpts),
    /// Install npcnix on the machine
    Install(InstallOpts),
    /// Run as a daemon periodically activating NixOS configuration from the
    /// remote
    Follow(FollowOpts),
    /// Permanently or temporarily pause the npcnix daemon
    Pause(PauseOpts),
    /// Unpause the npcnix daemon
    Unpause,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigOpts {
    Show,
    /// Change daemon settings
    Set {
        /// Only update if not already set
        #[arg(long)]
        init: bool,

        #[command(subcommand)]
        value: SetOpts,
    },
}

#[derive(Parser, Debug, Clone)]
pub struct PullOpts {
    /// Override the remote from config
    #[arg(long)]
    remote: Option<Url>,

    #[arg(long)]
    /// Destination directory
    dst: PathBuf,
}

#[derive(Parser, Debug, Clone)]
pub struct PauseOpts {
    /// Pause for this many hours
    #[arg(long, group("duration"))]
    hours: Option<u64>,

    #[arg(long, group("duration"))]
    minutes: Option<u64>,
}

#[derive(Parser, Debug, Clone)]
pub struct ActivateCommonOpts {
    #[arg(long)]
    extra_substituters: Vec<String>,

    #[arg(long)]
    extra_trusted_public_keys: Vec<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct ActivateOpts {
    #[arg(long, default_value = ".")]
    /// Source directory
    src: PathBuf,

    #[arg(long)]
    /// Configuration to apply
    configuration: Option<String>,

    #[command(flatten)]
    activate: ActivateCommonOpts,
}

#[derive(Parser, Debug, Clone)]
pub struct InstallOpts {
    #[arg(long)]
    /// Remote to use
    remote: Url,

    #[arg(long)]
    /// Configuration to apply
    configuration: String,

    #[command(flatten)]
    activate: ActivateCommonOpts,
}

impl From<ActivateCommonOpts> for npcnix::ActivateOpts {
    fn from(value: ActivateCommonOpts) -> Self {
        npcnix::ActivateOpts {
            extra_substituters: value.extra_substituters,
            extra_trusted_public_keys: value.extra_trusted_public_keys,
        }
    }
}

#[derive(Parser, Debug, Clone)]
pub struct PackCommonOpts {
    /// Source directory
    #[arg(long)]
    src: PathBuf,

    /// Include this subdirectory (can be specified multiple times; default:
    /// all)
    #[arg(long)]
    include: Vec<OsString>,
}

#[derive(Parser, Debug, Clone)]
pub struct PushOpts {
    #[command(flatten)]
    pack: PackCommonOpts,

    /// To prevent accidental push, remote is required
    #[arg(long)]
    remote: Url,
}

#[derive(Parser, Debug, Clone)]
pub struct PackOpts {
    #[command(flatten)]
    pack: PackCommonOpts,

    /// Destination file
    #[arg(long)]
    dst: PathBuf,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SetOpts {
    Remote { url: Url },
    Configuration { configuration: String },
}

#[derive(Parser, Debug, Clone)]
pub struct FollowOpts {
    #[command(flatten)]
    activate: ActivateCommonOpts,

    /// Stop after first success
    #[arg(long)]
    once: bool,
}

pub fn tracing_init() -> anyhow::Result<()> {
    let filter_layer = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(io::stderr)
        .with_filter(filter_layer);

    tracing_subscriber::registry().with(fmt_layer).init();
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_init()?;
    trace!("Staring npcnix");
    let opts = Opts::parse();

    match opts.command {
        Command::Pull(ref pull_opts) => npcnix::pull(
            &opts
                .data_dir()
                .get_current_remote_with_opt_override(pull_opts.remote.as_ref())?,
            &pull_opts.dst,
        )?,
        Command::Push(ref push_opts) => npcnix::push(
            &push_opts.pack.src,
            &push_opts.clone().pack.include.into_iter().collect(),
            &push_opts.remote,
        )?,
        Command::Pack(ref pack_opts) => npcnix::pack(
            &pack_opts.pack.src,
            &pack_opts.clone().pack.include.into_iter().collect(),
            &pack_opts.dst,
        )?,
        Command::Config { ref command } => match command {
            Some(ConfigOpts::Show) | None => {
                let _ = write!(std::io::stdout(), "{}", opts.data_dir().load_config()?);
            }
            Some(ConfigOpts::Set { init, ref value }) => match value {
                SetOpts::Remote { ref url } => opts.data_dir().store_config(
                    &opts
                        .data_dir()
                        .load_config()?
                        .with_remote_maybe_init(url, *init),
                )?,
                SetOpts::Configuration { ref configuration } => opts.data_dir().store_config(
                    &opts
                        .data_dir()
                        .load_config()?
                        .with_configuration_maybe_init(configuration, *init),
                )?,
            },
        },
        Command::Activate(ref activate_opts) => {
            if opts.data_dir().config_exist()? {
                let configuration = opts
                    .data_dir()
                    .get_current_configuration_with_opt_override(
                        activate_opts.configuration.as_deref(),
                    )?;
                npcnix::activate(
                    Some(&opts.data_dir()),
                    &activate_opts.src,
                    &configuration,
                    &activate_opts.clone().activate.into(),
                )?;
            } else {
                npcnix::activate(
                    None,
                    &activate_opts.src,
                    activate_opts.configuration.as_deref().ok_or_else(|| {
                        anyhow::format_err!("Must pass configuration to activate")
                    })?,
                    &activate_opts.clone().activate.into(),
                )?;
            }
        }
        Command::Follow(ref follow_opts) => {
            npcnix::follow(
                &opts.data_dir(),
                &follow_opts.clone().activate.into(),
                follow_opts.once,
            )?;
        }
        Command::Pause(PauseOpts { hours, minutes }) => {
            let config = opts.data_dir().load_config()?;

            let config = if let Some(minutes) = minutes {
                config.with_paused_until(
                    chrono::Utc::now()
                        + chrono::Duration::seconds(TryFrom::try_from(minutes.saturating_add(60))?),
                )
            } else if let Some(hours) = hours {
                config.with_paused_until(
                    chrono::Utc::now()
                        + chrono::Duration::seconds(TryFrom::try_from(
                            hours.saturating_add(60 * 60),
                        )?),
                )
            } else {
                config.with_paused_indefinitely()
            };

            opts.data_dir().store_config(&config)?;
        }
        Command::Unpause => {
            let config = opts.data_dir().load_config()?;
            opts.data_dir().store_config(&config.with_unpaused())?;
        }
        Command::Install(InstallOpts {
            ref remote,
            ref configuration,
            ref activate,
        }) => {
            opts.data_dir().store_config(
                &opts
                    .data_dir()
                    .load_config()?
                    .with_remote(remote)
                    .with_configuration(configuration),
            )?;

            npcnix::follow(&opts.data_dir(), &activate.clone().into(), true)?;
        }
    }

    Ok(())
}
