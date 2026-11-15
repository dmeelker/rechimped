mod config;
mod review;
mod week;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, NaiveDate};
use timechimp_api::{CurrentUser, Mileage, Project, ProjectTask, Tag, TimeEntry, TimeReportRow};

pub use self::config::ConfigState;
use self::review::Review;
pub use self::week::WeekState;
use crate::backend::Api;
use crate::domain::report::{Report, Summary};
use crate::domain::{self, same_name, week::RecentItem};
#[cfg(test)]
use crate::store::config::TripPreset;
use crate::store::config::{self as config_file, Config, Settings, TimePreset};
use crate::tab::Tab;

pub struct Session {
    /// `TimeChimpApp::run` drops results that arrive for an older epoch.
    epoch: u64,
    client: Option<Api>,
    base: Option<Base>,
    /// Bumped on every base data load, so an older one finishing last doesn't win.
    base_generation: u64,
    config: ConfigState,
    week: WeekState,
    history: HashMap<i64, TimeEntry>,
    recent: Vec<RecentItem>,

    /// Always the first of the month.
    report_month: NaiveDate,
    /// May be for another month than `report_month` until the new one is in.
    report: Report,
    /// `report.summary()`, worked out once when the report changes rather than every frame.
    report_summary: Summary,
    /// Bumped on every report load and by `invalidate_derived`, so a load that started before a
    /// newer one, or before a change to the user's data, is dropped.
    report_generation: u64,
    /// Bumped by `invalidate_derived`: loads without a generation of their own (the chart, the
    /// entry the Review tab opens) compare it to drop what was read before a change.
    data_version: u64,

    review: Review,
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
            base_generation: 0,
            config: ConfigState::default(),
            week: WeekState::new(today),
            history: HashMap::new(),
            recent: Vec::new(),
            report_month: domain::time::first_of_month(today),
            report: Report::default(),
            report_summary: Report::default().summary(),
            report_generation: 0,
            data_version: 0,
            review: Review::new(today),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// config.toml as loaded: settings, project tags and presets.
    pub fn config(&self) -> &ConfigState {
        &self.config
    }

    /// The week the Time and Travel tabs show, with its entries, trips and selection.
    pub fn week(&self) -> &WeekState {
        &self.week
    }

    pub fn recent(&self) -> &[RecentItem] {
        &self.recent
    }

    /// Stands in for loading `[projects]` from config.toml.
    #[cfg(test)]
    pub fn set_project_tags(&mut self, project_tags: HashMap<i64, Vec<Tag>>) {
        self.config.set_project_tags(project_tags);
    }

