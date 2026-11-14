
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, Months, NaiveDate};
use timechimp_api::{CurrentUser, Mileage, Project, ProjectTask, Tag, TimeEntry, TimeReportRow};

use crate::backend::Api;
use crate::domain::report::{Report, Summary};
use crate::domain::{self, review, same_name, week::RecentItem};
use crate::store::config::{self, Config, Settings, TimePreset, TripPreset};

pub struct Session {
    /// `TimeChimpApp::run` drops results that arrive for an older epoch.
    epoch: u64,
    client: Option<Api>,
    base: Option<Base>,

    /// Read through `tags_for`, which offers all tags while none are configured.
    project_tags: HashMap<i64, Vec<Tag>>,
    settings: Settings,
    trip_presets: Vec<TripPreset>,
    time_presets: Vec<TimePreset>,
    /// Why config.toml failed to load. While set nothing writes to it: the defaults shown
    /// meanwhile would be saved over what the file holds.
    config_error: Option<String>,

    day: NaiveDate,
    /// Always the Monday of `day`'s week.
    week_start: NaiveDate,
    /// Moving to another week empties `entries`/`travel` right away, and `apply_week` takes only
    /// the week shown, so they never hold another week than `week_start`.
    loaded_week: Option<NaiveDate>,
    entries: Vec<TimeEntry>,
    /// Always one of the selected day's entries, if it has any.
    selected_entry: Option<i64>,
    travel: Vec<Mileage>,
    selected_travel: Option<i64>,
    /// Bumped on every week load and on moving to another week, so a slow response for a week
    /// we already left is dropped, even if no load for the new week could start yet.
    week_generation: u64,
    history: HashMap<i64, TimeEntry>,
    recent: Vec<RecentItem>,

    /// Always the first of the month.
    report_month: NaiveDate,
    /// May be for another month than `report_month` until the new one is in.
    report: Report,
    /// `report.summary()`, worked out once when the report changes rather than every frame.
    report_summary: Summary,

    review: Review,
}

