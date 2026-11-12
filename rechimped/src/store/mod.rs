pub mod config;
pub mod reminder;
pub mod token;

use std::path::PathBuf;
use std::sync::OnceLock;

/// Settled on the first read, so every file goes to the same place for the whole run.
static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Puts every file (config, token, reminder) in `dir` instead, for `--mock`.
///
/// Panics once a file path has been worked out: some files would already be in the real folder.
pub fn override_dir(dir: PathBuf) {
    DIR.set(Some(dir)).expect("store::override_dir after the config folder was in use");
}

pub fn config_dir() -> Option<PathBuf> {
    DIR.get_or_init(default_dir).clone()
}

fn default_dir() -> Option<PathBuf> {
    // Empty variables count as unset, as the XDG spec asks for XDG_CONFIG_HOME.
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = var("APPDATA")
        .map(PathBuf::from)
        .or_else(|| var("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("rechimped"))
}
