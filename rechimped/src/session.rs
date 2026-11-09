//! Logging out replaces the whole `Session`, so nothing of the previous account survives.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, NaiveDate};
use timechimp_api::{Client, CurrentUser, Mileage, Project, ProjectTask, Tag, TimeEntry};

use crate::domain::report::Report;
use crate::domain::{self, same_name, week::RecentItem};
use crate::store::config::{self, Config, Settings, TimePreset, TripPreset};

pub struct Session {
    /// `TimeChimpApp::run` drops results that arrive for an older epoch.
    pub epoch: u64,
    client: Option<Client>,
    base: Option<Base>,

    /// Read through `tags_for`, which offers all tags while none are configured.
    pub project_tags: HashMap<i64, Vec<Tag>>,
    pub settings: Settings,
    pub trip_presets: Vec<TripPreset>,
    pub time_presets: Vec<TimePreset>,

    day: NaiveDate,
    /// Always the Monday of `day`'s week.
    week_start: NaiveDate,
    /// Moving to another week empties `entries`/`travel` right away, so nothing of the week left
    /// shows under the new one's title.
    loaded_week: Option<NaiveDate>,
    entries: Vec<TimeEntry>,
    /// Always one of the selected day's entries, if it has any.
    selected_entry: Option<i64>,
    travel: Vec<Mileage>,
    selected_travel: Option<i64>,
    /// Bumped on every week load, so a slow response for a week we already left is dropped.
    week_generation: u64,
    history: HashMap<i64, TimeEntry>,
    pub recent: Vec<RecentItem>,

    /// Always the first of the month.
    report_month: NaiveDate,
    /// May be for another month than `report_month` until the new one is in.
    report: Report,
}

pub struct Base {
    pub me: CurrentUser,
    pub projects: Vec<Project>,
    pub project_tasks: HashMap<i64, Vec<ProjectTask>>,
    pub tags: Vec<Tag>,
}

pub struct Week {
    pub start: NaiveDate,
    pub entries: Vec<TimeEntry>,
    pub travel: Vec<Mileage>,
}

impl Session {
    pub fn new(client: Option<Client>, today: NaiveDate) -> Self {
        static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
        Self {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            client,
            base: None,
            project_tags: HashMap::new(),
            settings: Settings::default(),
            trip_presets: Vec::new(),
            time_presets: Vec::new(),
            day: today,
            week_start: domain::time::monday_of(today),
            loaded_week: None,
            entries: Vec::new(),
            selected_entry: None,
            travel: Vec::new(),
            selected_travel: None,
            week_generation: 0,
            history: HashMap::new(),
            recent: Vec::new(),
            report_month: domain::time::first_of_month(today),
            report: Report::default(),
        }
    }

    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    pub fn me(&self) -> Option<&CurrentUser> {
        self.base.as_ref().map(|b| &b.me)
    }

    pub fn projects(&self) -> &[Project] {
        self.base.as_ref().map_or(&[], |b| &b.projects)
    }

    pub fn project_tasks(&self, project_id: i64) -> &[ProjectTask] {
        self.base.as_ref().and_then(|b| b.project_tasks.get(&project_id)).map_or(&[], Vec::as_slice)
    }

    pub fn tags(&self) -> &[Tag] {
        self.base.as_ref().map_or(&[], |b| &b.tags)
    }

    pub fn entries(&self) -> &[TimeEntry] {
        &self.entries
    }

    pub fn travel(&self) -> &[Mileage] {
        &self.travel
    }

    /// `history` leaves out the loaded week, so its `entries` are added here.
    pub fn apply_base(&mut self, base: Base, history: Option<Vec<TimeEntry>>, entries: &[TimeEntry]) {
        self.base = Some(base);
        if let Some(history) = history {
            self.history = history.into_iter().chain(entries.iter().cloned()).map(|e| (e.id, e)).collect();
            self.recent = domain::week::recent_items(self.history.values());
        }
    }

