//! The day the travel reminder was last dismissed ("I didn't travel today"), so it stays away for
//! the rest of that day only.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use super::config_dir;

pub fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("travel_reminder_dismissed"))
}

pub fn dismissed_on(path: &Path) -> Option<NaiveDate> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

pub fn dismiss(path: &Path, day: NaiveDate) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(path, day.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("rechimped-reminder-{}", std::process::id()));
        let path = dir.join("travel_reminder_dismissed");
        assert_eq!(dismissed_on(&path), None, "missing file");
        let day = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        dismiss(&path, day).unwrap();
        assert_eq!(dismissed_on(&path), Some(day));
        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(dismissed_on(&path), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
