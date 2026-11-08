use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Everything OUTPOST owns lives under one directory, so copying that directory
/// (with the application closed) is a complete backup.
#[derive(Clone, Debug)]
pub struct Paths {
    pub data: PathBuf,
    pub engines: PathBuf,
    pub downloads: PathBuf,
    pub ai_cwd: PathBuf,
    pub db: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self> {
        let data = match std::env::var_os("OUTPOST_DATA_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => {
                let base = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
                    .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
                    .context("neither XDG_DATA_HOME, HOME nor APPDATA is set")?;
                base.join("outpost")
            }
        };
        Self::at(data)
    }

    pub fn at(data: PathBuf) -> Result<Self> {
        let paths = Paths {
            engines: data.join("engines"),
            downloads: data.join("downloads"),
            ai_cwd: data.join("ai-workspace"),
            db: data.join("outpost.db"),
            data,
        };
        for dir in [&paths.data, &paths.engines, &paths.downloads, &paths.ai_cwd] {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&paths.data, fs::Permissions::from_mode(0o700))?;
        }
        Ok(paths)
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
