use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{self, Stdio};

use anyhow::bail;
use serde::Deserialize;
use tracing::info;
use url::Url;

pub mod config;
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

pub fn push(src: &Path, remote: &url::Url) -> anyhow::Result<()> {
    let scheme = remote.scheme();
    let (mut writer, mut child) = match scheme {
        "s3" => push_s3(remote)?,
        _ => anyhow::bail!("Protocol not supported: {scheme}"),
    };

    pack_archive_from(src, &mut writer)?;
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

pub fn activate(src: &Path, configuration: &str) -> Result<(), anyhow::Error> {
    info!(
        configuration,
        src = %src.display(),
        "Activating configuration"
    );
    process::Command::new("aws")
        .args([
            "nixos-rebuild",
            "switch",
            "--flake",
            &format!(".{configuration}"),
        ])
        .current_dir(src)
        .status()?;
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

fn pack_archive_from(src: &Path, writer: impl Write) -> io::Result<()> {
    let encoder = zstd::stream::Encoder::new(writer, 0)?;
    let mut builder = tar::Builder::new(encoder);
    builder.append_dir_all(".", src)?;
    builder.into_inner()?.finish()?;

    Ok(())
}
