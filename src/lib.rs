#![doc = include_str!("../README.md")]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::ops::ControlFlow;
use std::path::Path;
use std::process::{self, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, format_err, Context};
use config::Config;
use data_dir::DataDir;

use serde::Deserialize;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::flag;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Once {
    Any,
    Activate,
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

pub fn with_activate_lock<T>(
    data_dir: Option<&DataDir>,
    f: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let mut lock = data_dir
        .map(|data_dir| data_dir.activate_lock())
        .transpose()?
        .flatten();

    // Workaround: due to &mut aliasing limitations, it seems impossible to
    // `try_write` first, then `write` if previous one failed, on the same
    // locked file. So we open same files twice instead.
    let mut lock2 = data_dir
        .map(|data_dir| data_dir.activate_lock())
        .transpose()?
        .flatten();

    let _lock = match lock.as_mut().map(|lock| lock.try_write()).transpose() {
        Ok(_lock) => {
            return f();
        }
        Err(e) => {
            if e.kind() != io::ErrorKind::WouldBlock {
                return Err(e)?;
            }

            warn!("Waiting for another instance to finish");
            lock2.as_mut().map(|lock| lock.write()).transpose()?
        }
    };

    f()
}

pub fn activate(
    data_dir: Option<&DataDir>,
    src: &Path,
    configuration: &str,
    activate_opts: &ActivateOpts,
) -> Result<(), anyhow::Error> {
    with_activate_lock(data_dir, || {
        // Note: we load every time, in case settings changed
        activate_inner(src, configuration, activate_opts)?;
        data_dir
            .map(|data_dir| data_dir.update_last_reconfiguration(configuration, ""))
            .transpose()
    })?;
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

    let status = cmd
        .log_debug()
        .status()
        .context("Calling `nixos-rebuild` failed")?;
    if !status.success() {
        bail!("nixos-rebuild returned exit code={:?}", status.code());
    }
    Ok(())
}

pub fn pack(src: &Path, include: &HashSet<OsString>, dst: &Path) -> anyhow::Result<()> {
    verify_flake_src(src)?;

    let tmp_dst = dst.with_extension("tmp");
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp_dst)
        .with_context(|| format!("Could not create temporary file: {}", tmp_dst.display()))?;
    let mut writer = io::BufWriter::new(&file);

    pack_archive_from(src, include, &mut writer)
        .with_context(|| format!("Failed to pack the src archive: {}", src.display()))?;
    writer.flush()?;
    drop(writer);
    file.sync_data()?;
    drop(file);
    std::fs::rename(&tmp_dst, dst).with_context(|| {
        format!(
            "Could not rename temporary file: {} to the final destination: {}",
            tmp_dst.display(),
            dst.display()
        )
    })?;
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

pub fn follow(
    data_dir: &DataDir,
    activate_opts: &ActivateOpts,
    override_configuration: Option<&str>,
    once: Option<Once>,
    ignore_etag: bool,
) -> anyhow::Result<()> {
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let shutdown_on_signal = Arc::new(AtomicBool::new(false));

    for sig in TERM_SIGNALS {
        // On first signal, mark shutdown as requested
        flag::register(*sig, Arc::clone(&shutdown_requested))?;
        // Also make the second signal shutdown immediately
        flag::register(*sig, Arc::clone(&shutdown_on_signal))?;

        // If shutdown_on_signal was already set, shutdown immediately on signal
        flag::register_conditional_shutdown(*sig, 1, Arc::clone(&shutdown_on_signal))?;
    }

    while !shutdown_requested.load(Ordering::SeqCst) {
        if let ControlFlow::Break(()) = follow_inner(
            data_dir,
            activate_opts,
            override_configuration,
            once,
            ignore_etag,
        )? {
            break;
        }

        // reload the config, just in case it changed in the meantime
        let config = data_dir.load_config()?;
        // During sleep, shutdown immediately on any signal
        shutdown_on_signal.store(true, Ordering::SeqCst);
        config.rng_sleep();
        shutdown_on_signal.store(false, Ordering::SeqCst);
    }
    Ok(())
}

fn follow_inner(
    data_dir: &DataDir,
    activate_opts: &ActivateOpts,
    override_configuration: Option<&str>,
    once: Option<Once>,
    ignore_etag: bool,
) -> Result<ControlFlow<(), ()>, anyhow::Error> {
    with_activate_lock(Some(data_dir), || {
        // Note: we load every time, in case settings changed
        let config = data_dir.load_config()?;

        if config.is_paused() {
            info!("Paused");
        } else {
            match follow_inner_try(&config, activate_opts, override_configuration, ignore_etag) {
                Ok(res) => {
                    match res {
                        Some((ref configuration, ref etag)) => {
                            data_dir.update_last_reconfiguration(&configuration, &etag)?;
                            info!(etag, "Successfully activated new configuration");
                        }
                        None => {
                            info!("Remote not changed");
                        }
                    }
                    match (once, res.is_some()) {
                        (None, _) => {}
                        (Some(Once::Activate), false) => {}
                        (Some(Once::Any), _) | (Some(Once::Activate), true) => {
                            debug!("Exiting after success due to `once` option");
                            return Ok(ControlFlow::Break(()));
                        }
                    }
                }
                Err(e) => error!(error = %e, "Failed to activate new configuration"),
            }
        }
        Ok(ControlFlow::Continue(()))
    })
}

pub fn follow_inner_try(
    config: &Config,
    activate_opts: &ActivateOpts,
    override_configuration: Option<&str>,
    ignore_etag: bool,
) -> anyhow::Result<Option<(String, String)>> {
    let configuration = override_configuration
        .map(Ok)
        .unwrap_or_else(|| config.configuration())?;

    let etag = self::get_etag(config.remote()?)?;

    if !ignore_etag && config.last_configuration() == configuration && config.last_etag() == etag {
        return Ok(None);
    }

    let tmp_dir = tempfile::TempDir::new()?;
    self::pull(config.remote()?, tmp_dir.path())?;
    self::activate_inner(tmp_dir.path(), configuration, activate_opts)?;

    Ok(Some((configuration.to_string(), etag)))
}
