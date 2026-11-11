pub mod config;
pub mod reminder;
pub mod token;

use std::path::PathBuf;

pub fn config_dir() -> Option<PathBuf> {
    // Empty variables count as unset, as the XDG spec asks for XDG_CONFIG_HOME.
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = var("APPDATA")
        .map(PathBuf::from)
        .or_else(|| var("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("rechimped"))
}
