//! Everything that belongs to one logged-in session: the client, the user's data and what is
//! selected in it. Logging out replaces the whole `Session`, so nothing of the previous account
//! survives, and responses still underway for it are dropped (`epoch`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, NaiveDate};
use timechimp::{Client, CurrentUser, Mileage, Project, ProjectTask, Tag, Task, TimeEntry};

use crate::domain::report::Report;
use crate::domain::{self, week::RecentItem};
use crate::net;
use crate::store::config::{self, Config, TripPreset};

pub struct Session {
    /// Unique per session; `TimeChimpApp::run` drops results that arrive for an older one.
    pub epoch: u64,
    /// None until logged in.
    pub client: Option<Client>,
    /// None until the first load is in.
    pub me: Option<CurrentUser>,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub project_tasks: HashMap<i64, Vec<ProjectTask>>,
    pub tags: Vec<Tag>,

    /// Tags each project may use, from `config.toml` (project id → tags).
    pub project_tags: HashMap<i64, Vec<Tag>>,
    /// Billable share to aim for, from `config.toml` (fraction).
    pub billable_goal: f64,
    /// `[[trips]]` from `config.toml`: buttons on the Mileage tab that open a filled-in trip form.
    pub trip_presets: Vec<TripPreset>,

    /// The selected day.
    pub day: NaiveDate,
    /// Always the Monday of `day`'s week.
    pub week_start: NaiveDate,
    pub entries: Vec<TimeEntry>,
    pub selected_entry: Option<usize>,
    pub mileage: Vec<Mileage>,
    pub selected_mileage: Option<usize>,
    /// Bumped on every week load, so a slow response for a week we already left is dropped.
    pub week_generation: u64,
    /// Entries of the last few weeks plus every week loaded since, by id; feeds `recent`.
    history: HashMap<i64, TimeEntry>,
    /// What the time form's Recent picker offers, most recent first.
    pub recent: Vec<RecentItem>,

    pub report_month: NaiveDate,
    pub report: Report,
}

impl Session {
    pub fn new(client: Option<Client>, today: NaiveDate) -> Self {
        static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
        Self {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            client,
            me: None,
            projects: Vec::new(),
            tasks: Vec::new(),
            project_tasks: HashMap::new(),
            tags: Vec::new(),
            project_tags: HashMap::new(),
            billable_goal: config::DEFAULT_BILLABLE_GOAL,
            trip_presets: Vec::new(),
            day: today,
            week_start: domain::time::monday_of(today),
            entries: Vec::new(),
            selected_entry: None,
            mileage: Vec::new(),
            selected_mileage: None,
            week_generation: 0,
            history: HashMap::new(),
            recent: Vec::new(),
            report_month: domain::time::first_of_month(today),
            report: Report::default(),
        }
    }

    /// Takes in what a full reload fetched. `history` leaves out the loaded week, whose `entries`
    /// are added here (and shown through `apply_week`).
    pub fn apply_base(&mut self, base: net::Base, history: Option<Vec<TimeEntry>>, entries: &[TimeEntry]) {
        self.me = Some(base.me);
        self.projects = base.projects;
        self.tasks = base.tasks;
        self.project_tasks = base.project_tasks;
        self.tags = base.tags;
        if let Some(history) = history {
            self.history = history.into_iter().chain(entries.iter().cloned()).map(|e| (e.id, e)).collect();
            self.recent = domain::week::recent_items(self.history.values());
        }
    }

    /// Takes in `config.toml` (None: no usable file, so the defaults); returns a warning about
    /// names in it that match nothing.
    pub fn apply_config(&mut self, config: Option<Config>) -> Option<String> {
        let Some(config) = config else {
            self.project_tags.clear();
            self.trip_presets.clear();
            self.billable_goal = config::DEFAULT_BILLABLE_GOAL;
            return None;
        };
        self.billable_goal = config.billable_goal;
        let (resolved, warning) = config::resolve_project_tags(&config.projects, &self.projects, &self.tags);
        self.project_tags = resolved;
        let unknown: Vec<String> = config
            .trips
            .iter()
            .filter(|t| self.preset_project(t).is_none())
            .map(|t| format!("project \"{}\" (trip \"{}\")", t.project, t.name))
            .collect();
        self.trip_presets = config.trips;
        match (warning, unknown.is_empty()) {
            (w, true) => w,
            (None, false) => Some(format!("config.toml: unknown {}", unknown.join(", "))),
            (Some(w), false) => Some(format!("{w}, {}", unknown.join(", "))),
        }
    }

