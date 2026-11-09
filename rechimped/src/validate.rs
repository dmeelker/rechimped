//! Works on plain snapshots of the forms, so it needs no window and is unit-tested.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use timechimp_api::{Mileage, MileageType, NewMileage, NewTimeEntry};

use crate::domain::same_name;
use crate::domain::time::{local, parse_decimal, parse_end, parse_time};
use crate::session::Session;
use crate::store::config::{self, Settings, TimePreset, TripPreset};

#[derive(Clone, Debug, PartialEq)]
pub enum TimeKind {
    New,
    /// Carries what the form doesn't edit, so an update doesn't drop it.
    Edit { id: i64, pause: f64, timezone: Option<String>, task: (i64, i64) },
}

#[derive(Clone, Debug)]
pub struct TimeValues {
    pub kind: TimeKind,
    pub date: String,
    pub project_id: Option<i64>,
    pub project_task_id: Option<i64>,
    pub start: String,
    pub end: String,
    pub hours: String,
    pub notes: String,
    /// Includes the kept tags the form has no checkbox for.
    pub tag_ids: Vec<i64>,
}

#[derive(Clone, Debug)]
pub struct TravelValues {
    pub editing: Option<Box<Mileage>>,
    pub date: String,
    pub project_id: Option<i64>,
    pub from: String,
    pub to: String,
    pub distance: String,
    pub return_trip: bool,
    pub travel_type: Option<MileageType>,
    pub billable: bool,
    pub notes: String,
}

pub enum SaveRequest {
    Time { id: Option<i64>, entry: NewTimeEntry },
    Travel { trip: NewMileage, return_trip: bool, editing: Option<Box<Mileage>> },
}

fn date_and_customer(date: &str, project_id: Option<i64>, session: &Session) -> Result<(NaiveDate, i64, i64), String> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| "Date must be YYYY-MM-DD")?;
    let project_id = project_id.ok_or("Pick a project")?;
    let customer_id = session
        .project(project_id)
        .map(|p| p.customer_id)
        .or(session.entries().iter().find(|e| e.project_id == project_id).map(|e| e.customer_id))
        .ok_or("Unknown project")?;
    Ok((date, project_id, customer_id))
}

fn user_id(session: &Session) -> Result<i64, String> {
    Ok(session.me().ok_or("Not logged in")?.id)
}

fn optional(text: &str) -> Option<String> {
    Some(text.to_owned()).filter(|t| !t.is_empty())
}

pub fn time_entry(v: &TimeValues, session: &Session) -> Result<(NaiveDate, SaveRequest), String> {
    let user_id = user_id(session)?;
    let (date, project_id, customer_id) = date_and_customer(&v.date, v.project_id, session)?;
    let project_task_id = v.project_task_id.ok_or("Pick a task")?;
    let task_id = session
        .project_tasks(project_id)
        .iter()
        .find(|t| t.id == project_task_id)
        .map(|t| t.task_id)
        // An edited entry's task may no longer be listed; it keeps its own.
        .or(match &v.kind {
            TimeKind::Edit { task: (pt, t), .. } if *pt == project_task_id => Some(*t),
            _ => None,
        })
        .ok_or("Unknown task")?;

    // An edited entry's times are read on its own wall clock, not the account's.
    let (pause, own_timezone) = match &v.kind {
        TimeKind::Edit { pause, timezone, .. } => (*pause, timezone.as_deref()),
        TimeKind::New => (0.0, None),
    };
    let tz = session.timezone(own_timezone);
    let entry = NewTimeEntry {
        user_id,
        customer_id,
        project_id,
        project_task_id,
        task_id,
        date,
        hours: 0.0,
        notes: optional(&v.notes),
        start: None,
        end: None,
        pause,
        tag_ids: v.tag_ids.clone(),
        timezone: Some(tz.name().to_owned()),
    };

    let start = parse_time(&v.start)?;
    let end = parse_end(start, &v.end)?;
    let entry = match (start, end) {
        (Some(start), Some(end)) => {
            if end <= start {
                return Err("End must be after start".into());
            }
            entry.with_times(local(date, start, tz)?, local(date, end, tz)?)
        }
        (None, None) => {
            let hours = parse_decimal(&v.hours).ok_or("Enter start and end, or a number of hours")?;
            if hours <= 0.0 {
                return Err("Hours must be positive".into());
            }
            NewTimeEntry { hours, ..entry }
        }
        _ => return Err("Fill in both start and end, or neither".into()),
    };
    let id = match &v.kind {
        TimeKind::Edit { id, .. } => Some(*id),
        TimeKind::New => None,
    };
    Ok((date, SaveRequest::Time { id, entry }))
}