    pub fn apply_config(&mut self, config: Option<Config>) -> Option<String> {
        let Some(config) = config else {
            self.project_tags.clear();
            self.trip_presets.clear();
            self.time_presets.clear();
            self.settings = Settings::default();
            return None;
        };
        self.settings = config.settings;
        let (resolved, warning) = config::resolve_project_tags(&config.projects, self.projects(), self.tags());
        self.project_tags = resolved;
        let mut unknown: Vec<String> = config
            .trips
            .iter()
            .filter(|t| self.project_named(&t.project).is_none())
            .map(|t| format!("project \"{}\" (trip \"{}\")", t.project, t.name))
            .collect();
        for preset in &config.time_entries {
            let what = time_preset_label(preset);
            match self.resolve_time_preset(preset) {
                Err(name) => unknown.push(format!("{name} (time entry \"{what}\")")),
                Ok(resolved) => unknown.extend(
                    resolved.unknown_tags.iter().map(|t| format!("tag \"{t}\" (time entry \"{what}\")")),
                ),
            }
        }
        self.trip_presets = config.trips;
        self.time_presets = config.time_entries;
        match (warning, unknown.is_empty()) {
            (w, true) => w,
            (None, false) => Some(format!("config.toml: unknown {}", unknown.join(", "))),
            (Some(w), false) => Some(format!("{w}, {}", unknown.join(", "))),
        }
    }

    pub fn begin_week_load(&mut self) -> (u64, NaiveDate) {
        self.week_generation += 1;
        (self.week_generation, self.week_start)
    }

    pub fn is_latest_week_load(&self, generation: u64) -> bool {
        generation == self.week_generation
    }

    pub fn week_loaded(&self) -> bool {
        self.loaded_week == Some(self.week_start)
    }

    pub fn apply_week(&mut self, week: Week, select_id: Option<i64>) {
        // The week replaces what the history had for it, so saves and deletes show up in Recent.
        let week_end = week.start + Duration::days(7);
        self.history.retain(|_, e| !(week.start..week_end).contains(&e.date));
        self.history.extend(week.entries.iter().map(|e| (e.id, e.clone())));
        self.recent = domain::week::recent_items(self.history.values());
        self.loaded_week = Some(week.start);

        let entry_ids = |entries: &[TimeEntry]| entries.iter().map(|e| e.id).collect::<Vec<_>>();
        let trip_ids = |trips: &[Mileage]| trips.iter().map(|m| m.id).collect::<Vec<_>>();
        self.selected_entry = reselect(self.selected_entry, select_id, &entry_ids(&self.entries), &entry_ids(&week.entries));
        self.selected_travel = reselect(self.selected_travel, select_id, &trip_ids(&self.travel), &trip_ids(&week.travel));
        self.entries = week.entries;
        self.travel = week.travel;
        self.fix_selection();
    }

    pub fn select_day(&mut self, day: NaiveDate) -> bool {
        self.day = day;
        let week_start = domain::time::monday_of(day);
        let moved = week_start != self.week_start;
        if moved {
            self.week_start = week_start;
            self.loaded_week = None;
            self.entries.clear();
            self.travel.clear();
            self.selected_travel = None;
        }
        self.fix_selection();
        moved
    }

    pub fn day(&self) -> NaiveDate {
        self.day
    }

    pub fn week_start(&self) -> NaiveDate {
        self.week_start
    }

    pub fn report_month(&self) -> NaiveDate {
        self.report_month
    }

    pub fn set_report_month(&mut self, day: NaiveDate) {
        self.report_month = domain::time::first_of_month(day);
    }

    pub fn report(&self) -> Option<&Report> {
        (self.report.month == Some(self.report_month)).then_some(&self.report)
    }

    pub fn apply_report(&mut self, report: Report) -> bool {
        let current = report.month == Some(self.report_month);
        if current {
            self.report = report;
        }
        current
    }

    pub fn invalidate_report(&mut self) {
        self.report = Report::default();
    }