    /// Shows a loaded week; afterwards selects the item with `select_id`, if given.
    pub fn apply_week(&mut self, week: net::Week, select_id: Option<i64>) {
        // The week replaces what the history had for it, so saves and deletes show up in Recent.
        let week_end = self.week_start + Duration::days(7);
        self.history.retain(|_, e| !(self.week_start..week_end).contains(&e.date));
        self.history.extend(week.entries.iter().map(|e| (e.id, e.clone())));
        self.recent = domain::week::recent_items(self.history.values());

        self.selected_entry = keep_selection(self.selected_entry, week.entries.len());
        self.selected_mileage = keep_selection(self.selected_mileage, week.mileage.len());
        self.entries = week.entries;
        self.mileage = week.mileage;
        if let Some(id) = select_id {
            if let Some(i) = self.entries.iter().position(|e| e.id == id) {
                self.selected_entry = Some(i);
            }
            if let Some(i) = self.mileage.iter().position(|m| m.id == id) {
                self.selected_mileage = Some(i);
            }
        }
        self.fix_selection();
    }

    /// Selects `day`; true when that moved to another week, which then needs loading.
    pub fn select_day(&mut self, day: NaiveDate) -> bool {
        self.day = day;
        let week_start = domain::time::monday_of(day);
        let moved = week_start != self.week_start;
        self.week_start = week_start;
        self.fix_selection();
        moved
    }

    // ---- Selection ---------------------------------------------------------------------------

    /// Indices into `entries` of the rows shown on the Week tab (the selected day's).
    pub fn visible_entries(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.entries[i].date == self.day).collect()
    }

    /// Keeps the selection on a visible entry (the first one of the day if it isn't).
    pub fn fix_selection(&mut self) {
        let visible = self.visible_entries();
        if !self.selected_entry.is_some_and(|s| visible.contains(&s)) {
            self.selected_entry = visible.first().copied();
        }
    }

    /// Moves the entry selection by `delta` rows within the selected day.
    pub fn move_entry_selection(&mut self, delta: isize) {
        let visible = self.visible_entries();
        let Some(last) = visible.len().checked_sub(1) else { return };
        let pos = self.selected_entry.and_then(|s| visible.iter().position(|&i| i == s));
        let pos = pos.map_or(0, |p| p.saturating_add_signed(delta).min(last));
        self.selected_entry = Some(visible[pos]);
    }

    /// Moves the trip selection by `delta` rows.
    pub fn move_mileage_selection(&mut self, delta: isize) {
        let len = self.mileage.len();
        if len > 0 {
            self.selected_mileage = Some(self.selected_mileage.map_or(0, |i| i.saturating_add_signed(delta).min(len - 1)));
        }
    }

    pub fn selected_entry(&self) -> Option<&TimeEntry> {
        self.entries.get(self.selected_entry?)
    }

    pub fn selected_mileage(&self) -> Option<&Mileage> {
        self.mileage.get(self.selected_mileage?)
    }

    // ---- Form data ---------------------------------------------------------------------------

    /// The active projects, plus `current` (id, name, customer: the project of the entry or trip
    /// at hand) when it isn't among them.
    pub fn project_options(&self, current: Option<(i64, &str, &str)>) -> Vec<(i64, String)> {
        let mut options: Vec<(i64, String)> = self
            .projects
            .iter()
            .filter(|p| p.active)
            .map(|p| (p.id, domain::project_label(&p.name, &p.customer_name)))
            .collect();
        if let Some((id, name, customer)) = current
            && !options.iter().any(|(o, _)| *o == id)
        {
            options.push((id, domain::project_label(name, customer)));
        }
        options
    }

    /// The active tasks of `project_id`, plus `entry`'s task when it is on that project but not
    /// among them.
    pub fn task_options(&self, project_id: Option<i64>, entry: Option<&TimeEntry>) -> Vec<(i64, String)> {
        let mut tasks: Vec<(i64, String)> = project_id
            .and_then(|id| self.project_tasks.get(&id))
            .map(|ts| ts.iter().filter(|t| t.active).map(|t| (t.id, t.name.clone())).collect())
            .unwrap_or_default();
        if let Some(e) = entry.filter(|e| Some(e.project_id) == project_id) {
            if !tasks.iter().any(|(id, _)| *id == e.project_task_id) {
                tasks.push((e.project_task_id, e.task_name.clone()));
            }
        }
        tasks
    }

    /// The tags in `tag_ids` that no project configures; the form can't show them, so they're kept.
    pub fn unconfigured_tags(&self, tag_ids: &[i64]) -> Vec<i64> {
        tag_ids.iter().copied().filter(|id| !self.project_tags.values().flatten().any(|t| t.id == *id)).collect()
    }

    /// The project a preset names (case-insensitive), among the user's projects.
    pub fn preset_project(&self, preset: &TripPreset) -> Option<&Project> {
        self.projects.iter().find(|p| p.name.eq_ignore_ascii_case(preset.project.trim()))
    }

    /// End time of the last entry on `date`, so a new entry continues where the day left off.
    pub fn next_start_time(&self, date: NaiveDate) -> Option<String> {
        self.entries
            .iter()
            .filter(|e| e.date == date)
            .filter_map(|e| e.start_end.as_deref()?.split_once('-').map(|(_, end)| end.to_owned()))
            .max()
    }

    /// The timezone for an entry's start/end: `own` (an edited entry's), else the account's, else
    /// what this week's entries were registered in.
    pub fn timezone(&self, own: Option<&str>) -> chrono_tz::Tz {
        let account = self.me.as_ref().and_then(|m| m.timezone.as_deref());
        let entries = self.entries.iter().map(|e| e.timezone.as_deref());
        domain::time::timezone([own, account].into_iter().chain(entries))
    }

    /// The user's saved addresses, then the ones on this week's trips.
    pub fn saved_addresses(&self) -> Vec<String> {
        let mut addresses: Vec<String> = self
            .me
            .as_ref()
            .and_then(|m| m.extra.get("mileageAddresses"))
            .and_then(domain::json_strings)
            .unwrap_or_default();
        for m in &self.mileage {
            for a in [&m.from_address, &m.to_address].into_iter().flatten() {
                if !addresses.contains(a) {
                    addresses.push(a.clone());
                }
            }
        }
        addresses
    }

    // ---- Week figures ------------------------------------------------------------------------

    /// Hours registered per day of the current week, Monday first.
    pub fn hours_per_day(&self) -> [f64; 7] {
        domain::week::hours_per_day(&self.entries, self.week_start)
    }

    /// Contracted hours for a day of the week (0 = Monday).
    pub fn target_for(&self, day: usize) -> f64 {
        self.me.as_ref().and_then(|m| m.user_availability.as_ref()).and_then(|a| a.for_day(day)).unwrap_or(0.0)
    }

    pub fn contract_hours(&self) -> f64 {
        self.me.as_ref().and_then(|m| m.contract_hours).unwrap_or(0.0)
    }
}

