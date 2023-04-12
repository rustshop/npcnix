use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

pub fn store_json_pretty_to_file<T>(path: &Path, val: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    Ok(store_to_file_with(path, |f| {
        serde_json::to_writer_pretty(f, val).map_err(Into::into)
    })
    .and_then(|res| res)?)
}

pub fn store_str_to_file(path: &Path, s: &str) -> io::Result<()> {
    store_to_file_with(path, |f| f.write_all(s.as_bytes())).and_then(|res| res)
}

pub fn store_to_file_with<E, F>(path: &Path, f: F) -> io::Result<Result<(), E>>
where
    F: Fn(&mut dyn io::Write) -> Result<(), E>,
{
    std::fs::create_dir_all(path.parent().expect("Not a root path"))?;
    let tmp_path = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp_path)?;
    if let Err(e) = f(&mut file) {
        return Ok(Err(e));
    }
    file.flush()?;
    file.sync_data()?;
    drop(file);
    std::fs::rename(tmp_path, path)?;
    Ok(Ok(()))
}