    pub fn visible_entries(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.entries[i].date == self.day).collect()
    }

    pub fn fix_selection(&mut self) {
        let visible = self.visible_entries();
        if !visible.iter().any(|&i| Some(self.entries[i].id) == self.selected_entry) {
            self.selected_entry = visible.first().map(|&i| self.entries[i].id);
        }
    }

    pub fn move_entry_selection(&mut self, delta: isize) {
        let visible = self.visible_entries();
        let Some(last) = visible.len().checked_sub(1) else { return };
        let pos = visible.iter().position(|&i| Some(self.entries[i].id) == self.selected_entry);
        let pos = pos.map_or(0, |p| p.saturating_add_signed(delta).min(last));
        self.selected_entry = Some(self.entries[visible[pos]].id);
    }

    pub fn move_travel_selection(&mut self, delta: isize) {
        let Some(last) = self.travel.len().checked_sub(1) else { return };
        let pos = self.travel.iter().position(|m| Some(m.id) == self.selected_travel);
        self.selected_travel = Some(self.travel[pos.map_or(0, |p| p.saturating_add_signed(delta).min(last))].id);
    }

    pub fn select_entry(&mut self, id: i64) {
        if let Some(day) = self.entries.iter().find(|e| e.id == id).map(|e| e.date) {
            self.selected_entry = Some(id);
            self.day = day;
        }
    }

    pub fn select_travel(&mut self, id: i64) {
        if self.travel.iter().any(|m| m.id == id) {
            self.selected_travel = Some(id);
        }
    }

    pub fn selected_entry_id(&self) -> Option<i64> {
        self.selected_entry
    }

    pub fn selected_travel_id(&self) -> Option<i64> {
        self.selected_travel
    }

    pub fn selected_entry(&self) -> Option<&TimeEntry> {
        self.entries.iter().find(|e| Some(e.id) == self.selected_entry)
    }

    pub fn selected_travel(&self) -> Option<&Mileage> {
        self.travel.iter().find(|m| Some(m.id) == self.selected_travel)
    }

    pub fn is_hidden(&self, project_id: i64) -> bool {
        self.project(project_id)
            .is_some_and(|p| self.settings.hidden_projects.iter().any(|name| same_name(name, &p.name)))
    }

    /// `current` is the project of the entry or trip at hand, offered even if hidden or inactive.
    pub fn project_options(&self, current: Option<(i64, &str, &str)>) -> Vec<(i64, String)> {
        let mut options: Vec<(i64, String)> = self
            .projects()
            .iter()
            .filter(|p| p.active && !self.is_hidden(p.id))
            .map(|p| (p.id, domain::project_label(&p.name, &p.customer_name)))
            .collect();
        if let Some((id, name, customer)) = current
            && !options.iter().any(|(o, _)| *o == id)
        {
            options.push((id, domain::project_label(name, customer)));
        }
        options
    }

    pub fn task_options(&self, project_id: Option<i64>, entry: Option<&TimeEntry>) -> Vec<(i64, String)> {
        let mut tasks: Vec<(i64, String)> = project_id
            .map(|id| self.project_tasks(id).iter().filter(|t| t.active).map(|t| (t.id, t.name.clone())).collect())
            .unwrap_or_default();
        if let Some(e) = entry.filter(|e| Some(e.project_id) == project_id)
            && !tasks.iter().any(|(id, _)| *id == e.project_task_id) {
                tasks.push((e.project_task_id, e.task_name.clone()));
            }
        tasks
    }

    /// Until a project has tags, every project offers all of them, so a new user needn't configure
    /// anything first.
    pub fn tags_configured(&self) -> bool {
        self.project_tags.values().any(|tags| !tags.is_empty())
    }

    pub fn tags_for(&self, project_id: Option<i64>) -> Vec<&Tag> {
        let Some(project_id) = project_id else { return Vec::new() };
        if self.tags_configured() {
            self.project_tags.get(&project_id).map(|tags| tags.iter().collect()).unwrap_or_default()
        } else {
            self.tags().iter().filter(|t| !t.name.starts_with("z_")).collect()
        }
    }

    /// The time form has no checkbox for these, so it leaves them untouched.
    pub fn kept_tags(&self, project_id: Option<i64>, tag_ids: &[i64]) -> Vec<i64> {
        let offered = self.tags_for(project_id);
        tag_ids.iter().copied().filter(|id| !offered.iter().any(|t| t.id == *id)).collect()
    }

    pub fn project_named(&self, name: &str) -> Option<&Project> {
        self.projects().iter().find(|p| same_name(&p.name, name))
    }

    pub fn project(&self, id: i64) -> Option<&Project> {
        self.projects().iter().find(|p| p.id == id)
    }

    pub fn resolve_time_preset(&self, preset: &TimePreset) -> Result<ResolvedTimePreset, String> {
        let project = self.project_named(&preset.project).ok_or_else(|| format!("project \"{}\"", preset.project))?;
        let tasks = self.project_tasks(project.id);
        let project_task_id = match &preset.task {
            Some(name) => Some(
                tasks
                    .iter()
                    .find(|t| same_name(&t.name, name))
                    .ok_or_else(|| format!("task \"{name}\" of project \"{}\"", project.name))?
                    .id,
            ),
            None => None,
        };
        let (mut tag_ids, mut unknown_tags) = (Vec::new(), Vec::new());
        for name in &preset.tags {
            match self.tags().iter().find(|t| same_name(&t.name, name)) {
                Some(tag) => tag_ids.push(tag.id),
                None => unknown_tags.push(name.clone()),
            }
        }
        Ok(ResolvedTimePreset { project_id: project.id, project_task_id, tag_ids, unknown_tags })
    }

    pub fn next_start_time(&self, date: NaiveDate) -> Option<String> {
        self.entries
            .iter()
            .filter(|e| e.date == date)
            .filter_map(|e| e.start_end.as_deref()?.split_once('-').map(|(_, end)| end.to_owned()))
            .max()
    }

    pub fn timezone(&self, own: Option<&str>) -> chrono_tz::Tz {
        let account = self.me().and_then(|m| m.timezone.as_deref());
        let entries = self.entries.iter().map(|e| e.timezone.as_deref());
        domain::time::timezone([own, account].into_iter().chain(entries))
    }

    pub fn saved_addresses(&self) -> Vec<String> {
        let mut addresses: Vec<String> = self.me().map(|m| m.mileage_addresses.clone()).unwrap_or_default();
        for m in &self.travel {
            for a in [&m.from_address, &m.to_address].into_iter().flatten() {
                if !addresses.contains(a) {
                    addresses.push(a.clone());
                }
            }
        }
        addresses
    }

    pub fn hours_per_day(&self) -> [f64; 7] {
        domain::week::hours_per_day(&self.entries, self.week_start)
    }

    pub fn target_for(&self, day: usize) -> f64 {
        self.me().and_then(|m| m.user_availability.as_ref()).and_then(|a| a.for_day(day)).unwrap_or(0.0)
    }

    pub fn shown_days(&self, today: NaiveDate) -> Vec<usize> {
        let work_days = self.settings.work_days;
        (0..7).filter(|&i| domain::week::shows_day(self.week_start + Duration::days(i as i64), work_days, today, self.day)).collect()
    }

    pub fn contract_hours(&self) -> f64 {
        self.me().and_then(|m| m.contract_hours).unwrap_or(0.0)
    }
}

