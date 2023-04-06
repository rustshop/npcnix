use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
struct Opts {
    #[arg(long, env = "NPCNIX_DATA_DIR", default_value = "/var/lib/npcnix")]
    data_dir: PathBuf,
}

fn main() {
    let opts = Opts::parse();

    println!("{opts:?}");
}
