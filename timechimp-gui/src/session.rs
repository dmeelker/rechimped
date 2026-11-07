//! Everything that belongs to one logged-in session: the client, the user's data and what is
//! selected in it. Logging out replaces the whole `Session`, so nothing of the previous account
//! survives, and responses still underway for it are dropped (`epoch`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, NaiveDate};
use timechimp::{Client, CurrentUser, Mileage, Project, ProjectTask, Tag, Task, TimeEntry};

use crate::domain::report::Report;
use crate::domain::{self, week::RecentItem};
use crate::store::config::{self, Config, TimePreset, TripPreset};

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
    /// `[[trips]]` from `config.toml`: the "+ Add trip" dropdown, each opening a filled-in trip form.
    pub trip_presets: Vec<TripPreset>,
    /// `[[time_entries]]` from `config.toml`: the "+ Add time" dropdown, each opening a filled-in
    /// time form.
    pub time_presets: Vec<TimePreset>,

    /// The selected day.
    day: NaiveDate,
    /// Always the Monday of `day`'s week.
    week_start: NaiveDate,
    /// The week `entries` and `mileage` belong to; None until it is loaded. Moving to another week
    /// empties them right away, so nothing of the week left shows under the new one's title.
    loaded_week: Option<NaiveDate>,
    pub entries: Vec<TimeEntry>,
    selected_entry: Option<usize>,
    pub mileage: Vec<Mileage>,
    selected_mileage: Option<usize>,
    /// Bumped on every week load, so a slow response for a week we already left is dropped.
    week_generation: u64,
    /// Entries of the last few weeks plus every week loaded since, by id; feeds `recent`.
    history: HashMap<i64, TimeEntry>,
    /// What the time form's Recent picker offers, most recent first.
    pub recent: Vec<RecentItem>,

    /// The month the Report tab shows (its first day).
    report_month: NaiveDate,
    /// The report last loaded; for another month than `report_month` (or none) until it's in.
    report: Report,
}

/// Everything that only changes on a full reload (`net::load_base`).
pub struct Base {
    pub me: CurrentUser,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub project_tasks: HashMap<i64, Vec<ProjectTask>>,
    pub tags: Vec<Tag>,
}

/// One week's entries and trips (`net::load_week`).
pub struct Week {
    /// The Monday.
    pub start: NaiveDate,
    pub entries: Vec<TimeEntry>,
    pub mileage: Vec<Mileage>,
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
            time_presets: Vec::new(),
            day: today,
            week_start: domain::time::monday_of(today),
            loaded_week: None,
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
    pub fn apply_base(&mut self, base: Base, history: Option<Vec<TimeEntry>>, entries: &[TimeEntry]) {
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
            self.time_presets.clear();
            self.billable_goal = config::DEFAULT_BILLABLE_GOAL;
            return None;
        };
        self.billable_goal = config.billable_goal;
        let (resolved, warning) = config::resolve_project_tags(&config.projects, &self.projects, &self.tags);
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

    /// Starts loading the current week: returns the generation `apply_week` will be called with
    /// and the Monday to load.
    pub fn begin_week_load(&mut self) -> (u64, NaiveDate) {
        self.week_generation += 1;
        (self.week_generation, self.week_start)
    }

    /// Whether `generation` (from `begin_week_load`) is the latest week load, so its result is
    /// still wanted.
    pub fn is_latest_week_load(&self, generation: u64) -> bool {
        generation == self.week_generation
    }

    /// Whether `entries` and `mileage` are those of the selected week (false while it loads or
    /// after loading it failed).
    pub fn week_loaded(&self) -> bool {
        self.loaded_week == Some(self.week_start)
    }