pub fn trip(v: &TravelValues, session: &Session) -> Result<(NaiveDate, SaveRequest), String> {
    let user_id = user_id(session)?;
    let (date, project_id, customer_id) = date_and_customer(&v.date, v.project_id, session)?;
    let distance = parse_decimal(&v.distance).ok_or("Distance must be a number (Ctrl+D calculates it)")?;
    if v.from.is_empty() || v.to.is_empty() {
        return Err("Fill in both addresses".into());
    }
    let trip = NewMileage {
        user_id,
        customer_id,
        project_id,
        date,
        distance,
        from_address: v.from.clone(),
        to_address: v.to.clone(),
        notes: optional(&v.notes),
        billable: v.billable,
        mileage_type: v.travel_type.unwrap_or(MileageType::Business),
        mileage_vehicle_id: v.editing.as_ref().and_then(|m| m.mileage_vehicle_id),
    };
    // Editing changes one trip; the return switch only applies to new ones.
    let return_trip = v.editing.is_none() && v.return_trip;
    Ok((date, SaveRequest::Travel { trip, return_trip, editing: v.editing.clone() }))
}

pub fn preset(v: &TravelValues, session: &Session) -> Result<TripPreset, String> {
    if v.from.is_empty() || v.to.is_empty() {
        return Err("Fill in both addresses first".into());
    }
    let project = v.project_id.and_then(|id| session.project(id)).ok_or("Pick a project first")?;
    let distance = match parse_decimal(&v.distance) {
        Some(km) => Some(km),
        None if v.distance.is_empty() => None,
        None => return Err("Distance must be a number (or empty)".into()),
    };
    let name = config::preset_name(&v.from, &v.to, &v.notes);
    if session.trip_presets.iter().any(|p| same_name(&p.name, &name)) {
        return Err(format!("There already is a preset \"{name}\" in config.toml"));
    }
    Ok(TripPreset {
        name,
        project: project.name.clone(),
        from: v.from.clone(),
        to: v.to.clone(),
        distance,
        return_trip: v.editing.is_none() && v.return_trip,
        travel_type: v.travel_type.and_then(config::travel_type_key),
        billable: v.billable,
        notes: optional(&v.notes),
    })
}

