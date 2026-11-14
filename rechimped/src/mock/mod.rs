//! An in-memory TimeChimp account for `--mock`: generated at startup (`data`), changed by the
//! app's saves and deletes, and gone when the app closes.

mod data;

pub use data::config;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Mutex;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use timechimp_api::{
    CurrentUser, Error, Mileage, MileageReportRow, NewMileage, NewTimeEntry, PlacePrediction, Project, ProjectTask, QueryResult,
    RouteDistance, Tag, TagType, TimeEntry, TimeReportRow,
};

use crate::backend::{Backend, Call};

pub struct Mock {
    state: Mutex<State>,
}

/// The approval statuses of an entry or trip: internal, client.
#[derive(Clone, Copy, Default)]
struct Status {
    intern: i32,
    client: i32,
}

struct State {
    me: CurrentUser,
    /// Inactive ones too: their entries still show in the reports.
    projects: Vec<Project>,
    tasks: Vec<ProjectTask>,
    tags: Vec<Tag>,
    entries: Vec<TimeEntry>,
    trips: Vec<Mileage>,
    next_id: i64,
}

impl Mock {
    /// The data runs up to `now`.
    pub fn new(now: DateTime<Utc>) -> Self {
        Self { state: Mutex::new(data::generate(now.with_timezone(&data::TZ).naive_local())) }
    }

    fn call<T: Send + 'static>(&self, f: impl FnOnce(&mut State) -> timechimp_api::Result<T>) -> Call<'static, T> {
        let result = f(&mut self.state.lock().expect("mock state"));
        Box::pin(std::future::ready(result))
    }
}

fn not_found(what: &str, id: i64) -> Error {
    Error::NotFound(format!("mock: no {what} {id}"))
}

fn page<T>(rows: Vec<T>, range: Range<u32>) -> QueryResult<T> {
    let row_count = rows.len() as i64;
    let row_data = rows.into_iter().skip(range.start as usize).take(range.len()).collect();
    QueryResult { row_data, row_count }
}

fn in_week(date: NaiveDate, monday: NaiveDate) -> bool {
    (monday..monday + chrono::Duration::days(7)).contains(&date)
}

fn non_empty(notes: &Option<String>) -> Option<String> {
    notes.clone().filter(|n| !n.trim().is_empty())
}

impl State {
    fn project(&self, id: i64) -> timechimp_api::Result<&Project> {
        self.projects.iter().find(|p| p.id == id).ok_or_else(|| not_found("project", id))
    }

    fn user_name(&self, user_id: i64) -> String {
        if user_id == self.me.id { self.me.display_name.clone() } else { data::COLLEAGUE.into() }
    }