/// The Review tab's month, kept apart from the report's.
struct Review {
    /// Always the first of the month.
    month: NaiveDate,
    /// The month `rows` are for; None while they are stale, as after `invalidate_review`.
    loaded: Option<NaiveDate>,
    /// Kept while stale, so a reload can keep the selection on the row nearest to it.
    rows: Vec<TimeReportRow>,
    selected: Option<i64>,
    generation: u64,
    /// Worked out from `rows` and the excluded tasks.
    analysis: review::Analysis,
    /// Bumped whenever `analysis` is redone, so the list knows to measure its lines again.
    revision: u64,
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
    pub fn new(client: Option<Api>, today: NaiveDate) -> Self {
        static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
        Self {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            client,
            base: None,
            project_tags: HashMap::new(),
            settings: Settings::default(),
            trip_presets: Vec::new(),
            time_presets: Vec::new(),
            config_error: None,
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
            report_summary: Report::default().summary(),
            // The month usually handed in.
            review: Review {
                month: domain::time::first_of_month(today) - Months::new(1),
                loaded: None,
                rows: Vec::new(),
                selected: None,
                generation: 0,
                analysis: review::Analysis::default(),
                revision: 0,
            },
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The tags configured per project id; offering them goes through `tags_for`.
    pub fn project_tags(&self) -> &HashMap<i64, Vec<Tag>> {
        &self.project_tags
    }

    pub fn trip_presets(&self) -> &[TripPreset] {
        &self.trip_presets
    }

    pub fn time_presets(&self) -> &[TimePreset] {
        &self.time_presets
    }

    pub fn recent(&self) -> &[RecentItem] {
        &self.recent
    }

    /// Stands in for loading `[projects]` from config.toml.
    #[cfg(test)]
    pub fn set_project_tags(&mut self, project_tags: HashMap<i64, Vec<Tag>>) {
        self.project_tags = project_tags;
    }

    /// Stands in for loading the presets from config.toml.
    #[cfg(test)]
    pub fn set_presets(&mut self, trips: Vec<TripPreset>, times: Vec<TimePreset>) {
        self.trip_presets = trips;
        self.time_presets = times;
    }

    pub fn client(&self) -> Option<&Api> {
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

    /// Falls back to the defaults, like a missing file, but blocks writes until a load succeeds.
    pub fn apply_config_error(&mut self, error: String) {
        self.apply_config(None);
        self.config_error = Some(error);
    }

    /// Err, with the message to show, while config.toml failed to load.
    pub fn config_writable(&self) -> Result<(), String> {
        match &self.config_error {
            Some(e) => Err(format!("Fix config.toml first: {e}")),
            None => Ok(()),
        }
    }

    pub fn apply_config(&mut self, config: Option<Config>) -> Option<String> {
        self.config_error = None;
        let Some(config) = config else {
            self.project_tags.clear();
            self.trip_presets.clear();
            self.time_presets.clear();
            self.set_settings(Settings::default());
            return None;
        };
        self.set_settings(config.settings);
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

    pub fn week_loaded(&self) -> bool {
        self.loaded_week == Some(self.week_start)
    }

    /// Drops what an older load or one for another week brings back.
    pub fn apply_week(&mut self, generation: u64, week: Week, select_id: Option<i64>) -> bool {
        if generation != self.week_generation || week.start != self.week_start {
            return false;
        }
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
        true
    }

    pub fn select_day(&mut self, day: NaiveDate) -> bool {
        self.day = day;
        let week_start = domain::time::monday_of(day);
        let moved = week_start != self.week_start;
        if moved {
            self.week_start = week_start;
            self.week_generation += 1;
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

    pub fn report_summary(&self) -> Option<&Summary> {
        self.report().is_some().then_some(&self.report_summary)
    }

    pub fn apply_report(&mut self, report: Report) -> bool {
        let current = report.month == Some(self.report_month);
        if current {
            self.report = report;
            self.report_summary = self.report.summary();
        }
        current
    }

    pub fn invalidate_report(&mut self) {
        self.report = Report::default();
        self.report_summary = self.report.summary();
    }

    pub fn review_month(&self) -> NaiveDate {
        self.review.month
    }

    /// Another month's rows go at once, so they never show under its title.
    pub fn set_review_month(&mut self, day: NaiveDate) {
        let month = domain::time::first_of_month(day);
        if month != self.review.month {
            let Review { generation, revision, .. } = self.review;
            let analysis = review::Analysis::default();
            self.review = Review { month, loaded: None, rows: Vec::new(), selected: None, generation, analysis, revision };
            self.analyze_review();
        }
    }

    pub fn begin_review_load(&mut self) -> (u64, NaiveDate) {
        self.review.generation += 1;
        (self.review.generation, self.review.month)
    }

    /// Drops what an older load or one for another month brings back.
    pub fn apply_review(&mut self, generation: u64, month: NaiveDate, rows: Vec<TimeReportRow>, select_id: Option<i64>) -> bool {
        if generation != self.review.generation || month != self.review.month {
            return false;
        }
        let ids = |rows: &[TimeReportRow]| rows.iter().map(|r| r.id).collect::<Vec<_>>();
        self.review.selected = reselect(self.review.selected, select_id, &ids(&self.review.rows), &ids(&rows));
        self.review.rows = rows;
        self.review.loaded = Some(month);
        self.analyze_review();
        true
    }

    fn analyze_review(&mut self) {
        self.review.analysis = review::Analysis::of(&self.review.rows, &self.settings.excluded_tasks);
        self.review.revision += 1;
    }

    /// The settings go through here (not by assigning the field), as the review's findings
    /// depend on the excluded tasks.
    pub fn set_settings(&mut self, settings: Settings) {
        let excluded_changed = settings.excluded_tasks != self.settings.excluded_tasks;
        self.settings = settings;
        if excluded_changed {
            self.analyze_review();
        }
    }

    /// The lines, tag mismatches and findings of `review_rows`; None while those are.
    pub fn review_analysis(&self) -> Option<&review::Analysis> {
        self.review_rows().is_some().then_some(&self.review.analysis)
    }

    /// Changes whenever `review_analysis` is redone, also across sessions (which have their own
    /// epoch).
    pub fn review_revision(&self) -> (u64, u64) {
        (self.epoch, self.review.revision)
    }

    /// None unless loaded for the month shown and not invalidated since.
    pub fn review_rows(&self) -> Option<&[TimeReportRow]> {
        (self.review.loaded == Some(self.review.month)).then_some(self.review.rows.as_slice())
    }

    pub fn invalidate_review(&mut self) {
        self.review.loaded = None;
    }

    pub fn selected_review_id(&self) -> Option<i64> {
        self.review.selected
    }

    pub fn selected_review(&self) -> Option<&TimeReportRow> {
        self.review_rows()?.iter().find(|r| Some(r.id) == self.review.selected)
    }

    pub fn select_review(&mut self, id: i64) {
        if self.review_rows().is_some_and(|rows| rows.iter().any(|r| r.id == id)) {
            self.review.selected = Some(id);
        }
    }

    pub fn move_review_selection(&mut self, delta: isize) {
        let Some(rows) = self.review_rows() else { return };
        let Some(last) = rows.len().checked_sub(1) else { return };
        let pos = rows.iter().position(|r| Some(r.id) == self.review.selected);
        self.review.selected = Some(rows[pos.map_or(0, |p| p.saturating_add_signed(delta).min(last))].id);
    }

    /// The next (`direction` 1) or previous (-1) row with a finding, wrapping around.
    pub fn next_review_finding(&mut self, direction: isize) {
        let Some(rows) = self.review_rows() else { return };
        let mismatches = &self.review.analysis.mismatches;
        let len = rows.len() as isize;
        let pos = rows.iter().position(|r| Some(r.id) == self.review.selected);
        let start = pos.map_or(if direction < 0 { len } else { -1 }, |p| p as isize);
        let next = (1..=len)
            .map(|step| (start + direction * step).rem_euclid(len) as usize)
            .find(|&i| review::has_finding(&rows[i], mismatches, &self.settings.excluded_tasks));
        if let Some(i) = next {
            self.review.selected = Some(rows[i].id);
        }
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

    /// By name, as the config holds them, so it also answers for inactive or deleted tasks.
    pub fn is_task_hidden(&self, project_id: i64, task_name: &str) -> bool {
        self.project(project_id).is_some_and(|p| {
            self.settings
                .hidden_tasks
                .iter()
                .any(|(project, tasks)| same_name(project, &p.name) && tasks.iter().any(|t| same_name(t, task_name)))
        })
    }

    /// `entry`'s task is offered even if hidden or inactive, and `keep` (a task chosen elsewhere,
    /// as by a preset) even if hidden.
    pub fn task_options(&self, project_id: Option<i64>, entry: Option<&TimeEntry>, keep: Option<i64>) -> Vec<(i64, String)> {
        let mut tasks: Vec<(i64, String)> = project_id
            .map(|id| {
                self.project_tasks(id)
                    .iter()
                    .filter(|t| t.active && (Some(t.id) == keep || !self.is_task_hidden(id, &t.name)))
                    .map(|t| (t.id, t.name.clone()))
                    .collect()
            })
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

    /// Where `date`'s last entry ended: from the week's entries, or the review's rows for a day
    /// outside the week.
    pub fn next_start_time(&self, date: NaiveDate) -> Option<String> {
        let in_week = domain::week::touches_week(self.week_start, &[date]);
        let times: Vec<Option<&str>> = if in_week {
            self.entries.iter().filter(|e| e.date == date).map(|e| e.start_end.as_deref()).collect()
        } else {
            let rows = self.review_rows().unwrap_or_default();
            rows.iter().filter(|r| r.date == date).map(|r| r.start_end.as_deref()).collect()
        };
        times.into_iter().filter_map(|t| t?.split_once('-').map(|(_, end)| end.to_owned())).max()
    }

    pub fn timezone(&self, own: Option<&str>) -> chrono_tz::Tz {
        let account = self.me().and_then(|m| m.timezone.as_deref());
        let entries = self.entries.iter().map(|e| e.timezone.as_deref());
        domain::time::timezone([own, account].into_iter().chain(entries))
    }

    /// The wall-clock time in the user's timezone.
    pub fn now(&self) -> chrono::NaiveDateTime {
        chrono::Utc::now().with_timezone(&self.timezone(None)).naive_local()
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
    use std::collections::BTreeMap;

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
        load_week(&mut s, week(vec![entry(1, 3, "09:00-10:00"), entry(2, 4, "09:00-10:00"), entry(3, 4, "10:00-11:30")]), None);
        assert_eq!(s.visible_entries(), [1, 2]);
        assert_eq!(s.selected_entry().map(|e| e.id), Some(2), "the first entry of the selected day");
        s.move_entry_selection(5);
        assert_eq!(s.selected_entry().map(|e| e.id), Some(3), "stays within the day");
        assert_eq!(s.next_start_time(date(4)).as_deref(), Some("11:30"));
        assert_eq!(s.recent.len(), 1, "same project, task and notes");

        load_week(&mut s, week(vec![entry(2, 4, "09:00-10:00"), entry(4, 4, "12:00-13:00")]), Some(4));
        assert_eq!(s.selected_entry().map(|e| e.id), Some(4));

        assert!(!s.select_day(date(8)), "Sunday is still this week");
        assert_eq!(s.selected_entry_id(), None);
        assert!(s.week_loaded());
        assert!(s.select_day(date(9)));
        assert_eq!(s.week_start, date(9));
        assert!(!s.week_loaded() && s.entries.is_empty(), "the week left doesn't show under the new one");

        // Its entries were cleared on leaving, and no load has come back since.
        assert!(s.select_day(date(4)));
        assert!(!s.week_loaded() && s.entries.is_empty());
    }

    fn load_week(s: &mut Session, week: Week, select_id: Option<i64>) -> bool {
        let (generation, _) = s.begin_week_load();
        s.apply_week(generation, week, select_id)
    }

    #[test]
    fn only_the_latest_load_of_the_week_shown_applies() {
        let mut s = Session::new(None, date(4));
        let week = |monday: u32, id| Week {
            start: date(monday),
            entries: vec![entry(id, monday + 1, "09:00-10:00")],
            travel: vec![trip(id, monday + 1)],
        };

        let (first, monday) = s.begin_week_load();
        let (second, _) = s.begin_week_load();
        assert_eq!(monday, date(2));
        assert!(!s.apply_week(first, week(2, 1), None), "an older load");
        assert!(!s.week_loaded());
        assert!(!s.apply_week(second, week(9, 1), None), "another week");
        assert!(!s.week_loaded() && s.entries().is_empty());
        assert!(s.apply_week(second, week(2, 1), None));
        assert!(s.week_loaded() && s.entries().len() == 1);

        // A load for this week is underway when the user moves on before any load for the next
        // could start (as while the user isn't known yet): its week must not show under the next.
        let (pending, _) = s.begin_week_load();
        assert!(s.select_day(date(11)));
        assert!(!s.apply_week(pending, week(2, 2), None));
        assert!(!s.week_loaded() && s.entries().is_empty() && s.travel().is_empty());
        assert!(load_week(&mut s, week(9, 3), None));
        assert_eq!(s.entries()[0].id, 3);

        let (generation, _) = s.begin_week_load();
        assert!(!s.select_day(date(10)), "the same week");
        assert!(s.apply_week(generation, week(9, 4), None), "a day within the week keeps its load");
    }

    #[test]
    fn next_start_time_outside_the_week_comes_from_the_review() {
        let mut s = Session::new(None, date(4));
        load_week(&mut s, Week { start: date(2), entries: vec![entry(1, 4, "09:00-10:00")], travel: Vec::new() }, None);
        assert_eq!(s.next_start_time(date(10)), None, "no review loaded");
        s.set_review_month(date(20));
        let (generation, month) = s.begin_review_load();
        let row = |id, day, start_end: &str| TimeReportRow { start_end: Some(start_end.into()), ..time_row(id, day) };
        let rows = vec![row(1, 4, "13:00-14:00"), row(2, 10, "08:00-12:15"), row(3, 10, "12:15-12:45")];
        s.apply_review(generation, month, rows, None);
        assert_eq!(s.next_start_time(date(4)).as_deref(), Some("10:00"), "a day of the week: its entries");
        assert_eq!(s.next_start_time(date(10)).as_deref(), Some("12:45"));
        assert_eq!(s.next_start_time(date(11)), None);
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
        let rows = vec![crate::test_support::report_row("A", "Dev", 2, 2.0, true)];
        assert!(s.apply_report(Report { time: rows, ..report(date(1)) }));
        assert!(s.report().is_some());
        assert_eq!(s.report_summary().map(|summary| summary.total), Some(2.0), "summed when applied");
        s.set_report_month(date(1) - Duration::days(1));
        assert!(s.report().is_none(), "another month");
        s.set_report_month(date(30));
        assert!(s.report().is_some());
        s.invalidate_report();
        assert!(s.report().is_none());
        assert!(s.report_summary().is_none());
    }

    #[test]
    fn review_month_and_rows() {
        let mut s = Session::new(None, date(4));
        assert_eq!(s.review_month(), NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), "the previous month");
        assert!(s.review_rows().is_none());

        let november = date(1);
        let (stale, _) = s.begin_review_load();
        s.set_review_month(date(20));
        let (generation, month) = s.begin_review_load();
        assert_eq!(month, november);
        assert!(!s.apply_review(stale, month, vec![time_row(1, 2)], None), "an older load");
        assert!(!s.apply_review(generation, november - Months::new(1), vec![time_row(1, 2)], None), "another month");
        assert!(s.review_rows().is_none());

        assert!(s.apply_review(generation, month, vec![time_row(1, 2), time_row(2, 3)], None));
        assert_eq!(s.review_rows().map(<[_]>::len), Some(2));
        assert_eq!(s.selected_review_id(), Some(1), "the first row");

        s.set_review_month(date(30));
        assert!(s.review_rows().is_some(), "the same month keeps its rows");
        s.invalidate_review();
        assert!(s.review_rows().is_none());

        s.set_review_month(november - Months::new(1));
        assert!(s.review.rows.is_empty() && s.selected_review_id().is_none(), "stepping clears the rows");
    }

    #[test]
    fn review_analysis_follows_rows_and_excluded_tasks() {
        let mut s = Session::new(None, date(4));
        assert!(s.review_analysis().is_none());
        let (generation, month) = s.begin_review_load();
        let leave = TimeReportRow { project: "Intern".into(), task: "Verlof".into(), ..noted_row(2, None, None) };
        let start = s.review_revision();
        s.apply_review(generation, month, vec![noted_row(1, Some("x"), None), leave], None);
        let loaded = s.review_revision();
        assert_ne!(loaded, start, "a load redoes it");
        let analysis = s.review_analysis().unwrap();
        assert_eq!(analysis.lines.len(), 3);
        assert_eq!(analysis.findings.no_notes, 0, "leave needs no notes");

        s.set_settings(s.settings.clone());
        assert_eq!(s.review_revision(), loaded, "the same excluded tasks change nothing");
        s.set_settings(Settings { excluded_tasks: Default::default(), ..s.settings.clone() });
        assert_ne!(s.review_revision(), loaded);
        assert_eq!(s.review_analysis().unwrap().findings.no_notes, 1);

        s.invalidate_review();
        assert!(s.review_analysis().is_none(), "stale with its rows");
        let before = s.review_revision();
        s.set_review_month(date(4) - Months::new(3));
        assert_ne!(s.review_revision(), before, "another month");
        assert_ne!(Session::new(None, date(4)).review_revision(), Session::new(None, date(4)).review_revision());
    }

    #[test]
    fn review_finding_navigation_wraps() {
        let mut s = Session::new(None, date(4));
        s.next_review_finding(1);
        assert_eq!(s.selected_review_id(), None, "nothing loaded");
        let (generation, month) = s.begin_review_load();
        let rows = vec![
            noted_row(1, Some("x"), None),
            noted_row(2, None, None),
            noted_row(3, Some("y"), None),
            noted_row(4, Some("z"), Some("a")),
            noted_row(5, Some("z"), None),
        ];
        s.apply_review(generation, month, rows, None);
        assert_eq!(s.selected_review_id(), Some(1));
        let mut visits = Vec::new();
        for _ in 0..4 {
            s.next_review_finding(1);
            visits.push(s.selected_review_id().unwrap());
        }
        assert_eq!(visits, [2, 4, 5, 2], "forwards, wrapping");
        visits.clear();
        for _ in 0..4 {
            s.next_review_finding(-1);
            visits.push(s.selected_review_id().unwrap());
        }
        assert_eq!(visits, [5, 4, 2, 5], "backwards, wrapping");

        let (generation, month) = s.begin_review_load();
        s.apply_review(generation, month, vec![noted_row(1, Some("x"), None)], None);
        s.next_review_finding(1);
        assert_eq!(s.selected_review_id(), Some(1), "no findings: stays");
    }

    #[test]
    fn review_selection() {
        let mut s = Session::new(None, date(4));
        let load = |s: &mut Session, ids: &[i64], select_id| {
            let (generation, month) = s.begin_review_load();
            s.apply_review(generation, month, ids.iter().map(|&id| time_row(id, 2)).collect(), select_id)
        };
        s.select_review(1);
        assert_eq!(s.selected_review_id(), None, "nothing loaded");
        load(&mut s, &[1, 2, 3], None);
        s.select_review(2);
        s.select_review(9);
        assert_eq!(s.selected_review_id(), Some(2), "not a row");
        s.move_review_selection(5);
        assert_eq!(s.selected_review_id(), Some(3), "stops at the last row");
        s.move_review_selection(-1);
        assert_eq!(s.selected_review_id(), Some(2));

        s.invalidate_review();
        s.move_review_selection(1);
        assert_eq!(s.selected_review_id(), Some(2), "not while stale");
        load(&mut s, &[3, 1, 2], None);
        assert_eq!(s.selected_review_id(), Some(2), "kept after a reload");
        s.invalidate_review();
        load(&mut s, &[3, 1], None);
        assert_eq!(s.selected_review_id(), Some(1), "deleted: the row now at its place");
        load(&mut s, &[3, 1, 7], Some(7));
        assert_eq!(s.selected_review_id(), Some(7), "the saved row");
    }

    #[test]
    fn kept_tags() {
        let mut s = Session::new(None, date(4));
        s.set_project_tags(HashMap::from([(1, vec![tag(10, "a")]), (2, vec![tag(20, "b")])]));
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

        s.set_project_tags(HashMap::from([(1, Vec::new())]));
        assert_eq!(ids(&s, Some(1)), [10, 30], "only empty lists count as nothing configured");
        s.set_project_tags(HashMap::from([(1, Vec::new()), (2, vec![tag(30, "b")])]));
        assert_eq!(ids(&s, Some(1)), Vec::<i64>::new(), "once one project has tags, the lists hold");
        assert_eq!(ids(&s, Some(2)), [30]);
    }

    #[test]
    fn hidden_projects_are_not_offered() {
        let mut s = Session::new(None, date(4));
        s.apply_base(base(vec![project(1, "Keep"), project(2, "Old")], HashMap::new(), Vec::new()), None, &[]);
        s.set_settings(Settings { hidden_projects: vec!["old".into()], ..Settings::default() });
        let ids = |options: Vec<(i64, String)>| options.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert!(s.is_hidden(2) && !s.is_hidden(1));
        assert_eq!(ids(s.project_options(None)), [1]);
        assert_eq!(ids(s.project_options(Some((2, "Old", "Us")))), [1, 2], "still offered when editing its entry");
    }

    #[test]
    fn hidden_tasks_are_not_offered() {
        let mut s = Session::new(None, date(4));
        let tasks = HashMap::from([(1, vec![task(10, 1, "Dev"), task(20, 1, "Old"), task(30, 1, "Meet")])]);
        s.apply_base(base(vec![project(1, "Keep"), project(2, "Other")], tasks, Vec::new()), None, &[]);
        s.set_settings(Settings {
            hidden_tasks: BTreeMap::from([("keep".into(), vec!["old".into(), "meet".into()])]),
            ..Settings::default()
        });
        let ids = |options: Vec<(i64, String)>| options.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert!(s.is_task_hidden(1, "OLD") && !s.is_task_hidden(1, "Dev") && !s.is_task_hidden(2, "Old"));
        assert_eq!(ids(s.task_options(Some(1), None, None)), [10]);
        assert_eq!(ids(s.task_options(Some(1), None, Some(30))), [10, 30], "kept when a preset picks it");
        let mut e = entry(5, 4, "09:00-10:00");
        (e.project_id, e.project_task_id, e.task_name) = (1, 20, "Old".into());
        assert_eq!(ids(s.task_options(Some(1), Some(&e), None)), [10, 20], "still offered when editing its entry");
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
    fn a_failed_config_load_blocks_writes_until_one_succeeds() {
        let config = |billable_goal| Config {
            settings: Settings { billable_goal, ..Settings::default() },
            projects: HashMap::new(),
            trips: Vec::new(),
            time_entries: Vec::new(),
        };
        let mut s = Session::new(None, date(4));
        assert_eq!(s.config_writable(), Ok(()));
        s.apply_config(Some(config(0.7)));
        s.apply_config_error("bad billable_goal".into());
        assert_eq!(s.settings, Settings::default());
        assert_eq!(s.config_writable(), Err("Fix config.toml first: bad billable_goal".into()));
        s.apply_config(Some(config(0.8)));
        assert_eq!(s.config_writable(), Ok(()), "a fixed file loads");
        s.apply_config_error("again".into());
        s.apply_config(None);
        assert_eq!(s.config_writable(), Ok(()), "a missing file gets a fresh template");
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