    /// Shows a loaded week; afterwards selects the item with `select_id`, if given.
    pub fn apply_week(&mut self, week: Week, select_id: Option<i64>) {
        // The week replaces what the history had for it, so saves and deletes show up in Recent.
        let week_end = week.start + Duration::days(7);
        self.history.retain(|_, e| !(week.start..week_end).contains(&e.date));
        self.history.extend(week.entries.iter().map(|e| (e.id, e.clone())));
        self.recent = domain::week::recent_items(self.history.values());
        self.loaded_week = Some(week.start);

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
        if moved {
            self.week_start = week_start;
            self.entries.clear();
            self.mileage.clear();
            self.selected_mileage = None;
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

    // ---- Report ------------------------------------------------------------------------------

    pub fn report_month(&self) -> NaiveDate {
        self.report_month
    }

    /// Shows the month of `day` on the Report tab (which then needs loading).
    pub fn set_report_month(&mut self, day: NaiveDate) {
        self.report_month = domain::time::first_of_month(day);
    }

    /// The report, if it is loaded for the month shown.
    pub fn report(&self) -> Option<&Report> {
        (self.report.month == Some(self.report_month)).then_some(&self.report)
    }

    /// Takes in a loaded report, unless it is for another month than the one shown now (the
    /// user moved on meanwhile).
    pub fn apply_report(&mut self, report: Report) -> bool {
        let current = report.month == Some(self.report_month);
        if current {
            self.report = report;
        }
        current
    }

    /// Forgets the report, after a change that affects it (or before a reload).
    pub fn invalidate_report(&mut self) {
        self.report = Report::default();
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

    /// Selects entry `index` (and its day, in this week).
    pub fn select_entry(&mut self, index: usize) {
        if let Some(day) = self.entries.get(index).map(|e| e.date) {
            self.selected_entry = Some(index);
            self.day = day;
        }
    }

    pub fn select_mileage(&mut self, index: usize) {
        if index < self.mileage.len() {
            self.selected_mileage = Some(index);
        }
    }

    pub fn selected_entry_index(&self) -> Option<usize> {
        self.selected_entry
    }

    pub fn selected_mileage_index(&self) -> Option<usize> {
        self.selected_mileage
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
        if let Some(e) = entry.filter(|e| Some(e.project_id) == project_id)
            && !tasks.iter().any(|(id, _)| *id == e.project_task_id) {
                tasks.push((e.project_task_id, e.task_name.clone()));
            }
        tasks
    }

    /// The tags in `tag_ids` that aren't configured for `project_id`: the time form has no checkbox
    /// for them, so it keeps them as they are.
    pub fn kept_tags(&self, project_id: Option<i64>, tag_ids: &[i64]) -> Vec<i64> {
        let configured = project_id.and_then(|id| self.project_tags.get(&id)).map(Vec::as_slice).unwrap_or_default();
        tag_ids.iter().copied().filter(|id| !configured.iter().any(|t| t.id == *id)).collect()
    }

    /// The project a preset names (case-insensitive), among the user's projects.
    pub fn project_named(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The ids a time preset's names stand for; Err names the project or task that matches nothing.
    pub fn resolve_time_preset(&self, preset: &TimePreset) -> Result<ResolvedTimePreset, String> {
        let project = self.project_named(&preset.project).ok_or_else(|| format!("project \"{}\"", preset.project))?;
        let tasks = self.project_tasks.get(&project.id).map(Vec::as_slice).unwrap_or_default();
        let project_task_id = match &preset.task {
            Some(name) => Some(
                tasks
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(name.trim()))
                    .ok_or_else(|| format!("task \"{name}\" of project \"{}\"", project.name))?
                    .id,
            ),
            None => None,
        };
        let (mut tag_ids, mut unknown_tags) = (Vec::new(), Vec::new());
        for name in &preset.tags {
            match self.tags.iter().find(|t| t.name.eq_ignore_ascii_case(name.trim())) {
                Some(tag) => tag_ids.push(tag.id),
                None => unknown_tags.push(name.clone()),
            }
        }
        Ok(ResolvedTimePreset { project_id: project.id, project_task_id, tag_ids, unknown_tags })
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
        let mut addresses: Vec<String> = self.me.as_ref().map(|m| m.mileage_addresses.clone()).unwrap_or_default();
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

/// A time preset's names as ids of the user's data.
#[derive(Debug, PartialEq)]
pub struct ResolvedTimePreset {
    pub project_id: i64,
    /// None: the preset names no task, so the form picks the project's first.
    pub project_task_id: Option<i64>,
    pub tag_ids: Vec<i64>,
    /// Tag names that match no tag; left out.
    pub unknown_tags: Vec<String>,
}

/// How a time preset is shown: its notes, else its task, else its project.
pub fn time_preset_label(preset: &TimePreset) -> &str {
    [&preset.notes, &preset.task]
        .into_iter()
        .flatten()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or(&preset.project)
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
        let week = |entries| Week { start: date(2), entries, mileage: Vec::new() };
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
        assert!(s.week_loaded());
        assert!(s.select_day(date(9)));
        assert_eq!(s.week_start, date(9));
        assert!(!s.week_loaded() && s.entries.is_empty(), "the week left doesn't show under the new one");

        // A load that was overtaken by a newer one is not wanted any more.
        let (first, _) = s.begin_week_load();
        let (second, monday) = s.begin_week_load();
        assert_eq!(monday, date(9));
        assert!(!s.is_latest_week_load(first) && s.is_latest_week_load(second));
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
        let tag = |id: i64| -> Tag {
            serde_json::from_value(serde_json::json!({ "id": id, "companyId": 1, "name": format!("t{id}"), "active": true, "type": 1 }))
                .unwrap()
        };
        let mut s = Session::new(None, date(4));
        s.project_tags = HashMap::from([(1, vec![tag(10)]), (2, vec![tag(20)])]);
        // A tag only another project configures has no checkbox here either, so it is kept.
        assert_eq!(s.kept_tags(Some(2), &[10, 20, 30]), [10, 30]);
        assert_eq!(s.kept_tags(Some(1), &[10, 20, 30]), [20, 30]);
        assert_eq!(s.kept_tags(None, &[10, 20]), [10, 20]);
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
            time_entries: vec![TimePreset { project: "Gone".into(), task: None, notes: Some("Standup".into()), tags: Vec::new() }],
        };
        let warning = s.apply_config(Some(config)).unwrap();
        assert!(warning.contains("project \"Nope\"") && warning.contains("trip \"Trip\""), "{warning}");
        assert!(warning.contains("(time entry \"Standup\")"), "{warning}");
        assert_eq!((s.billable_goal, s.trip_presets.len(), s.time_presets.len()), (0.8, 1, 1));
        assert_eq!(s.apply_config(None), None);
        assert_eq!((s.billable_goal, s.trip_presets.len(), s.time_presets.len()), (config::DEFAULT_BILLABLE_GOAL, 0, 0));
    }

    #[test]
    fn time_presets() {
        let mut s = Session::new(None, date(4));
        s.projects = vec![
            serde_json::from_value(serde_json::json!({
                "id": 3, "name": "Intern", "customerId": 2, "customerName": "Us",
                "intern": true, "unspecified": false, "active": true,
            }))
            .unwrap(),
        ];
        s.project_tasks = HashMap::from([(
            3,
            vec![
                serde_json::from_value(serde_json::json!({
                    "id": 4, "name": "Meeting", "billable": false, "active": true, "projectId": 3, "taskId": 5,
                }))
                .unwrap(),
            ],
        )]);
        s.tags = vec![serde_json::from_value(serde_json::json!({ "id": 7, "companyId": 1, "name": "Daily", "active": true, "type": 1 })).unwrap()];
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
