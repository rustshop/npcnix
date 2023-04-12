use std::io::Write as _;
use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use url::Url;

#[derive(Parser, Debug, Clone)]
struct Opts {
    #[clap(flatten)]
    common: npcnix::opts::Common,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    #[command(subcommand)]
    Set(SetOpts),
    Config,
    Pull(PullOpts),
    Push(PushOpts),
    Activate(ActivateOpts),
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
pub struct ActivateOpts {
    #[arg(long, default_value = ".")]
    /// Source directory
    src: PathBuf,

    #[arg(long)]
    /// Configuration to apply
    configuration: Option<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct PushOpts {
    /// To prevent accidental push, remote is required
    #[arg(long)]
    remote: Url,

    /// Source directory
    #[arg(long)]
    src: PathBuf,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SetOpts {
    Remote { url: Url },
    Configuration { configuration: String },
}

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    match opts.command {
        Command::Pull(ref pull_opts) => npcnix::pull(
            &opts
                .common
                .get_current_remote_with_opt_override(pull_opts.remote.as_ref())?,
            &pull_opts.dst,
        )?,
        Command::Push(ref push_opts) => npcnix::push(&push_opts.src, &push_opts.remote)?,
        Command::Set(ref set_opts) => match set_opts {
            SetOpts::Remote { url } => opts
                .common
                .store_config(&opts.common.load_config()?.with_remote(url))?,
            SetOpts::Configuration { configuration } => opts
                .common
                .store_config(&opts.common.load_config()?.with_configuration(configuration))?,
        },
        Command::Config => {
            let _ = write!(std::io::stdout(), "{}", opts.common.load_config()?);
        }
        Command::Activate(ref activate_opts) => {
            let configuration = opts.common.get_current_configuration_with_opt_override(
                activate_opts.configuration.as_deref(),
            )?;

            process::Command::new("aws")
                .args([
                    "nixos-rebuild",
                    "switch",
                    "--flake",
                    &format!(".{}", &configuration),
                ])
                .current_dir(&activate_opts.src)
                .status()?;
        }
    }

    Ok(())
}
