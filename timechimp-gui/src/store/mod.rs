//! Files in the config directory: `config.toml` and the session token.
//!
//! The directory is `timechimp-tui` for historical reasons (the former terminal UI); keeping the
//! path means existing sessions stay valid.

pub mod config;
pub mod token;

use std::path::PathBuf;

pub fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("timechimp-tui"))
}
