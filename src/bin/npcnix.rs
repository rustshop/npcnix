#![doc = include_str!("../../README.md")]
use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use npcnix::data_dir::DataDir;
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
    #[command(subcommand)]
    /// Change daemon settings
    Set(SetOpts),
    /// Show daemon config
    Show,
    /// Pack a Nix Flake in a directory into a file
    Pack(PackOpts),
    /// Pull a packed Nix Flake from a remote and extra to a directory
    Pull(PullOpts),
    /// Pack a Nix Flake in a directory into a file and upload to a remote
    Push(PushOpts),
    /// Activate a NixOS configuration from a Nix Flake in a directory
    Activate(ActivateOpts),
    /// Run as a daemon periodically activating NixOS configuration from the
    /// remote
    Follow(FollowOpts),
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
    Remote {
        /// Only update if not already set
        #[arg(long)]
        init: bool,
        url: Url,
    },
    Configuration {
        /// Only update if not already set
        #[arg(long)]
        init: bool,
        configuration: String,
    },
}

#[derive(Parser, Debug, Clone)]
pub struct FollowOpts {
    #[command(flatten)]
    activate: ActivateCommonOpts,
}

fn main() -> anyhow::Result<()> {
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
        Command::Set(ref set_opts) => match set_opts {
            SetOpts::Remote { url, init } => opts.data_dir().store_config(
                &opts
                    .data_dir()
                    .load_config()?
                    .with_remote_maybe_init(url, *init),
            )?,
            SetOpts::Configuration {
                configuration,
                init,
            } => opts.data_dir().store_config(
                &opts
                    .data_dir()
                    .load_config()?
                    .with_configuration_maybe_init(configuration, *init),
            )?,
        },
        Command::Show => {
            let _ = write!(std::io::stdout(), "{}", opts.data_dir().load_config()?);
        }
        Command::Activate(ref activate_opts) => {
            let configuration = opts
                .data_dir()
                .get_current_configuration_with_opt_override(
                    activate_opts.configuration.as_deref(),
                )?;
            npcnix::activate(
                &activate_opts.src,
                &configuration,
                &activate_opts.clone().activate.into(),
            )?;
        }
        Command::Follow(ref follow_opts) => {
            npcnix::follow(&opts.data_dir(), &follow_opts.clone().activate.into())?;
        }
    }

    Ok(())
}