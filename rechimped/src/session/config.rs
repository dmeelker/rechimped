use std::collections::HashMap;

use timechimp_api::Tag;

use crate::store::config::{Settings, TimePreset, TripPreset};

/// What config.toml holds, as loaded for this session. Changed only through `Session`, which
/// resolves it against the account's projects and tags, and redoes the review when the settings
/// change.
#[derive(Default)]
pub struct ConfigState {
    /// Read through `seed::tags_for`, which offers all tags while none are configured.
    project_tags: HashMap<i64, Vec<Tag>>,
    settings: Settings,
    trip_presets: Vec<TripPreset>,
    time_presets: Vec<TimePreset>,
    /// Why config.toml failed to load. While set nothing writes to it: the defaults shown
    /// meanwhile would be saved over what the file holds.
    error: Option<String>,
}

impl ConfigState {
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The tags configured per project id; offering them goes through `seed::tags_for`.
    pub fn project_tags(&self) -> &HashMap<i64, Vec<Tag>> {
        &self.project_tags
    }

    pub fn trip_presets(&self) -> &[TripPreset] {
        &self.trip_presets
    }

    pub fn time_presets(&self) -> &[TimePreset] {
        &self.time_presets
    }

    /// Until a project has tags, every project offers all of them, so a new user needn't configure
    /// anything first.
    pub fn tags_configured(&self) -> bool {
        self.project_tags.values().any(|tags| !tags.is_empty())
    }

    /// Err, with the message to show, while config.toml failed to load.
    pub fn writable(&self) -> Result<(), String> {
        match &self.error {
            Some(e) => Err(format!("Fix config.toml first: {e}")),
            None => Ok(()),
        }
    }

    /// Returns whether the excluded tasks changed, which the review's findings depend on.
    pub(super) fn replace_settings(&mut self, settings: Settings) -> bool {
        let excluded_changed = settings.excluded_tasks != self.settings.excluded_tasks;
        self.settings = settings;
        excluded_changed
    }

    /// A load succeeded (or found no file): writes are allowed again.
    pub(super) fn apply(&mut self, project_tags: HashMap<i64, Vec<Tag>>, trips: Vec<TripPreset>, times: Vec<TimePreset>) {
        self.error = None;
        self.project_tags = project_tags;
        self.trip_presets = trips;
        self.time_presets = times;
    }

    pub(super) fn fail(&mut self, error: String) {
        self.error = Some(error);
    }

    #[cfg(test)]
    pub(super) fn set_project_tags(&mut self, project_tags: HashMap<i64, Vec<Tag>>) {
        self.project_tags = project_tags;
    }

    #[cfg(test)]
    pub(super) fn set_presets(&mut self, trips: Vec<TripPreset>, times: Vec<TimePreset>) {
        self.trip_presets = trips;
        self.time_presets = times;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::tag;

    #[test]
    fn only_non_empty_tag_lists_count_as_configured() {
        let mut c = ConfigState::default();
        assert!(!c.tags_configured());
        c.set_project_tags(HashMap::from([(1, Vec::new())]));
        assert!(!c.tags_configured());
        c.set_project_tags(HashMap::from([(1, Vec::new()), (2, vec![tag(30, "b")])]));
        assert!(c.tags_configured());
    }

    #[test]
    fn a_failed_load_blocks_writes_until_one_applies() {
        let mut c = ConfigState::default();
        assert_eq!(c.writable(), Ok(()));
        c.fail("bad".into());
        assert_eq!(c.writable(), Err("Fix config.toml first: bad".into()));
        c.apply(HashMap::new(), Vec::new(), Vec::new());
        assert_eq!(c.writable(), Ok(()));
    }

    #[test]
    fn replacing_settings_tells_whether_the_excluded_tasks_changed() {
        let mut c = ConfigState::default();
        assert!(!c.replace_settings(Settings { billable_goal: 0.5, ..Settings::default() }));
        assert!(c.replace_settings(Settings { excluded_tasks: Default::default(), ..Settings::default() }));
    }
}