fn keep_selection(selected: Option<usize>, len: usize) -> Option<usize> {
    match (len, selected) {
        (0, _) => None,
        (n, Some(i)) => Some(i.min(n - 1)),
        (_, None) => Some(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, day: u32, start_end: &str) -> TimeEntry {
        serde_json::from_value(serde_json::json!({
            "id": id, "timeRowId": id, "userId": 1, "userDisplayName": "U",
            "customerId": 2, "customerName": "C", "projectId": 3, "projectName": "P",
            "projectTaskId": 4, "taskId": 5, "taskName": "T",
            "date": format!("2026-11-{day:02}T00:00:00"), "hours": 1.0, "notes": null,
            "startEnd": start_end, "pause": 0.0, "billable": true,
            "status": 0, "statusIntern": 0, "statusExtern": 0,
            "modified": "2026-11-01T00:00:00", "timezone": null,
        }))
        .unwrap()
    }

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }

    #[test]
    fn sessions_are_distinct() {
        assert_ne!(Session::new(None, date(4)).epoch, Session::new(None, date(4)).epoch);
    }

    #[test]
    fn week_and_selection() {
        let mut s = Session::new(None, date(4)); // a Wednesday
        assert_eq!(s.week_start, date(2));
        let week = |entries| net::Week { entries, mileage: Vec::new() };
        s.apply_week(week(vec![entry(1, 3, "09:00-10:00"), entry(2, 4, "09:00-10:00"), entry(3, 4, "10:00-11:30")]), None);
        assert_eq!(s.visible_entries(), [1, 2]);
        assert_eq!(s.selected_entry().map(|e| e.id), Some(2), "the first entry of the selected day");
        s.move_entry_selection(5);
        assert_eq!(s.selected_entry().map(|e| e.id), Some(3), "stays within the day");
        assert_eq!(s.next_start_time(date(4)).as_deref(), Some("11:30"));
        assert_eq!(s.recent.len(), 1, "same project, task and notes");

        // A reload selecting a saved entry.
        s.apply_week(week(vec![entry(2, 4, "09:00-10:00"), entry(4, 4, "12:00-13:00")]), Some(4));
        assert_eq!(s.selected_entry().map(|e| e.id), Some(4));

        assert!(!s.select_day(date(8)), "Sunday is still this week");
        assert_eq!(s.selected_entry, None);
        assert!(s.select_day(date(9)));
        assert_eq!(s.week_start, date(9));
    }

    #[test]
    fn config_warnings() {
        let mut s = Session::new(None, date(4));
        let config = Config {
            billable_goal: 0.8,
            projects: HashMap::from([("Nope".to_owned(), Vec::new())]),
            trips: vec![TripPreset {
                name: "Trip".into(),
                project: "Gone".into(),
                from: "A".into(),
                to: "B".into(),
                distance: None,
                return_trip: false,
                mileage_type: None,
                billable: false,
                notes: None,
            }],
        };
        let warning = s.apply_config(Some(config)).unwrap();
        assert!(warning.contains("project \"Nope\"") && warning.contains("trip \"Trip\""), "{warning}");
        assert_eq!((s.billable_goal, s.trip_presets.len()), (0.8, 1));
        assert_eq!(s.apply_config(None), None);
        assert_eq!((s.billable_goal, s.trip_presets.len()), (config::DEFAULT_BILLABLE_GOAL, 0));
    }
}
