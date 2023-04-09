use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{self, Stdio};

use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
struct Opts {
    #[arg(long, env = "NPCNIX_DATA_DIR", default_value = "/var/lib/npcnix")]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    Pull(PullOpts),
    Push(PushOpts),
}

#[derive(Parser, Debug, Clone)]
pub struct PullOpts {
    #[arg(long, env = "NPCNIX_REMOTE")]
    remote: url::Url,

    #[arg(long, env = "NPCNIX_DST")]
    dst: PathBuf,
}

#[derive(Parser, Debug, Clone)]
pub struct PushOpts {
    #[arg(long, env = "NPCNIX_REMOTE")]
    remote: url::Url,

    #[arg(long, env = "NPCNIX_SRC")]
    src: PathBuf,
}
fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    println!("{opts:?}");

    match opts.command {
        Command::Pull(ref pull_opts) => pull(&opts, pull_opts)?,
        Command::Push(ref push_opts) => push(&opts, push_opts)?,
    }

    Ok(())
}

fn pull(_opts: &Opts, pull_opts: &PullOpts) -> anyhow::Result<()> {
    let scheme = pull_opts.remote.scheme();
    let (reader, mut child) = match scheme {
        "s3" => pull_s3(pull_opts)?,
        _ => anyhow::bail!("Protocol not supported: {scheme}"),
    };

    unpack_archive(pull_opts, reader)?;
    child.wait()?;

    Ok(())
}

fn push(_opts: &Opts, push_opts: &PushOpts) -> anyhow::Result<()> {
    let scheme = push_opts.remote.scheme();
    let (mut writer, mut child) = match scheme {
        "s3" => push_s3(push_opts)?,
        _ => anyhow::bail!("Protocol not supported: {scheme}"),
    };

    pack_archive(push_opts, &mut writer)?;
    writer.flush()?;
    drop(writer);

    child.wait()?;

    Ok(())
}

fn pull_s3(opts: &PullOpts) -> anyhow::Result<(impl Read, process::Child)> {
    let mut child = process::Command::new("aws")
        .args(["s3", "cp", opts.remote.as_str(), "-"])
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();

    Ok((stdout, child))
}

fn push_s3(opts: &PushOpts) -> anyhow::Result<(impl Write, process::Child)> {
    let mut child = process::Command::new("aws")
        .args(["s3", "cp", "-", opts.remote.as_str()])
        .stdin(Stdio::piped())
        .spawn()?;

    let stdin = child.stdin.take().unwrap();

    Ok((stdin, child))
}

fn unpack_archive(opts: &PullOpts, reader: impl Read) -> io::Result<()> {
    fs::create_dir_all(&opts.dst)?;

    let decoder = zstd::stream::Decoder::new(reader)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(&opts.dst)?;

    Ok(())
}
fn pack_archive(opts: &PushOpts, writer: impl Write) -> io::Result<()> {
    let encoder = zstd::stream::Encoder::new(writer, 0)?;
    let mut builder = tar::Builder::new(encoder);
    builder.append_dir_all(".", &opts.src)?;
    builder.into_inner()?.finish()?;

    Ok(())
}
