#![doc = include_str!("../README.md")]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Stdio};

use anyhow::{bail, format_err, Context};
use config::Config;
use data_dir::DataDir;
use serde::Deserialize;
use tracing::{debug, error, info, trace, warn};
use url::Url;

pub mod config;
pub mod data_dir;
pub mod misc;
pub mod opts;

pub trait CommandExt {
    fn log_debug(&mut self) -> &mut Self;
}

impl CommandExt for process::Command {
    fn log_debug(&mut self) -> &mut Self {
        debug!(
            cmd = [self.get_program()]
                .into_iter()
                .chain(self.get_args())
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" "),
            "Executing command"
        );
        self
    }
}

pub fn aws_cli_path() -> OsString {
    std::env::var_os("NPCNIX_AWS_CLI").unwrap_or_else(|| OsString::from("aws"))
}

pub fn nixos_rebuild_path() -> OsString {
    std::env::var_os("NPCNIX_NIXOS_REBUILD").unwrap_or_else(|| OsString::from("nixos-rebuild"))
}

pub fn pull(remote: &Url, dst: &Path) -> anyhow::Result<()> {
    let scheme = remote.scheme();
    let (reader, mut child) = match scheme {
        "s3" => pull_s3(remote)?,
        _ => anyhow::bail!("Protocol not supported: {scheme}"),
    };

    unpack_archive_to(reader, dst)?;
    child.wait()?;

    Ok(())
}

pub fn push(src: &Path, include: &HashSet<OsString>, remote: &url::Url) -> anyhow::Result<()> {
    verify_flake_src(src)?;
    let scheme = remote.scheme();
    let (mut writer, mut child) = match scheme {
        "s3" => push_s3(remote)?,
        _ => anyhow::bail!("Protocol not supported: {scheme}"),
    };

    pack_archive_from(src, include, &mut writer).context("Failed to pack the src archive")?;
    writer.flush()?;
    drop(writer);

    child.wait()?;

    Ok(())
}

pub fn get_etag(remote: &Url) -> anyhow::Result<String> {
    let scheme = remote.scheme();
    Ok(match scheme {
        "s3" => get_etag_s3(remote)?,
        _ => anyhow::bail!("Protocol not supported: {scheme}"),
    })
}

#[derive(Debug, Clone)]
pub struct ActivateOpts {
    pub extra_substituters: Vec<String>,
    pub extra_trusted_public_keys: Vec<String>,
}

pub fn activate(
    data_dir: Option<&DataDir>,
    src: &Path,
    configuration: &str,
    activate_opts: &ActivateOpts,
) -> Result<(), anyhow::Error> {
    activate_inner(src, configuration, activate_opts)?;
    data_dir
        .map(|data_dir| data_dir.update_last_reconfiguration(""))
        .transpose()?;
    Ok(())
}

fn activate_inner(
    src: &Path,
    configuration: &str,
    activate_opts: &ActivateOpts,
) -> Result<(), anyhow::Error> {
    verify_flake_src(src)?;
    info!(
        configuration,
        src = %src.display(),
        "Activating configuration"
    );
    let mut cmd = process::Command::new(nixos_rebuild_path());
    cmd.args(["switch", "-L"]);

    for subscriber in &activate_opts.extra_substituters {
        cmd.args(["--option", "extra-substituters", subscriber]);
    }
    for key in &activate_opts.extra_trusted_public_keys {
        cmd.args(["--option", "extra-trusted-public-keys", key]);
    }

    cmd.args(["--flake", &format!(".#{configuration}")])
        .current_dir(src);

    let status = cmd.log_debug().status().context("`nixos-rebuild` failed")?;
    if !status.success() {
        bail!(
            "aws s3api get-object-attributes returned code={:?}",
            status.code(),
        )
    }
    Ok(())
}

pub fn pack(src: &Path, include: &HashSet<OsString>, dst: &Path) -> anyhow::Result<()> {
    verify_flake_src(src)?;

    let mut writer = io::BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)?,
    );

    pack_archive_from(src, include, &mut writer).context("Failed to pack the src archive")?;
    writer.flush()?;
    drop(writer);
    Ok(())
}

fn verify_flake_src(src: &Path) -> anyhow::Result<()> {
    if !src.join("flake.nix").exists() {
        anyhow::bail!(
            "Flake source directory {} does not contain flake.nix file",
            src.display()
        );
    }
    Ok(())
}

#[derive(Deserialize)]
struct EtagResponse {
    #[serde(rename = "ETag")]
    etag: String,
}