    fn take_id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }

    fn time_entry(&self, id: i64, new: &NewTimeEntry, status: Status) -> timechimp_api::Result<TimeEntry> {
        let project = self.project(new.project_id)?;
        let task = self
            .tasks
            .iter()
            .find(|t| t.id == new.project_task_id && t.project_id == project.id)
            .ok_or_else(|| not_found("project task", new.project_task_id))?;
        let tags: Vec<Tag> = self.tags.iter().filter(|t| new.tag_ids.contains(&t.id)).cloned().collect();
        let start_end = new.start.zip(new.end).map(|(s, e)| format!("{}-{}", s.format("%H:%M"), e.format("%H:%M")));
        Ok(TimeEntry {
            id,
            time_row_id: id,
            user_id: new.user_id,
            user_display_name: self.user_name(new.user_id),
            user_tag_names: Vec::new(),
            customer_id: project.customer_id,
            customer_name: project.customer_name.clone(),
            project_id: project.id,
            project_name: project.name.clone(),
            project_code: None,
            project_task_id: task.id,
            task_id: task.task_id,
            task_name: task.name.clone(),
            task_code: None,
            date: new.date,
            hours: new.effective_hours(),
            notes: non_empty(&new.notes),
            start_end,
            start: new.start.map(|t| t.naive_utc()),
            end: new.end.map(|t| t.naive_utc()),
            pause: new.pause,
            billable: task.billable,
            status: status.intern,
            status_intern: status.intern,
            status_extern: status.client,
            tag_ids: tags.iter().map(|t| t.id).collect(),
            tag_names: tags.iter().map(|t| t.name.clone()).collect(),
            tags,
            external_url: None,
            external_name: None,
            modified: Utc::now().naive_utc(),
            timezone: new.timezone.clone(),
            extra: HashMap::new(),
        })
    }

    fn trip(&self, id: i64, new: &NewMileage, status: Status) -> timechimp_api::Result<Mileage> {
        let project = self.project(new.project_id)?;
        Ok(Mileage {
            id,
            user_id: new.user_id,
            user_display_name: self.user_name(new.user_id),
            customer_id: project.customer_id,
            customer_name: project.customer_name.clone(),
            project_id: project.id,
            project_name: project.name.clone(),
            project_code: None,
            date: new.date,
            mileage_vehicle_id: new.mileage_vehicle_id,
            mileage_vehicle_name: None,
            from_address: Some(new.from_address.clone()),
            to_address: Some(new.to_address.clone()),
            notes: non_empty(&new.notes),
            distance: new.distance,
            billable: new.billable,
            mileage_type: new.mileage_type,
            status: status.intern,
            status_intern: status.intern,
            status_extern: status.client,
            modified: Utc::now().naive_utc(),
        })
    }

    fn add_entry(&mut self, new: &NewTimeEntry, status: Status) -> timechimp_api::Result<TimeEntry> {
        let id = self.take_id();
        let entry = self.time_entry(id, new, status)?;
        self.entries.push(entry.clone());
        Ok(entry)
    }

    fn add_trip(&mut self, new: &NewMileage, status: Status) -> timechimp_api::Result<Mileage> {
        let id = self.take_id();
        let trip = self.trip(id, new, status)?;
        self.trips.push(trip.clone());
        Ok(trip)
    }

    fn report_row(e: &TimeEntry) -> TimeReportRow {
        TimeReportRow {
            id: e.id,
            week: e.date.iso_week().week() as i32,
            date: e.date,
            billable: e.billable,
            customer: e.customer_name.clone(),
            customer_id: e.customer_id,
            project: e.project_name.clone(),
            project_id: e.project_id,
            project_code: e.project_code.clone(),
            task: e.task_name.clone(),
            task_id: e.task_id,
            user: e.user_display_name.clone(),
            user_id: e.user_id,
            team: None,
            team_id: None,
            time: e.hours,
            notes: e.notes.clone(),
            start_end: e.start_end.clone(),
            start: e.start,
            end: e.end,
            pause: e.pause,
            amount: None,
            tags: (!e.tag_names.is_empty()).then(|| e.tag_names.join(", ")),
            status: e.status,
            client_status: e.status_extern,
            invoice_id: None,
            extra: HashMap::new(),
        }
    }

    fn mileage_row(m: &Mileage) -> MileageReportRow {
        MileageReportRow {
            id: m.id,
            week: m.date.iso_week().week() as i32,
            date: m.date,
            user: m.user_display_name.clone(),
            user_id: m.user_id,
            customer: m.customer_name.clone(),
            customer_id: m.customer_id,
            project: m.project_name.clone(),
            project_id: m.project_id,
            from_address: m.from_address.clone(),
            to_address: m.to_address.clone(),
            notes: m.notes.clone(),
            distance: m.distance,
            billable: m.billable,
            mileage_type: m.mileage_type,
            amount: Some((m.distance * data::RATE_PER_KM * 100.0).round() / 100.0),
            status: m.status,
            team: None,
            extra: HashMap::new(),
        }
    }
}

