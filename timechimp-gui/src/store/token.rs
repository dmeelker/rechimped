//! The session's refresh token, so the app only asks for the password once.

use std::path::PathBuf;

use super::config_dir;

fn token_path() -> Option<PathBuf> {
    Some(config_dir()?.join("refresh_token"))
}

pub fn load_token() -> Option<String> {
    std::env::var("TIMECHIMP_REFRESH_TOKEN")
        .ok()
        .or_else(|| std::fs::read_to_string(token_path()?).ok())
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

pub fn save_token(token: &str) -> std::io::Result<()> {
    let path = token_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(path, token)
}

pub fn delete_token() {
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(path);
    }
}
