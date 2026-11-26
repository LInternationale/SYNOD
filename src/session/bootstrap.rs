//! Startup configuration shared by clients and the session process.
use crate::adapters::local::state::ClientConfig;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Roots {
    pub state: PathBuf,
    pub work: PathBuf,
}

pub fn configured(file: &Path) -> io::Result<Option<Roots>> {
    ClientConfig::read(file)?
        .fields
        .remove("roots")
        .map(serde_json::from_value)
        .transpose()
        .map_err(io::Error::other)
}

fn resolve(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Enter an absolute directory path",
        ));
    }
    if path.exists() {
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Expected a directory",
            ));
        }
        return path.canonicalize();
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Invalid directory name"))?;
    Ok(resolve(
        path.parent()
            .ok_or_else(|| io::Error::other("Missing directory parent"))?,
    )?
    .join(name))
}

pub fn initialize(file: &Path, state: &Path, work: &Path) -> io::Result<Roots> {
    let parent = file
        .parent()
        .ok_or_else(|| io::Error::other("Missing settings directory"))?;
    fs::create_dir_all(parent)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(parent.join("setup.lock"))?;
    lock.lock()?;
    if let Some(roots) = configured(file)? {
        return Ok(roots);
    }
    let state = resolve(state)?;
    let work = resolve(work)?;
    if state.starts_with(&work) || work.starts_with(&state) {
        return Err(io::Error::other("State and workspace directories overlap"));
    }
    fs::create_dir_all(&state)?;
    fs::create_dir_all(&work)?;
    let roots = Roots { state, work };
    let mut config = ClientConfig::read(file)?;
    config
        .fields
        .insert("roots".into(), serde_json::to_value(&roots)?);
    config.save(file)?;
    Ok(roots)
}
