use std::path::{Path, PathBuf};

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
    save_token_at(&path, token)
}

/// Owner-only on unix: the token logs in without a password.
fn save_token_at(path: &Path, token: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    super::replace_private_file(path, token)
}

pub fn delete_token() {
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_the_token() {
        let dir = std::env::temp_dir().join(format!("rechimped-token-{}", std::process::id()));
        let path = dir.join("refresh_token");
        save_token_at(&path, "abc").unwrap();
        save_token_at(&path, "def").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "def");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn token_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rechimped-token-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("refresh_token");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        save_token_at(&path, "new").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        let _ = std::fs::remove_dir_all(dir);
    }
}