    /// Stands in for loading the presets from config.toml.
    #[cfg(test)]
    pub fn set_presets(&mut self, trips: Vec<TripPreset>, times: Vec<TimePreset>) {
        self.config.set_presets(trips, times);
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

    pub fn begin_base_load(&mut self) -> u64 {
        self.base_generation += 1;
        self.base_generation
    }

    /// Drops what an older load brings back. Only replaces the base: the week, history and
    /// config are applied on their own, so these may arrive in either order.
    pub fn apply_base(&mut self, generation: u64, base: Base) -> bool {
        let current = generation == self.base_generation;
        if current {
            self.base = Some(base);
        }
        current
    }

    /// Stands in for loading the base data.
    #[cfg(test)]
    pub fn set_base(&mut self, base: Base) {
        let generation = self.begin_base_load();
        self.apply_base(generation, base);
    }

    /// `history` leaves out the loaded week, so its `entries` are added here.
    pub fn apply_history(&mut self, history: Vec<TimeEntry>, entries: &[TimeEntry]) {
        self.history = history.into_iter().chain(entries.iter().cloned()).map(|e| (e.id, e)).collect();
        self.recent = domain::week::recent_items(self.history.values());
    }

    /// Falls back to the defaults, like a missing file, but blocks writes until a load succeeds.
    pub fn apply_config_error(&mut self, error: String) {
        self.apply_config(None);
        self.config.fail(error);
    }

    pub fn apply_config(&mut self, config: Option<Config>) -> Option<String> {
        let Some(config) = config else {
            self.config.apply(HashMap::new(), Vec::new(), Vec::new());
            self.set_settings(Settings::default());
            return None;
        };
        self.set_settings(config.settings);
        let (resolved, warning) = config_file::resolve_project_tags(&config.projects, self.projects(), self.tags());
        let mut unknown: Vec<String> = config
            .trips
            .iter()
            .filter(|t| self.project_named(&t.project).is_none())
            .map(|t| format!("project \"{}\" (trip \"{}\")", t.project, t.name))
            .collect();
        for preset in &config.time_entries {
            let what = preset.label();
            match self.resolve_time_preset(preset) {
                Err(name) => unknown.push(format!("{name} (time entry \"{what}\")")),
                Ok(resolved) => {
                    unknown.extend(resolved.unknown_tags.iter().map(|t| format!("tag \"{t}\" (time entry \"{what}\")")))
                }
            }
        }
        self.config.apply(resolved, config.trips, config.time_entries);
        match (warning, unknown.is_empty()) {
            (w, true) => w,
            (None, false) => Some(format!("config.toml: unknown {}", unknown.join(", "))),
            (Some(w), false) => Some(format!("{w}, {}", unknown.join(", "))),
        }
    }

    /// The settings go through here (not by assigning the field), as the review's findings
    /// depend on the excluded tasks.
    pub fn set_settings(&mut self, settings: Settings) {
        if self.config.replace_settings(settings) {
            self.analyze_review();
        }
    }

    pub fn begin_week_load(&mut self) -> (u64, NaiveDate) {
        self.week.begin_load()
    }

    /// Drops what an older load or one for another week brings back.
    pub fn apply_week(&mut self, generation: u64, week: Week, select_id: Option<i64>) -> bool {
        let start = week.start;
        if !self.week.apply(generation, week, select_id) {
            return false;
        }
        // The week replaces what the history had for it, so saves and deletes show up in Recent.
        let week_end = start + Duration::days(7);
        self.history.retain(|_, e| !(start..week_end).contains(&e.date));
        self.history.extend(self.week.entries().iter().map(|e| (e.id, e.clone())));
        self.recent = domain::week::recent_items(self.history.values());
        true
    }

    /// Returns whether `day` is in another week, which then needs loading.
    pub fn select_day(&mut self, day: NaiveDate) -> bool {
        self.week.select_day(day)
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

    pub fn begin_report_load(&mut self) -> (u64, NaiveDate) {
        self.report_generation += 1;
        (self.report_generation, self.report_month)
    }

    /// Drops what an older load, one started before `invalidate_derived` or one for another
    /// month brings back.
    pub fn apply_report(&mut self, generation: u64, report: Report) -> bool {
        let current = generation == self.report_generation && report.month == Some(self.report_month);
        if current {
            self.report = report;
            self.report_summary = self.report.summary();
        }
        current
    }

    /// Drops the report and the review together, and whatever loads for them are still running:
    /// call after any change to the user's data.
    pub fn invalidate_derived(&mut self) {
        self.invalidate_report();
        self.invalidate_review();
        self.data_version += 1;
    }

    /// Changes whenever `invalidate_derived` runs.
    pub fn data_version(&self) -> u64 {
        self.data_version
    }

    fn invalidate_report(&mut self) {
        self.report = Report::default();
        self.report_summary = self.report.summary();
        self.report_generation += 1;
    }

    pub fn review_month(&self) -> NaiveDate {
        self.review.month()
    }

    /// Another month's rows go at once, so they never show under its title.
    pub fn set_review_month(&mut self, day: NaiveDate) {
        if self.review.set_month(day) {
            self.analyze_review();
        }
    }

    pub fn begin_review_load(&mut self) -> (u64, NaiveDate) {
        self.review.begin_load()
    }

    /// Drops what an older load or one for another month brings back.
    pub fn apply_review(&mut self, generation: u64, month: NaiveDate, rows: Vec<TimeReportRow>, select_id: Option<i64>) -> bool {
        let applied = self.review.apply(generation, month, rows, select_id);
        if applied {
            self.analyze_review();
        }
        applied
    }

    fn analyze_review(&mut self) {
        self.review.analyze(&self.config.settings().excluded_tasks);
    }

    /// The lines, tag mismatches and findings of `review_rows`; None while those are.
    pub fn review_analysis(&self) -> Option<&domain::review::Analysis> {
        self.review.analysis()
    }

    /// Changes whenever `review_analysis` is redone, also across sessions (which have their own
    /// epoch).
    pub fn review_revision(&self) -> (u64, u64) {
        (self.epoch, self.review.revision())
    }

    /// None unless loaded for the month shown and not invalidated since.
    pub fn review_rows(&self) -> Option<&[TimeReportRow]> {
        self.review.rows()
    }

    fn invalidate_review(&mut self) {
        self.review.invalidate();
    }

    pub fn selected_review_id(&self) -> Option<i64> {
        self.review.selected_id()
    }

    pub fn selected_review(&self) -> Option<&TimeReportRow> {
        self.review.selected()
    }

    /// The next (`direction` 1) or previous (-1) row with a finding, wrapping around.
    pub fn next_review_finding(&mut self, direction: isize) {
        self.review.next_finding(direction, &self.config.settings().excluded_tasks);
    }

    /// Moves `tab`'s selection `delta` rows, stopping at either end. The Report tab has none.
    pub fn move_selection(&mut self, tab: Tab, delta: isize) {
        match tab {
            Tab::Week => self.week.move_entry_selection(delta),
            Tab::Travel => self.week.move_travel_selection(delta),
            Tab::Review => self.review.move_selection(delta),
            Tab::Report => {}
        }
    }

    /// Selects row `id` of `tab`, if it is one of the rows that tab shows.
    pub fn select(&mut self, tab: Tab, id: i64) {
        match tab {
            Tab::Week => self.week.select_entry(id),
            Tab::Travel => self.week.select_travel(id),
            Tab::Review => self.review.select(id),
            Tab::Report => {}
        }
    }

    /// What is selected on `tab`; the Report tab has no selection.
    pub fn selection(&self, tab: Tab) -> Selected<'_> {
        let selected = match tab {
            Tab::Week => self.week.selected_entry().map(Selected::Entry),
            Tab::Travel => self.week.selected_travel().map(Selected::Trip),
            Tab::Review => self.selected_review().map(Selected::ReviewRow),
            Tab::Report => None,
        };
        selected.unwrap_or(Selected::None)
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
        let in_week = domain::week::touches_week(self.week.week_start(), &[date]);
        let times: Vec<Option<&str>> = if in_week {
            self.week.entries().iter().filter(|e| e.date == date).map(|e| e.start_end.as_deref()).collect()
        } else {
            let rows = self.review_rows().unwrap_or_default();
            rows.iter().filter(|r| r.date == date).map(|r| r.start_end.as_deref()).collect()
        };
        times.into_iter().filter_map(|t| t?.split_once('-').map(|(_, end)| end.to_owned())).max()
    }

    pub fn timezone(&self, own: Option<&str>) -> chrono_tz::Tz {
        let account = self.me().and_then(|m| m.timezone.as_deref());
        let entries = self.week.entries().iter().map(|e| e.timezone.as_deref());
        domain::time::timezone([own, account].into_iter().chain(entries))
    }

    /// The wall-clock time in the user's timezone; the system's until the account is loaded.
    pub fn now(&self) -> chrono::NaiveDateTime {
        self.wall_clock(chrono::Utc::now())
    }

    /// Today in the user's timezone (the date of `now`). Use this, not the system date, wherever
    /// a session exists, so "today" agrees with the forms' seeds near midnight.
    pub fn today(&self) -> NaiveDate {
        self.now().date()
    }

    fn wall_clock(&self, utc: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDateTime {
        match self.base {
            Some(_) => utc.with_timezone(&self.timezone(None)).naive_local(),
            None => utc.with_timezone(&chrono::Local).naive_local(),
        }
    }

    pub fn target_for(&self, day: usize) -> f64 {
        self.me().and_then(|m| m.user_availability.as_ref()).and_then(|a| a.for_day(day)).unwrap_or(0.0)
    }

    pub fn shown_days(&self, today: NaiveDate) -> Vec<usize> {
        let work_days = self.config.settings().work_days;
        let (week_start, day) = (self.week.week_start(), self.week.day());
        (0..7).filter(|&i| domain::week::shows_day(week_start + Duration::days(i as i64), work_days, today, day)).collect()
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

/// The row of a tab's list that is selected (see `Session::selection`).
#[derive(Clone, Copy, Debug)]
pub enum Selected<'a> {
    Entry(&'a TimeEntry),
    ReviewRow(&'a TimeReportRow),
    Trip(&'a Mileage),
    None,
}

impl Selected<'_> {
    /// Why the selected time entry can't be edited or deleted, if it can't; trips don't lock.
    pub fn lock_status(&self) -> Option<domain::ApprovalStatus> {
        match self {
            Selected::Entry(e) => domain::lock_status(e.status, e.status_extern),
            Selected::ReviewRow(r) => domain::lock_status(r.status, r.client_status),
            Selected::Trip(_) | Selected::None => None,
        }
    }
}

/// The id `delta` places from `selected` in `ids`, stopping at either end; the first one when
/// `selected` isn't among them, None when there are none.
fn step(ids: &[i64], selected: Option<i64>, delta: isize) -> Option<i64> {
    let last = ids.len().checked_sub(1)?;
    let pos = ids.iter().position(|&id| Some(id) == selected);
    Some(ids[pos.map_or(0, |p| p.saturating_add_signed(delta).min(last))])
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
    use chrono::Months;

    use super::*;
    use crate::test_support::*;

    #[test]
    fn clock_follows_the_account_timezone() {
        let utc = chrono::DateTime::parse_from_rfc3339("2026-11-04T12:00:00Z").unwrap().to_utc();
        let mut s = Session::new(None, date(4));
        assert_eq!(s.wall_clock(utc), utc.with_timezone(&chrono::Local).naive_local(), "system time before the account loads");
        let mut b = base(Vec::new(), HashMap::new(), Vec::new());
        b.me.timezone = Some("Pacific/Kiritimati".into());
        s.set_base(b);
        assert_eq!(s.wall_clock(utc).date(), date(5), "UTC+14 is already the next day");
        assert_eq!(s.wall_clock(utc).time(), chrono::NaiveTime::from_hms_opt(2, 0, 0).unwrap());
    }

    #[test]
    fn sessions_are_distinct() {
        assert_ne!(Session::new(None, date(4)).epoch, Session::new(None, date(4)).epoch);
    }

    #[test]
    fn week_and_selection() {
        let mut s = Session::new(None, date(4));
        assert_eq!(s.week().week_start(), date(2));
        let week = |entries| Week { start: date(2), entries, travel: Vec::new() };
        load_week(&mut s, week(vec![entry(1, 3, "09:00-10:00"), entry(2, 4, "09:00-10:00"), entry(3, 4, "10:00-11:30")]), None);
        assert_eq!(s.week().visible_entries(), [1, 2]);
        assert_eq!(s.week().selected_entry().map(|e| e.id), Some(2), "the first entry of the selected day");
        s.move_selection(Tab::Week, 5);
        assert_eq!(s.week().selected_entry().map(|e| e.id), Some(3), "stays within the day");
        assert_eq!(s.next_start_time(date(4)).as_deref(), Some("11:30"));
        assert_eq!(s.recent.len(), 1, "same project, task and notes");

        load_week(&mut s, week(vec![entry(2, 4, "09:00-10:00"), entry(4, 4, "12:00-13:00")]), Some(4));
        assert_eq!(s.week().selected_entry().map(|e| e.id), Some(4));

        assert!(!s.select_day(date(8)), "Sunday is still this week");
        assert_eq!(s.week().selected_entry_id(), None);
        assert!(s.week().loaded());
        assert!(s.select_day(date(9)));
        assert_eq!(s.week().week_start(), date(9));
        assert!(!s.week().loaded() && s.week().entries().is_empty(), "the week left doesn't show under the new one");

        // Its entries were cleared on leaving, and no load has come back since.
        assert!(s.select_day(date(4)));
        assert!(!s.week().loaded() && s.week().entries().is_empty());
    }

    fn load_week(s: &mut Session, week: Week, select_id: Option<i64>) -> bool {
        let (generation, _) = s.begin_week_load();
        s.apply_week(generation, week, select_id)
    }

    #[test]
    fn base_and_week_apply_in_either_order() {
        let week = || Week { start: date(2), entries: vec![entry(1, 4, "09:00-10:00")], travel: Vec::new() };
        let projects = |name| base(vec![project(3, name)], HashMap::new(), Vec::new());
        for base_first in [true, false] {
            let mut s = Session::new(None, date(4));
            s.set_base(projects("Old"));
            // A reload with the user known: the week and a base refresh run side by side.
            let generation = s.begin_base_load();
            if base_first {
                assert!(s.apply_base(generation, projects("New")));
            }
            load_week(&mut s, week(), Some(1));
            if !base_first {
                assert!(s.apply_base(generation, projects("New")));
            }
            assert_eq!(s.projects()[0].name, "New");
            assert!(s.week().loaded());
            assert_eq!(s.week().selected_entry_id(), Some(1), "kept when the base comes in after the week");
            assert_eq!(s.recent().len(), 1, "the week's entry is in Recent");
        }
    }

    #[test]
    fn base_from_an_older_load_is_dropped() {
        let mut s = Session::new(None, date(4));
        let first = s.begin_base_load();
        let second = s.begin_base_load();
        let projects = |name| base(vec![project(3, name)], HashMap::new(), Vec::new());
        assert!(s.apply_base(second, projects("Newer")));
        assert!(!s.apply_base(first, projects("Older")), "finished last, started first");
        assert_eq!(s.projects()[0].name, "Newer");
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
        let (generation, month) = s.begin_report_load();
        assert_eq!(month, date(1));
        assert!(!s.apply_report(generation, report(date(1) - Duration::days(1))), "October's report arrives too late");
        let rows = vec![crate::test_support::report_row("A", "Dev", 2, 2.0, true)];
        assert!(s.apply_report(generation, Report { time: rows, ..report(date(1)) }));
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
    fn invalidate_derived_drops_report_and_review() {
        let mut s = Session::new(None, date(4));
        s.set_report_month(date(1));
        s.set_review_month(date(1));
        let (generation, _) = s.begin_report_load();
        assert!(s.apply_report(generation, Report { month: Some(date(1)), ..Report::default() }));
        let (generation, month) = s.begin_review_load();
        assert!(s.apply_review(generation, month, vec![time_row(1, 2)], None));
        assert!(s.report().is_some() && s.review_rows().is_some());
        let version = s.data_version();
        s.invalidate_derived();
        assert!(s.report().is_none());
        assert!(s.review_rows().is_none());
        assert_ne!(s.data_version(), version);
    }

    #[test]
    fn report_loads_from_before_a_change_are_dropped() {
        let mut s = Session::new(None, date(4));
        let report = || Report { month: Some(date(1)), ..Report::default() };
        let (stale, _) = s.begin_report_load();
        s.invalidate_derived();
        assert!(!s.apply_report(stale, report()), "read before the change");
        assert!(s.report().is_none());

        let (older, _) = s.begin_report_load();
        let (current, _) = s.begin_report_load();
        assert!(!s.apply_report(older, report()), "a newer load is running");
        assert!(s.apply_report(current, report()), "the load started after the change");
        assert!(s.report().is_some());
    }

    #[test]
    fn review_loads_from_before_a_change_are_dropped() {
        let mut s = Session::new(None, date(4));
        let (stale, month) = s.begin_review_load();
        s.invalidate_derived();
        assert!(!s.apply_review(stale, month, vec![time_row(1, 2)], None));
        assert!(s.review_rows().is_none());
        let (current, month) = s.begin_review_load();
        assert!(s.apply_review(current, month, vec![time_row(1, 2)], None));
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

        s.set_settings(s.config().settings().clone());
        assert_eq!(s.review_revision(), loaded, "the same excluded tasks change nothing");
        s.set_settings(Settings { excluded_tasks: Default::default(), ..s.config().settings().clone() });
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
    fn step_stops_at_either_end() {
        assert_eq!(step(&[], Some(1), 1), None, "no rows");
        assert_eq!(step(&[4, 5, 6], None, 1), Some(4), "nothing selected: the first");
        assert_eq!(step(&[4, 5, 6], Some(9), -1), Some(4), "not a row: the first");
        assert_eq!(step(&[4, 5, 6], Some(4), 1), Some(5));
        assert_eq!(step(&[4, 5, 6], Some(5), 7), Some(6), "stops at the last");
        assert_eq!(step(&[4, 5, 6], Some(5), -7), Some(4), "stops at the first");
    }

    #[test]
    fn selection_per_tab() {
        let mut s = Session::new(None, date(4));
        assert!(matches!(s.selection(Tab::Week), Selected::None), "nothing loaded");
        let mut locked = entry(2, 4, "09:00-10:00");
        locked.status = 2;
        let travel = vec![trip(7, 3), trip(8, 4)];
        load_week(&mut s, Week { start: date(2), entries: vec![entry(1, 3, "09:00-10:00"), locked], travel }, None);
        let (generation, month) = s.begin_review_load();
        let mut row = time_row(5, 2);
        row.client_status = 1;
        s.apply_review(generation, month, vec![time_row(4, 2), row], None);
        s.select(Tab::Travel, 8);
        s.select(Tab::Review, 5);

        let Selected::Entry(e) = s.selection(Tab::Week) else { panic!("an entry") };
        assert_eq!(e.id, 2, "the selected day's");
        assert_eq!(s.selection(Tab::Week).lock_status(), Some(domain::ApprovalStatus::Approved));
        let Selected::Trip(m) = s.selection(Tab::Travel) else { panic!("a trip") };
        assert_eq!(m.id, 8);
        assert_eq!(s.selection(Tab::Travel).lock_status(), None, "trips don't lock");
        let Selected::ReviewRow(r) = s.selection(Tab::Review) else { panic!("a review row") };
        assert_eq!(r.id, 5);
        assert_eq!(s.selection(Tab::Review).lock_status(), Some(domain::ApprovalStatus::PendingApproval));
        assert!(matches!(s.selection(Tab::Report), Selected::None), "the report has no selection");

        s.select(Tab::Week, 1);
        assert_eq!(s.week().selected_entry_id(), Some(1));
        s.move_selection(Tab::Travel, -1);
        assert_eq!(s.week().selected_travel_id(), Some(7));
        s.invalidate_review();
        assert!(matches!(s.selection(Tab::Review), Selected::None), "not while stale");
    }

    #[test]
    fn review_selection() {
        let mut s = Session::new(None, date(4));
        let load = |s: &mut Session, ids: &[i64], select_id| {
            let (generation, month) = s.begin_review_load();
            s.apply_review(generation, month, ids.iter().map(|&id| time_row(id, 2)).collect(), select_id)
        };
        s.select(Tab::Review, 1);
        assert_eq!(s.selected_review_id(), None, "nothing loaded");
        load(&mut s, &[1, 2, 3], None);
        s.select(Tab::Review, 2);
        s.select(Tab::Review, 9);
        assert_eq!(s.selected_review_id(), Some(2), "not a row");
        s.move_selection(Tab::Review, 5);
        assert_eq!(s.selected_review_id(), Some(3), "stops at the last row");
        s.move_selection(Tab::Review, -1);
        assert_eq!(s.selected_review_id(), Some(2));

        s.invalidate_review();
        s.move_selection(Tab::Review, 1);
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
            time_entries: vec![TimePreset {
                project: "Gone".into(),
                task: None,
                notes: Some("Standup".into()),
                tags: Vec::new(),
            }],
        };
        let warning = s.apply_config(Some(config)).unwrap();
        assert!(warning.contains("project \"Nope\"") && warning.contains("trip \"Trip\""), "{warning}");
        assert!(warning.contains("(time entry \"Standup\")"), "{warning}");
        assert_eq!(
            (s.config().settings().billable_goal, s.config().trip_presets().len(), s.config().time_presets().len()),
            (0.7, 1, 1)
        );
        assert_eq!(s.apply_config(None), None);
        assert_eq!(
            (s.config().settings(), s.config().trip_presets().len(), s.config().time_presets().len()),
            (&Settings::default(), 0, 0)
        );
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
        assert_eq!(s.config().writable(), Ok(()));
        s.apply_config(Some(config(0.7)));
        s.apply_config_error("bad billable_goal".into());
        assert_eq!(s.config().settings(), &Settings::default());
        assert_eq!(s.config().writable(), Err("Fix config.toml first: bad billable_goal".into()));
        s.apply_config(Some(config(0.8)));
        assert_eq!(s.config().writable(), Ok(()), "a fixed file loads");
        s.apply_config_error("again".into());
        s.apply_config(None);
        assert_eq!(s.config().writable(), Ok(()), "a missing file gets a fresh template");
    }

    #[test]
    fn time_presets() {
        let mut s = Session::new(None, date(4));
        s.set_base(base(vec![project(3, "Intern")], HashMap::from([(3, vec![task(4, 3, "Meeting")])]), vec![tag(7, "Daily")]));
        let preset = |task: Option<&str>, tags: &[&str]| TimePreset {
            project: "intern".into(),
            task: task.map(Into::into),
            notes: None,
            tags: tags.iter().map(|t| t.to_string()).collect(),
        };
        assert_eq!(
            s.resolve_time_preset(&preset(Some("meeting"), &["daily", "gone"])),
            Ok(ResolvedTimePreset {
                project_id: 3,
                project_task_id: Some(4),
                tag_ids: vec![7],
                unknown_tags: vec!["gone".into()]
            })
        );
        assert_eq!(s.resolve_time_preset(&preset(None, &[])).unwrap().project_task_id, None);
        assert!(s.resolve_time_preset(&preset(Some("Nap"), &[])).unwrap_err().starts_with("task \"Nap\""));
    }
}