impl Backend for Mock {
    fn current_user(&self) -> Call<'_, CurrentUser> {
        self.call(|s| Ok(s.me.clone()))
    }

    fn projects_for_user(&self, _user_id: i64) -> Call<'_, Vec<Project>> {
        self.call(|s| Ok(s.projects.iter().filter(|p| p.active).cloned().collect()))
    }

    fn tags(&self, tag_type: TagType) -> Call<'_, Vec<Tag>> {
        self.call(move |s| Ok(s.tags.iter().filter(|t| t.tag_type == tag_type).cloned().collect()))
    }

    fn project_tasks(&self, project_id: i64) -> Call<'_, Vec<ProjectTask>> {
        self.call(move |s| Ok(s.tasks.iter().filter(|t| t.project_id == project_id).cloned().collect()))
    }

    fn time_entries_for_week(&self, user_id: i64, monday: NaiveDate) -> Call<'_, Vec<TimeEntry>> {
        self.call(move |s| Ok(s.entries.iter().filter(|e| e.user_id == user_id && in_week(e.date, monday)).cloned().collect()))
    }

    fn time_entry(&self, id: i64) -> Call<'_, TimeEntry> {
        self.call(move |s| s.entries.iter().find(|e| e.id == id).cloned().ok_or_else(|| not_found("time entry", id)))
    }

    fn create_time_entry<'a>(&'a self, entry: &'a NewTimeEntry) -> Call<'a, TimeEntry> {
        self.call(|s| s.add_entry(entry, Status::default()))
    }

    fn update_time_entry<'a>(&'a self, id: i64, entry: &'a NewTimeEntry) -> Call<'a, TimeEntry> {
        self.call(move |s| {
            let i = s.entries.iter().position(|e| e.id == id).ok_or_else(|| not_found("time entry", id))?;
            let old = &s.entries[i];
            let updated = s.time_entry(id, entry, Status { intern: old.status, client: old.status_extern })?;
            s.entries[i] = updated.clone();
            Ok(updated)
        })
    }

    fn delete_time_entry(&self, id: i64) -> Call<'_, ()> {
        self.call(move |s| {
            let i = s.entries.iter().position(|e| e.id == id).ok_or_else(|| not_found("time entry", id))?;
            s.entries.remove(i);
            Ok(())
        })
    }

    fn query_time(&self, user_id: i64, from: NaiveDate, to: NaiveDate, rows: Range<u32>) -> Call<'_, QueryResult<TimeReportRow>> {
        self.call(move |s| {
            let mut matching: Vec<TimeReportRow> = s
                .entries
                .iter()
                .filter(|e| e.user_id == user_id && (from..=to).contains(&e.date))
                .map(State::report_row)
                .collect();
            matching.sort_by_key(|r| (r.date, r.start));
            Ok(page(matching, rows))
        })
    }

    fn mileage_for_week(&self, user_id: i64, monday: NaiveDate) -> Call<'_, Vec<Mileage>> {
        self.call(move |s| Ok(s.trips.iter().filter(|m| m.user_id == user_id && in_week(m.date, monday)).cloned().collect()))
    }

    fn create_mileage<'a>(&'a self, mileage: &'a NewMileage) -> Call<'a, Mileage> {
        self.call(|s| s.add_trip(mileage, Status::default()))
    }

    fn update_mileage<'a>(&'a self, original: &'a Mileage, mileage: &'a NewMileage) -> Call<'a, Mileage> {
        self.call(move |s| {
            let id = original.id;
            let i = s.trips.iter().position(|m| m.id == id).ok_or_else(|| not_found("trip", id))?;
            let updated = s.trip(id, mileage, Status { intern: original.status, client: original.status_extern })?;
            s.trips[i] = updated.clone();
            Ok(updated)
        })
    }

    fn delete_mileage(&self, id: i64) -> Call<'_, ()> {
        self.call(move |s| {
            let i = s.trips.iter().position(|m| m.id == id).ok_or_else(|| not_found("trip", id))?;
            s.trips.remove(i);
            Ok(())
        })
    }

    /// Everyone's trips, as the real grid returns them.
    fn query_mileage(&self, from: NaiveDate, to: NaiveDate, rows: Range<u32>) -> Call<'_, QueryResult<MileageReportRow>> {
        self.call(move |s| {
            let mut matching: Vec<MileageReportRow> =
                s.trips.iter().filter(|m| (from..=to).contains(&m.date)).map(State::mileage_row).collect();
            matching.sort_by_key(|r| (r.date, r.id));
            Ok(page(matching, rows))
        })
    }

    /// Any text is a place; its id is the text itself, so `route_distance` can name it.
    fn place_predictions<'a>(&'a self, query: &'a str) -> Call<'a, Vec<PlacePrediction>> {
        let query = query.trim().to_owned();
        self.call(move |_| {
            let place = Some(query).filter(|q| !q.is_empty());
            Ok(place.map(|q| PlacePrediction { description: q.clone(), place_id: q }).into_iter().collect())
        })
    }

    fn route_distance<'a>(&'a self, from_place_id: &'a str, to_place_id: &'a str) -> Call<'a, RouteDistance> {
        let km = data::distance(from_place_id, to_place_id);
        let route = RouteDistance {
            origin: from_place_id.to_owned(),
            destination: to_place_id.to_owned(),
            meters: (km * 1000.0).round() as u64,
            // About 70 km/h.
            seconds: (km * 3600.0 / 70.0).round() as u64,
        };
        self.call(move |_| Ok(route))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::time::monday_of;

    use chrono::TimeZone;

    /// 14:07 in Amsterdam.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 12, 13, 7, 0).unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap()
    }

    fn block_on<T>(call: Call<'_, T>) -> T {
        futures::executor::block_on(call).unwrap()
    }

    #[test]
    fn generates_weeks_of_data_up_to_now() {
        let mock = Mock::new(now());
        let me = block_on(mock.current_user());
        let today = today();
        let week = block_on(mock.time_entries_for_week(me.id, monday_of(today)));
        assert!(!week.is_empty());
        assert!(week.iter().all(|e| e.date <= today));
        let rows = block_on(mock.query_time(me.id, today - chrono::Duration::weeks(8), today, 0..10_000));
        assert!(rows.row_count > 150, "{} rows", rows.row_count);
        assert!(rows.row_data.iter().any(|r| r.status == -1), "a rejected entry");
        assert!(rows.row_data.iter().any(|r| r.status >= 2), "approved entries");
        assert!(rows.row_data.iter().any(|r| r.notes.is_none() && r.task != "Verlof" && r.task != "Ziek"), "one without notes");
    }

    #[test]
    fn saves_updates_and_deletes_hold() {
        let mock = Mock::new(now());
        let me = block_on(mock.current_user());
        let project = block_on(mock.projects_for_user(me.id)).remove(1);
        let task = block_on(mock.project_tasks(project.id)).remove(0);
        let day = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        let new = NewTimeEntry::new(me.id, &project, &task, day, 2.0).with_notes("Mock test");
        let created = block_on(mock.create_time_entry(&new));
        assert_eq!((created.project_name.as_str(), created.notes.as_deref()), (project.name.as_str(), Some("Mock test")));

        let changed = NewTimeEntry { hours: 3.0, ..new };
        block_on(mock.update_time_entry(created.id, &changed));
        assert_eq!(block_on(mock.time_entry(created.id)).hours, 3.0);
        let week = block_on(mock.time_entries_for_week(me.id, monday_of(day)));
        assert_eq!(week.iter().filter(|e| e.id == created.id).count(), 1);

        block_on(mock.delete_time_entry(created.id));
        assert!(futures::executor::block_on(mock.time_entry(created.id)).is_err());
    }

    #[test]
    fn report_grids_page() {
        let mock = Mock::new(now());
        let me = block_on(mock.current_user());
        let today = today();
        let all = block_on(mock.query_time(me.id, today - chrono::Duration::weeks(4), today, 0..10_000));
        let second = block_on(mock.query_time(me.id, today - chrono::Duration::weeks(4), today, 10..20));
        assert_eq!(second.row_count, all.row_count);
        assert_eq!(
            second.row_data.iter().map(|r| r.id).collect::<Vec<_>>(),
            all.row_data[10..20].iter().map(|r| r.id).collect::<Vec<_>>()
        );
        let trips = block_on(mock.query_mileage(today - chrono::Duration::weeks(8), today, 0..10_000));
        assert!(trips.row_data.iter().any(|r| r.user_id != me.id), "the grid holds a colleague's trips too");
    }

    #[test]
    fn its_config_loads_with_presets_on_its_projects() {
        let dir = std::env::temp_dir().join(format!("rechimped-mock-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, config()).unwrap();
        let loaded = crate::store::config::load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        let config = loaded.unwrap().expect("a config");

        assert!(config.settings.travel_reminder);
        assert_eq!((config.trips.len(), config.time_entries.len()), (2, 2));
        let mock = Mock::new(now());
        let projects = block_on(mock.projects_for_user(0));
        let known = |name: &str| projects.iter().any(|p| p.name == name);
        assert!(config.trips.iter().all(|t| known(&t.project)));
        assert!(config.time_entries.iter().all(|t| known(&t.project)));
    }
}