fn get_etag_s3(remote: &Url) -> anyhow::Result<String> {
    let output = process::Command::new(aws_cli_path())
        .args([
            "s3api",
            "get-object-attributes",
            "--bucket",
            remote
                .host_str()
                .ok_or_else(|| format_err!("Invalid URL"))?,
            "--key",
            remote
                .path()
                .split_once('/')
                .ok_or_else(|| format_err!("Path doesn't start with a /"))?
                .1,
            "--object-attributes",
            "ETag",
        ])
        .log_debug()
        .output()
        .context("`aws` cli failed")?;

    if !output.status.success() {
        bail!(
            "aws s3api get-object-attributes returned code={:?} stdout={} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    }
    let resp: EtagResponse = serde_json::from_slice(&output.stdout)?;

    Ok(resp.etag)
}

fn pull_s3(remote: &Url) -> anyhow::Result<(impl Read, process::Child)> {
    // by default this has 60s read & connect timeouts, so should not just
    // hang, so no need for extra timeouts, I guess
    let mut child = process::Command::new(aws_cli_path())
        .args(["s3", "cp", remote.as_str(), "-"])
        .stdout(Stdio::piped())
        .log_debug()
        .spawn()
        .context("`aws` cli failed")?;

    let stdout = child.stdout.take().unwrap();

    Ok((stdout, child))
}

fn push_s3(remote: &Url) -> anyhow::Result<(impl Write, process::Child)> {
    let mut child = process::Command::new(aws_cli_path())
        .args(["s3", "cp", "-", remote.as_str()])
        .stdin(Stdio::piped())
        .log_debug()
        .spawn()
        .context("`aws` cli failed")?;

    let stdin = child.stdin.take().unwrap();

    Ok((stdin, child))
}

fn unpack_archive_to(reader: impl Read, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;

    let decoder = zstd::stream::Decoder::new(reader)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dst)?;

    Ok(())
}

fn pack_archive_from(
    src: &Path,
    include: &HashSet<OsString>,
    writer: impl Write,
) -> io::Result<()> {
    let encoder = zstd::stream::Encoder::new(writer, 0)?;
    let mut builder = tar::Builder::new(encoder);
    let paths = fs::read_dir(src)?;
    for path in paths {
        let entry = path?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .expect("read_dir must return only items with valid file_name");
        let metadata = path.symlink_metadata()?;
        trace!(
            src = %path.display(),
            "Considering path for archive inclusion"
        );
        if metadata.is_dir() {
            if include.is_empty() || include.contains(file_name) {
                trace!(src = %path.display(), "Packing directory");
                builder.append_dir_all(file_name, &path)?;
            } else {
                debug!(
                    src = %path.display(),
                    "Ignoring directory with no 'include'"
                );
            }
        } else if metadata.is_symlink() {
            let path_target = path.read_link()?;
            if !path_target.is_absolute() {
                trace!(src = %path.display(),
                    
                    target = %path_target.display(),
                     "Packing relative symlink");
                builder.append_path_with_name(&path, file_name)?;
            } else {
                warn!(
                    src = %path.display(),
                    "Ignoring absolute symlink"
                );
            }
        } else if metadata.is_file() {
            trace!(src = %path.display(), "Packing file");
            builder.append_path_with_name(&path, file_name)?;
        } else {
            warn!(src = %path.display(), "Ignoring unknown file type");
        }
    }
    builder.into_inner()?.finish()?;

    Ok(())
}

pub fn follow(data_dir: &DataDir, activate_opts: &ActivateOpts, once: bool) -> anyhow::Result<()> {
    loop {
        // Note: we load every time, in case settings changed
        let config = data_dir.load_config()?;

        if config.is_paused() {
            info!("Paused");
            config.rng_sleep();
            continue;
        }

        match follow_inner(&config, activate_opts) {
            Ok(Some(etag)) => {
                data_dir.update_last_reconfiguration(&etag)?;
                info!(etag, "Successfully activated new configuration");

                if once {
                    debug!("Exiting after successful activation with `once` option");
                    return Ok(());
                }
            }
            Ok(None) => {
                info!("Remote not changed");
            }
            Err(e) => error!(error = %e, "Failed to activate new configuration"),
        }

        config.rng_sleep();
    }
}

pub fn follow_inner(
    config: &Config,
    activate_opts: &ActivateOpts,
) -> anyhow::Result<Option<String>> {
    let etag = self::get_etag(config.remote()?)?;

    if config.last_etag() == etag {
        return Ok(None);
    }

    let tmp_dir = tempfile::TempDir::new()?;
    self::pull(config.remote()?, tmp_dir.path())?;
    self::activate_inner(tmp_dir.path(), config.configuration()?, activate_opts)?;

    Ok(Some(etag))
}
