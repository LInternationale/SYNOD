//! Per-user client settings, independent of the team's repository and state root.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Persistent client settings. Fields added by other components survive updates.
#[derive(Debug, Deserialize, Serialize)]
pub struct ClientConfig {
    #[serde(default = "default_rgb")]
    pub theme_rgb: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_team: Option<crate::application::setup::PendingTeam>,
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

fn default_rgb() -> [u8; 3] {
    crate::DEFAULT_THEME_RGB
}
impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            theme_rgb: default_rgb(),
            fields: BTreeMap::new(),
            pending_team: None,
        }
    }
}

/// Resolve the client configuration file at process startup.
pub fn system_config_file() -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
    #[cfg(windows)]
    let base = env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));
    base.filter(|p| p.is_absolute())
        .map(|p| p.join("synod/client.json"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "User configuration directory unavailable",
            )
        })
}

impl ClientConfig {
    pub fn read(path: &Path) -> io::Result<Self> {
        match File::open(path) {
            Ok(file) => Ok(serde_json::from_reader(file)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Atomically install the complete settings document. The caller owns serialization of writers.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Missing settings directory")
        })?;
        fs::create_dir_all(parent)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let (temp, mut file) = loop {
            let name = format!(
                ".client-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let temp = parent.join(name);
            match options.open(&temp) {
                Ok(f) => break (temp, f),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        let result = (|| {
            serde_json::to_writer_pretty(&mut file, self)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)?;
            #[cfg(unix)]
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}
