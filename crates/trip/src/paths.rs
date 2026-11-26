//! Where tripppin keeps its settings and data (XDG, with the usual fallbacks).

use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// `~/.config/tripppin`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from).join("tripppin")
}

/// `~/.local/share/tripppin`.
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map_or_else(|| home().join(".local/share"), PathBuf::from).join("tripppin")
}

/// `~/.cache/tripppin`.
pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home().join(".cache"), PathBuf::from).join("tripppin")
}

/// Makes a file readable by its owner only (keys live in files like this).
pub fn private(file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
}

/// `~/x` to the home folder's path.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if path == "~" => home(),
        None => PathBuf::from(path),
    }
}
