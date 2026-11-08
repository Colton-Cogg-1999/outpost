//! Finds UCI engines that are already installed. Only well-known executable names
//! are considered; nothing is run until the user chooses to add one (which probes it).

use serde::Serialize;
use std::path::{Path, PathBuf};

const KNOWN: &[(&str, &str)] = &[
    ("stockfish", "Stockfish"),
    ("lc0", "Leela Chess Zero"),
    ("berserk", "Berserk"),
    ("ethereal", "Ethereal"),
    ("Ethereal", "Ethereal"),
    ("rubichess", "RubiChess"),
    ("RubiChess", "RubiChess"),
    ("koivisto", "Koivisto"),
    ("Koivisto", "Koivisto"),
    ("igel", "Igel"),
    ("seer", "Seer"),
    ("obsidian", "Obsidian"),
    ("caissa", "Caissa"),
    ("reckless", "Reckless"),
    ("viridithas", "Viridithas"),
    ("halogen", "Halogen"),
    ("fairy-stockfish", "Fairy-Stockfish"),
    ("crafty", "Crafty"),
    ("fruit", "Fruit"),
];

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub label: String,
}

pub fn search_dirs(extra: &[PathBuf]) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in [
        home.join(".local/bin"),
        home.join("bin"),
        PathBuf::from("/usr/games"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/snap/bin"),
    ] {
        dirs.push(d);
    }
    dirs.extend_from_slice(extra);
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| d.is_absolute() && seen.insert(d.clone()));
    dirs
}

pub fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    }
}

pub fn find(dirs: &[PathBuf]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        for (bin, label) in KNOWN {
            let mut path = dir.join(bin);
            if cfg!(windows) {
                path.set_extension("exe");
            }
            if is_executable(&path) {
                let real = std::fs::canonicalize(&path).unwrap_or(path.clone());
                if seen.insert(real) {
                    out.push(Candidate { path: path.to_string_lossy().to_string(), label: label.to_string() });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_executable_known_names() {
        let dir = tempfile::tempdir().unwrap();
        let sf = dir.path().join("stockfish");
        std::fs::write(&sf, "#!/bin/sh\n").unwrap();
        let other = dir.path().join("lc0");
        std::fs::write(&other, "not executable").unwrap();
        std::fs::write(dir.path().join("random-tool"), "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&sf, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::set_permissions(dir.path().join("random-tool"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let found = find(&[dir.path().to_path_buf()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label, "Stockfish");
    }
}