#[derive(Debug, PartialEq)]
pub struct ResolvedTimePreset {
    pub project_id: i64,
    /// None: the form picks the project's first task.
    pub project_task_id: Option<i64>,
    pub tag_ids: Vec<i64>,
    pub unknown_tags: Vec<String>,
}

pub fn time_preset_label(preset: &TimePreset) -> &str {
    [&preset.notes, &preset.task]
        .into_iter()
        .flatten()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or(&preset.project)
}

/// If the selected row was deleted, the row now at its position takes over.
fn reselect(selected: Option<i64>, wanted: Option<i64>, old: &[i64], new: &[i64]) -> Option<i64> {
    if let Some(id) = [wanted, selected].into_iter().flatten().find(|id| new.contains(id)) {
        return Some(id);
    }
    let position = selected.and_then(|id| old.iter().position(|&o| o == id)).unwrap_or(0);
    new.get(position.min(new.len().saturating_sub(1))).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn sessions_are_distinct() {
        assert_ne!(Session::new(None, date(4)).epoch, Session::new(None, date(4)).epoch);
    }

    #[test]
    fn week_and_selection() {
        let mut s = Session::new(None, date(4));
        assert_eq!(s.week_start, date(2));
        let week = |entries| Week { start: date(2), entries, travel: Vec::new() };
        s.apply_week(week(vec![entry(1, 3, "09:00-10:00"), entry(2, 4, "09:00-10:00"), entry(3, 4, "10:00-11:30")]), None);
        assert_eq!(s.visible_entries(), [1, 2]);
        assert_eq!(s.selected_entry().map(|e| e.id), Some(2), "the first entry of the selected day");
        s.move_entry_selection(5);
        assert_eq!(s.selected_entry().map(|e| e.id), Some(3), "stays within the day");
        assert_eq!(s.next_start_time(date(4)).as_deref(), Some("11:30"));
        assert_eq!(s.recent.len(), 1, "same project, task and notes");

        s.apply_week(week(vec![entry(2, 4, "09:00-10:00"), entry(4, 4, "12:00-13:00")]), Some(4));
        assert_eq!(s.selected_entry().map(|e| e.id), Some(4));

        assert!(!s.select_day(date(8)), "Sunday is still this week");
        assert_eq!(s.selected_entry_id(), None);
        assert!(s.week_loaded());
        assert!(s.select_day(date(9)));
        assert_eq!(s.week_start, date(9));
        assert!(!s.week_loaded() && s.entries.is_empty(), "the week left doesn't show under the new one");

        let (first, _) = s.begin_week_load();
        let (second, monday) = s.begin_week_load();
        assert_eq!(monday, date(9));
        assert!(!s.is_latest_week_load(first) && s.is_latest_week_load(second));

        // Its entries were cleared on leaving, and no load has come back since.
        assert!(s.select_day(date(4)));
        assert!(!s.week_loaded() && s.entries.is_empty());
    }

    #[test]
    fn reload_keeps_the_selected_item() {
        assert_eq!(reselect(Some(2), None, &[1, 2, 3], &[3, 1, 2]), Some(2), "by id, not position");
        assert_eq!(reselect(Some(2), Some(9), &[1, 2], &[1, 2, 9]), Some(9), "the saved item");
        assert_eq!(reselect(Some(2), None, &[1, 2, 3], &[1, 3]), Some(3), "deleted: the row at its place");
        assert_eq!(reselect(Some(3), None, &[1, 2, 3], &[1, 2]), Some(2), "deleted last: the new last");
        assert_eq!(reselect(None, None, &[], &[5, 6]), Some(5));
        assert_eq!(reselect(Some(1), None, &[1], &[]), None);
    }

    #[test]
    fn report_follows_the_month() {
        let mut s = Session::new(None, date(4));
        assert!(s.report().is_none());
        let report = |month| Report { month: Some(month), ..Report::default() };
        assert!(!s.apply_report(report(date(1) - Duration::days(1))), "October's report arrives too late");
        assert!(s.apply_report(report(date(1))));
        assert!(s.report().is_some());
        s.set_report_month(date(1) - Duration::days(1));
        assert!(s.report().is_none(), "another month");
        s.set_report_month(date(30));
        assert!(s.report().is_some());
        s.invalidate_report();
        assert!(s.report().is_none());
    }

    #[test]
    fn kept_tags() {
        let mut s = Session::new(None, date(4));
        s.project_tags = HashMap::from([(1, vec![tag(10, "a")]), (2, vec![tag(20, "b")])]);
        // A tag only another project configures has no checkbox here either, so it is kept.
        assert_eq!(s.kept_tags(Some(2), &[10, 20, 30]), [10, 30]);
        assert_eq!(s.kept_tags(Some(1), &[10, 20, 30]), [20, 30]);
        assert_eq!(s.kept_tags(None, &[10, 20]), [10, 20]);
    }

    #[test]
    fn all_tags_until_some_are_configured() {
        let mut s = Session::new(None, date(4));
        let tags = vec![tag(10, "a"), tag(20, "z_old"), tag(30, "b")];
        s.apply_base(base(vec![project(1, "P")], HashMap::new(), tags), None, &[]);
        let ids = |s: &Session, p| s.tags_for(p).iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(&s, Some(1)), [10, 30], "nothing configured: all but z_");
        assert_eq!(ids(&s, None), Vec::<i64>::new(), "no project, no tags");
        assert_eq!(s.kept_tags(Some(1), &[10, 20]), [20]);

        s.project_tags = HashMap::from([(1, Vec::new())]);
        assert_eq!(ids(&s, Some(1)), [10, 30], "only empty lists count as nothing configured");
        s.project_tags = HashMap::from([(1, Vec::new()), (2, vec![tag(30, "b")])]);
        assert_eq!(ids(&s, Some(1)), Vec::<i64>::new(), "once one project has tags, the lists hold");
        assert_eq!(ids(&s, Some(2)), [30]);
    }

    #[test]
    fn hidden_projects_are_not_offered() {
        let mut s = Session::new(None, date(4));
        s.apply_base(base(vec![project(1, "Keep"), project(2, "Old")], HashMap::new(), Vec::new()), None, &[]);
        s.settings.hidden_projects = vec!["old".into()];
        let ids = |options: Vec<(i64, String)>| options.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert!(s.is_hidden(2) && !s.is_hidden(1));
        assert_eq!(ids(s.project_options(None)), [1]);
        assert_eq!(ids(s.project_options(Some((2, "Old", "Us")))), [1, 2], "still offered when editing its entry");
    }

    #[test]
    fn config_warnings() {
        let mut s = Session::new(None, date(4));
        let config = Config {
            settings: Settings { billable_goal: 0.7, ..Settings::default() },
            projects: HashMap::from([("Nope".to_owned(), Vec::new())]),
            trips: vec![TripPreset {
                name: "Trip".into(),
                project: "Gone".into(),
                from: "A".into(),
                to: "B".into(),
                distance: None,
                return_trip: false,
                travel_type: None,
                billable: false,
                notes: None,
            }],
            time_entries: vec![TimePreset { project: "Gone".into(), task: None, notes: Some("Standup".into()), tags: Vec::new() }],
        };
        let warning = s.apply_config(Some(config)).unwrap();
        assert!(warning.contains("project \"Nope\"") && warning.contains("trip \"Trip\""), "{warning}");
        assert!(warning.contains("(time entry \"Standup\")"), "{warning}");
        assert_eq!((s.settings.billable_goal, s.trip_presets.len(), s.time_presets.len()), (0.7, 1, 1));
        assert_eq!(s.apply_config(None), None);
        assert_eq!((s.settings, s.trip_presets.len(), s.time_presets.len()), (Settings::default(), 0, 0));
    }

    #[test]
    fn time_presets() {
        let mut s = Session::new(None, date(4));
        s.apply_base(base(vec![project(3, "Intern")], HashMap::from([(3, vec![task(4, 3, "Meeting")])]), vec![tag(7, "Daily")]), None, &[]);
        let preset = |task: Option<&str>, tags: &[&str]| TimePreset {
            project: "intern".into(),
            task: task.map(Into::into),
            notes: None,
            tags: tags.iter().map(|t| t.to_string()).collect(),
        };
        assert_eq!(
            s.resolve_time_preset(&preset(Some("meeting"), &["daily", "gone"])),
            Ok(ResolvedTimePreset { project_id: 3, project_task_id: Some(4), tag_ids: vec![7], unknown_tags: vec!["gone".into()] })
        );
        assert_eq!(s.resolve_time_preset(&preset(None, &[])).unwrap().project_task_id, None);
        assert!(s.resolve_time_preset(&preset(Some("Nap"), &[])).unwrap_err().starts_with("task \"Nap\""));

        assert_eq!(time_preset_label(&preset(Some("Meeting"), &[])), "Meeting");
        assert_eq!(time_preset_label(&TimePreset { notes: Some(" Standup ".into()), ..preset(None, &[]) }), "Standup");
        assert_eq!(time_preset_label(&preset(None, &[])), "intern");
    }
}
