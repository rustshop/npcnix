use std::io::Write as _;
use std::path::PathBuf;

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client;
use clap::Parser;

#[derive(Parser, Debug)]
struct Opts {
    #[arg(long, env = "NPCNIX_DATA_DIR", default_value = "/var/lib/npcnix")]
    data_dir: PathBuf,

    #[arg(long, env = "NPCNIX_AWS_REGION")]
    aws_region: Option<String>,

    #[arg(long, env = "NPCNIX_SRC")]
    src: url::Url,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    println!("{opts:?}");

    let scheme = opts.src.scheme();
    match scheme {
        "s3" => fetch_s3(&opts).await?,
        _ => anyhow::bail!("Protocol not supported: {scheme}"),
    }

    Ok(())
}

async fn fetch_s3(opts: &Opts) -> anyhow::Result<()> {
    let region_provider = RegionProviderChain::first_try(opts.aws_region.clone().map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));

    let config = aws_config::from_env().region(region_provider).load().await;

    // Set up S3 client with the appropriate region
    let client = Client::new(&config);

    let response = client
        .get_object()
        .bucket(
            opts.src
                .host_str()
                .ok_or_else(|| anyhow::format_err!("Missing bucket"))?,
        )
        .key(opts.src.path().trim_start_matches('/'))
        .send()
        .await?;

    let bytes = response.body.collect().await?.into_bytes();
    std::io::stdout().write_all(&bytes)?;

    Ok(())
}
