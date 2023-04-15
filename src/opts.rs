use std::path::PathBuf;

use clap::Parser;

use crate::data_dir::DataDir;

#[derive(Parser, Debug, Clone)]
pub struct Common {
    #[arg(long, env = "NPCNIX_DATA_DIR", default_value = "/var/lib/npcnix")]
    data_dir: PathBuf,
}

impl Common {
    pub fn data_dir(&self) -> DataDir {
        DataDir::new(&self.data_dir)
    }
}
