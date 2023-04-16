#![doc = include_str!("../README.md")]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Stdio};

use anyhow::bail;
use data_dir::DataDir;
use serde::Deserialize;
use tracing::{debug, info};
use url::Url;

pub mod config;
pub mod data_dir;
pub mod misc;
pub mod opts;

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

    pack_archive_from(src, include, &mut writer)?;
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
    let mut cmd = process::Command::new("nixos-rebuild");
    cmd.args(["switch"]);

    for subscriber in &activate_opts.extra_substituters {
        cmd.args(["--option", "extra-substituters", subscriber]);
    }
    for key in &activate_opts.extra_trusted_public_keys {
        cmd.args(["--option", "extra-trusted-public-keys", key]);
    }

    cmd.args(["--flake", &format!(".{configuration}")])
        .current_dir(src);

    debug!(
        cmd = [cmd.get_program()]
            .into_iter()
            .chain(cmd.get_args())
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" "),
        "Switching configuration"
    );

    cmd.status()?;
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

    pack_archive_from(src, include, &mut writer)?;
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
    let output = process::Command::new("aws")
        .args([
            "s3api",
            "get-object-attributes",
            "--bucket",
            remote
                .host_str()
                .ok_or_else(|| anyhow::format_err!("Invalid URL"))?,
            "--key",
            remote.path(),
            "--object-attributes",
            "ETag",
        ])
        .output()?;

    if !output.status.success() {
        bail!(
            "s3api get-object-attributes returned code={:?}",
            output.status.code()
        )
    }
    let resp: EtagResponse = serde_json::from_slice(&output.stdout)?;

    Ok(resp.etag)
}

fn pull_s3(remote: &Url) -> anyhow::Result<(impl Read, process::Child)> {
    // by default this has 60s read & connect timeouts, so should not just
    // hang, so no need for extra timeouts, I guess
    let mut child = process::Command::new("aws")
        .args(["s3", "cp", remote.as_str(), "-"])
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();

    Ok((stdout, child))
}

fn push_s3(remote: &Url) -> anyhow::Result<(impl Write, process::Child)> {
    let mut child = process::Command::new("aws")
        .args(["s3", "cp", "-", remote.as_str()])
        .stdin(Stdio::piped())
        .spawn()?;

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
        if path.is_dir() {
            if include.is_empty() || include.contains(file_name) {
                builder.append_dir_all(file_name, &path)?;
            }
        } else {
            builder.append_path_with_name(&path, file_name)?;
        }
    }
    builder.into_inner()?.finish()?;

    Ok(())
}

pub fn follow(data_dir: &DataDir, activate_opts: &ActivateOpts) -> anyhow::Result<()> {
    loop {
        // Note: we load every time, in case settings changed
        let config = &data_dir.load_config()?;
        config.rng_sleep();

        let etag = self::get_etag(config.remote()?)?;

        if config.last_etag() == etag {
            info!("Remote not changed");
            continue;
        }

        let tmp_dir = tempfile::TempDir::new()?;
        self::pull(config.remote()?, tmp_dir.path())?;
        self::activate(tmp_dir.path(), config.configuration(), activate_opts)?;
        data_dir.update_last_reconfiguration(&etag)?;
    }
}
