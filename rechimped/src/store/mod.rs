pub mod config;
pub mod reminder;
pub mod token;

use std::io::Write;
use std::path::{Path, PathBuf};
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

/// Via a temporary file in the same folder and a rename, so a crash or full disk halfway leaves the
/// old file intact rather than truncated.
pub fn replace_file(path: &Path, contents: &str) -> std::io::Result<()> {
    write_then_rename(path, contents, false)
}

/// As `replace_file`, but on unix readable by the owner only (0600), also when the file it replaces
/// was not: the rename puts the new file, with its mode, in the old one's place.
pub fn replace_private_file(path: &Path, contents: &str) -> std::io::Result<()> {
    write_then_rename(path, contents, true)
}

fn write_then_rename(path: &Path, contents: &str, private: bool) -> std::io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    // A leftover from a crash would keep its old mode: create_new needs it gone.
    let _ = std::fs::remove_file(&temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let result =
        options.open(&temp).and_then(|mut file| file.write_all(contents.as_bytes())).and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_file_overwrites() {
        let dir = std::env::temp_dir().join(format!("rechimped-replace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.toml");
        replace_file(&path, "one").unwrap();
        replace_file(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert!(!dir.join("file.toml.tmp").exists(), "no temp file left");
        let _ = std::fs::remove_dir_all(dir);
    }
}