pub fn time_preset(v: &TimeValues, session: &Session) -> Result<TimePreset, String> {
    let project = v.project_id.and_then(|id| session.project(id)).ok_or("Pick a project first")?;
    let task = v
        .project_task_id
        .and_then(|id| session.project_tasks(project.id).iter().find(|t| t.id == id))
        .ok_or("Pick a task first")?;
    let preset = TimePreset {
        project: project.name.clone(),
        task: Some(task.name.clone()),
        notes: optional(&v.notes),
        tags: v.tag_ids.iter().filter_map(|id| session.tags().iter().find(|t| t.id == *id)).map(|t| t.name.clone()).collect(),
    };
    let same = |p: &TimePreset| {
        same_name(&p.project, &preset.project)
            && p.task.as_deref().is_some_and(|t| same_name(t, &task.name))
            && p.notes.as_deref().map(str::trim) == preset.notes.as_deref()
    };
    if session.time_presets.iter().any(same) {
        return Err("There already is a preset for this project, task and description in config.toml".into());
    }
    Ok(preset)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsValues {
    pub work_days: [bool; 7],
    pub billable_goal: String,
    pub hidden_projects: Vec<String>,
    pub hidden_tasks: BTreeMap<String, Vec<String>>,
}

pub fn settings(v: &SettingsValues) -> Result<Settings, String> {
    if !v.work_days.contains(&true) {
        return Err("Pick at least one work day".into());
    }
    let billable_goal = parse_decimal(v.billable_goal.trim_end_matches('%'))
        .filter(|p| (0.0..=100.0).contains(p))
        .ok_or("Billable target must be a percentage between 0 and 100")?;
    Ok(Settings {
        work_days: v.work_days,
        billable_goal: billable_goal / 100.0,
        hidden_projects: v.hidden_projects.clone(),
        hidden_tasks: v.hidden_tasks.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::test_support::{base, date, project, tag, task};

    fn session() -> Session {
        let mut s = Session::new(None, date(4));
        s.apply_base(base(vec![project(3, "Intern")], HashMap::from([(3, vec![task(4, 3, "Dev")])]), vec![tag(7, "Daily")]), None, &[]);
        s
    }

    fn time(start: &str, end: &str, hours: &str) -> TimeValues {
        TimeValues {
            kind: TimeKind::New,
            date: "2026-11-04".into(),
            project_id: Some(3),
            project_task_id: Some(4),
            start: start.into(),
            end: end.into(),
            hours: hours.into(),
            notes: String::new(),
            tag_ids: vec![7],
        }
    }

    fn entry(v: &TimeValues) -> Result<NewTimeEntry, String> {
        match time_entry(v, &session())? {
            (_, SaveRequest::Time { entry, .. }) => Ok(entry),
            _ => unreachable!(),
        }
    }

    #[test]
    fn time_entries() {
        let e = entry(&time("9", "+1h30", "")).unwrap();
        assert_eq!((e.customer_id, e.task_id, e.effective_hours()), (2, 5, 1.5));
        assert_eq!(e.start.unwrap().to_rfc3339(), "2026-11-04T09:00:00+01:00");
        assert_eq!(e.timezone.as_deref(), Some("Europe/Amsterdam"));
        assert_eq!(e.notes, None);
        assert_eq!(e.tag_ids, [7]);

        assert_eq!(entry(&time("", "", "2,5")).unwrap().hours, 2.5);
        assert_eq!(entry(&time("10", "9", "")).unwrap_err(), "End must be after start");
        assert_eq!(entry(&time("9", "", "")).unwrap_err(), "Fill in both start and end, or neither");
        assert_eq!(entry(&time("", "", "")).unwrap_err(), "Enter start and end, or a number of hours");
        assert_eq!(entry(&time("", "", "0")).unwrap_err(), "Hours must be positive");
        assert_eq!(entry(&TimeValues { date: "4-11".into(), ..time("9", "10", "") }).unwrap_err(), "Date must be YYYY-MM-DD");
        assert_eq!(entry(&TimeValues { project_id: None, ..time("9", "10", "") }).unwrap_err(), "Pick a project");
        assert_eq!(entry(&TimeValues { project_id: Some(99), ..time("9", "10", "") }).unwrap_err(), "Unknown project");
        assert_eq!(entry(&TimeValues { project_task_id: Some(99), ..time("9", "10", "") }).unwrap_err(), "Unknown task");
    }

    #[test]
    fn edited_entries_keep_what_the_form_does_not_show() {
        let kind = TimeKind::Edit { id: 11, pause: 0.5, timezone: Some("Europe/London".into()), task: (99, 98) };
        let v = TimeValues { kind, project_task_id: Some(99), ..time("9", "11", "") };
        let (_, SaveRequest::Time { id, entry }) = time_entry(&v, &session()).unwrap() else { unreachable!() };
        assert_eq!(id, Some(11));
        assert_eq!(entry.task_id, 98, "a task no longer listed stays");
        assert_eq!(entry.effective_hours(), 1.5, "minus the pause");
        assert_eq!(entry.start.unwrap().to_rfc3339(), "2026-11-04T09:00:00+00:00", "on the entry's own clock");
    }

    fn trip_values() -> TravelValues {
        TravelValues {
            editing: None,
            date: "2026-11-04".into(),
            project_id: Some(3),
            from: "Home 1, Town".into(),
            to: "Office 2, City".into(),
            distance: "23,5".into(),
            return_trip: true,
            travel_type: Some(MileageType::HomeWork),
            billable: false,
            notes: String::new(),
        }
    }

    #[test]
    fn trips() {
        let (date, SaveRequest::Travel { trip: t, return_trip, editing }) = trip(&trip_values(), &session()).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 11, 4).unwrap());
        assert_eq!((t.distance, t.customer_id, t.mileage_type), (23.5, 2, MileageType::HomeWork));
        assert!(return_trip && editing.is_none());

        let no_distance = TravelValues { distance: String::new(), ..trip_values() };
        assert_eq!(trip(&no_distance, &session()).err().unwrap(), "Distance must be a number (Ctrl+D calculates it)");
        let no_address = TravelValues { to: String::new(), ..trip_values() };
        assert_eq!(trip(&no_address, &session()).err().unwrap(), "Fill in both addresses");
    }

    #[test]
    fn presets() {
        let mut s = session();
        let p = preset(&TravelValues { distance: String::new(), ..trip_values() }, &s).unwrap();
        assert_eq!(p.name, "Home 1 → Office 2");
        assert_eq!((p.project.as_str(), p.distance, p.return_trip), ("Intern", None, true));
        assert_eq!(p.travel_type.as_deref(), Some("home-work"));

        s.trip_presets.push(p);
        assert!(preset(&trip_values(), &s).unwrap_err().starts_with("There already is a preset"));
        let bad = TravelValues { distance: "far".into(), notes: "Other".into(), ..trip_values() };
        assert_eq!(preset(&bad, &s).unwrap_err(), "Distance must be a number (or empty)");
        let no_project = TravelValues { project_id: None, notes: "Other".into(), ..trip_values() };
        assert_eq!(preset(&no_project, &s).unwrap_err(), "Pick a project first");
    }

    #[test]
    fn time_presets() {
        let mut s = session();
        let v = TimeValues { notes: "Standup".into(), ..time("9", "10", "") };
        let p = time_preset(&v, &s).unwrap();
        assert_eq!(
            p,
            TimePreset { project: "Intern".into(), task: Some("Dev".into()), notes: Some("Standup".into()), tags: vec!["Daily".into()] }
        );
        s.time_presets.push(p);
        assert!(time_preset(&v, &s).unwrap_err().starts_with("There already is a preset"));
        assert!(time_preset(&TimeValues { notes: "Other".into(), ..v.clone() }, &s).is_ok());
        assert_eq!(time_preset(&TimeValues { project_task_id: None, ..v }, &s).unwrap_err(), "Pick a task first");
    }

    #[test]
    fn settings_values() {
        let v = |days: [bool; 7], goal: &str| SettingsValues {
            work_days: days,
            billable_goal: goal.into(),
            hidden_projects: vec!["Old".into()],
            hidden_tasks: BTreeMap::from([("P".into(), vec!["T".into()])]),
        };
        let weekdays = [true, true, true, true, true, false, false];
        assert_eq!(
            settings(&v(weekdays, "75 %")),
            Ok(Settings {
                work_days: weekdays,
                billable_goal: 0.75,
                hidden_projects: vec!["Old".into()],
                hidden_tasks: BTreeMap::from([("P".into(), vec!["T".into()])]),
            })
        );
        assert_eq!(settings(&v(weekdays, "82,5")).unwrap().billable_goal, 0.825);
        assert!(settings(&v([false; 7], "80")).unwrap_err().contains("work day"));
        assert!(settings(&v(weekdays, "101")).unwrap_err().starts_with("Billable"));
        assert!(settings(&v(weekdays, "lots")).unwrap_err().starts_with("Billable"));
    }
}
