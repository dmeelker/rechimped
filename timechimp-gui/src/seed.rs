//! What the forms start out with, as plain data: which project, task, times and tags a time form
//! shows for a new, edited, copied or prefilled entry, and what a trip form shows. `app` builds
//! the gpuikit entities from these; working them out needs no window, so it is unit-tested here.

use chrono::NaiveDate;
use timechimp::{Mileage, MileageType, TimeEntry};

use crate::session::Session;
use crate::store::config::TripPreset;
use crate::validate::TimeKind;

/// A tag checkbox of the time form.
#[derive(Debug, Clone, PartialEq)]
pub struct TagSeed {
    pub id: i64,
    pub name: String,
    pub checked: bool,
}

/// The parts of the time form that follow the project: its tasks and tag checkboxes, and the tags
/// it keeps without showing them (`Session::kept_tags`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSeed {
    pub tasks: Vec<(i64, String)>,
    /// None: the first task.
    pub task_id: Option<i64>,
    pub tags: Vec<TagSeed>,
    pub kept_tag_ids: Vec<i64>,
}

/// A time form as it opens.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeSeed {
    pub kind: TimeKind,
    pub date: NaiveDate,
    pub projects: Vec<(i64, String)>,
    pub project_id: Option<i64>,
    pub project: ProjectSeed,
    pub start: String,
    pub end: String,
    pub hours: String,
    pub notes: String,
    /// Whether to offer the Recent picker (a new entry, with recent work to pick from).
    pub recent: bool,
}

/// What filling in a time form from a recent item or a preset puts in it; date and times stay.
pub struct TimeFill {
    pub project_id: i64,
    pub project_task_id: Option<i64>,
    pub notes: String,
    pub tag_ids: Vec<i64>,
}

/// The time form after a `TimeFill`.
#[derive(Debug, Clone, PartialEq)]
pub struct FillSeed {
    pub projects: Vec<(i64, String)>,
    pub project_id: i64,
    pub project: ProjectSeed,
    pub notes: String,
}

/// A trip form as it opens.
#[derive(Debug, Clone, PartialEq)]
pub struct TripSeed {
    pub date: NaiveDate,
    pub projects: Vec<(i64, String)>,
    pub project_id: Option<i64>,
    pub from: String,
    pub to: String,
    pub distance: String,
    pub notes: String,
    pub return_trip: bool,
    pub mileage_type: Option<MileageType>,
    pub billable: bool,
    /// What From and To suggest (`Session::saved_addresses`).
    pub addresses: Vec<String>,
}

/// Checkboxes for the tags configured for `project_id`, checked where `checked` has the tag.
pub fn tags(session: &Session, project_id: Option<i64>, checked: &[i64]) -> Vec<TagSeed> {
    let configured = project_id.and_then(|id| session.project_tags.get(&id)).map(Vec::as_slice).unwrap_or_default();
    configured.iter().map(|t| TagSeed { id: t.id, name: t.name.clone(), checked: checked.contains(&t.id) }).collect()
}

/// The time form for a new entry, for editing `entry`, or (with `copy`) for a copy of it.
pub fn time_form(session: &Session, entry: Option<&TimeEntry>, copy: bool) -> TimeSeed {
    let projects = session.project_options(entry.map(|e| (e.project_id, &*e.project_name, &*e.customer_name)));
    // New entries default to the project/task/tag of the most recently modified entry.
    let template = entry.or_else(|| session.entries().iter().max_by_key(|e| e.modified));
    let project_id = template.map(|e| e.project_id).or(projects.first().map(|(id, _)| *id));
    let project = ProjectSeed {
        tasks: session.task_options(project_id, entry),
        task_id: template.map(|e| e.project_task_id),
        tags: tags(session, project_id, template.map_or(&[], |e| &e.tag_ids)),
        kept_tag_ids: entry.map(|e| session.kept_tags(project_id, &e.tag_ids)).unwrap_or_default(),
    };

    let editing = entry.filter(|_| !copy);
    let date = editing.map(|e| e.date).unwrap_or(session.day());
    let (start, end) = editing
        .and_then(|e| e.start_end.as_deref())
        .and_then(|s| s.split_once('-'))
        .map(|(s, e)| (s.to_owned(), e.to_owned()))
        .unwrap_or_else(|| (session.next_start_time(date).unwrap_or_default(), String::new()));
    TimeSeed {
        kind: match editing {
            None => TimeKind::New,
            Some(e) => TimeKind::Edit {
                id: e.id,
                pause: e.pause,
                timezone: e.timezone.clone(),
                task: (e.project_task_id, e.task_id),
            },
        },
        date,
        projects,
        project_id,
        project,
        start,
        end,
        hours: editing.map(|e| format!("{}", e.hours)).unwrap_or_default(),
        notes: entry.and_then(|e| e.notes.clone()).unwrap_or_default(),
        recent: entry.is_none() && !session.recent.is_empty(),
    }
}

/// The open time form filled with `fill` (`from`: the entry it comes from, so its project and task
/// are offered even when no longer active).
pub fn time_fill(session: &Session, fill: TimeFill, from: Option<&TimeEntry>) -> FillSeed {
    let project_id = Some(fill.project_id);
    FillSeed {
        projects: session.project_options(from.map(|e| (e.project_id, &*e.project_name, &*e.customer_name))),
        project_id: fill.project_id,
        project: ProjectSeed {
            tasks: session.task_options(project_id, from),
            task_id: fill.project_task_id,
            tags: tags(session, project_id, &fill.tag_ids),
            kept_tag_ids: session.kept_tags(project_id, &fill.tag_ids),
        },
        notes: fill.notes,
    }
}

/// The time form's tasks and tags after switching to `project_id`, given the tags it kept so far.
pub fn project_changed(session: &Session, project_id: Option<i64>, kept_tag_ids: &[i64]) -> ProjectSeed {
    // Pre-check the tags the project's most recently modified entry had, and the kept tags this
    // project has a checkbox for; the rest stays kept.
    let last = session.entries().iter().filter(|e| Some(e.project_id) == project_id).max_by_key(|e| e.modified);
    let mut checked: Vec<i64> = last.map_or(&[][..], |e| &e.tag_ids).to_vec();
    checked.extend(kept_tag_ids);
    ProjectSeed {
        tasks: session.task_options(project_id, None),
        task_id: None,
        tags: tags(session, project_id, &checked),
        kept_tag_ids: session.kept_tags(project_id, kept_tag_ids),
    }
}

/// The trip form: for a new trip filled in from `preset`, or empty (project, type and billable
/// from the last trip); or filled in with `editing`.
pub fn trip_form(session: &Session, editing: Option<&Mileage>, preset: Option<&TripPreset>) -> TripSeed {
    let template = editing.or(session.mileage().last());
    let addresses = session.saved_addresses();
    let projects = session.project_options(editing.map(|m| (m.project_id, &*m.project_name, &*m.customer_name)));
    let (mut project_id, mut mileage_type, mut billable, mut return_trip) = (
        template.map(|m| m.project_id),
        template.map(|m| m.mileage_type),
        template.is_some_and(|m| m.billable),
        false,
    );
    if let Some(p) = preset {
        project_id = session.project_named(&p.project).map(|p| p.id).or(project_id);
        mileage_type = p.trip_type();
        billable = p.billable;
        return_trip = p.return_trip;
    }
    let (date, from, to, distance, notes) = match (editing, preset) {
        (None, Some(p)) => (
            session.day(),
            p.from.clone(),
            p.to.clone(),
            p.distance.map(|d| format!("{d}")).unwrap_or_default(),
            p.notes.clone().unwrap_or_default(),
        ),
        (Some(m), _) => (
            m.date,
            m.from_address.clone().unwrap_or_default(),
            m.to_address.clone().unwrap_or_default(),
            format!("{}", m.distance),
            m.notes.clone().unwrap_or_default(),
        ),
        (None, None) => (
            session.day(),
            addresses.first().cloned().unwrap_or_default(),
            addresses.get(1).cloned().unwrap_or_default(),
            String::new(),
            String::new(),
        ),
    };
    TripSeed { date, projects, project_id, from, to, distance, notes, return_trip, mileage_type, billable, addresses }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::session::Base;
    use crate::store::config::TripPreset;
    use crate::test_support::*;

    /// Projects 3 ("P", task 4, tag 10) and 6 ("Other", task 7, tag 20). On Wednesday the 4th:
    /// entry 1 on project 3 at 09:00-10:00 with tags 10, 20 (another project's) and 99 (no
    /// project's), modified last; entry 2 on project 6 at 10:00-11:30 with tag 20; one trip.
    fn session() -> Session {
        let mut s = Session::new(None, date(4));
        let mut me = user();
        me.mileage_addresses = vec!["Home".into(), "Office".into()];
        let tasks = HashMap::from([(3, vec![task(4, 3, "Dev")]), (6, vec![task(7, 6, "Ops")])]);
        s.apply_base(Base { me, ..base(vec![project(3, "P"), project(6, "Other")], tasks, Vec::new()) }, None, &[]);
        s.project_tags = HashMap::from([(3, vec![tag(10, "a")]), (6, vec![tag(20, "b")])]);
        let mut first = entry(1, 4, "09:00-10:00");
        first.tag_ids = vec![10, 20, 99];
        first.modified = date(3).and_hms_opt(12, 0, 0).unwrap();
        let mut second = entry(2, 4, "10:00-11:30");
        (second.project_id, second.project_task_id, second.tag_ids) = (6, 7, vec![20]);
        s.apply_week(crate::session::Week { start: date(2), entries: vec![first, second], mileage: vec![trip(5, 3)] }, None);
        s
    }

    fn checked(tags: &[TagSeed]) -> Vec<(i64, bool)> {
        tags.iter().map(|t| (t.id, t.checked)).collect()
    }

    #[test]
    fn new_time_entry_follows_the_last_modified_one() {
        let s = session();
        let seed = time_form(&s, None, false);
        assert_eq!(seed.kind, TimeKind::New);
        assert_eq!((seed.date, seed.project_id, seed.project.task_id), (date(4), Some(3), Some(4)));
        assert_eq!((seed.start.as_str(), seed.end.as_str(), seed.hours.as_str()), ("11:30", "", ""), "continues the day");
        assert_eq!(checked(&seed.project.tags), [(10, true)]);
        assert!(seed.project.kept_tag_ids.is_empty(), "a new entry keeps nothing it doesn't show");
        assert!(seed.recent);
    }

    #[test]
    fn edited_entry_keeps_tags_without_a_checkbox() {
        let s = session();
        let first = s.entries()[0].clone();
        let seed = time_form(&s, Some(&first), false);
        assert!(matches!(seed.kind, TimeKind::Edit { id: 1, .. }));
        assert_eq!((seed.start.as_str(), seed.end.as_str(), seed.hours.as_str()), ("09:00", "10:00", "1"));
        assert_eq!(checked(&seed.project.tags), [(10, true)]);
        // Tag 20 is configured, but for project 6 only: it used to get lost here.
        assert_eq!(seed.project.kept_tag_ids, [20, 99]);
        assert!(!seed.recent);

        let copy = time_form(&s, Some(&first), true);
        assert_eq!(copy.kind, TimeKind::New);
        assert_eq!((copy.start.as_str(), copy.hours.as_str()), ("11:30", ""));
        assert_eq!(copy.project.kept_tag_ids, [20, 99]);
    }

    #[test]
    fn changing_the_project_moves_tags_between_checkboxes_and_kept() {
        let s = session();
        let seed = project_changed(&s, Some(6), &[20, 99]);
        assert_eq!(seed.tasks, [(7, "Ops".to_owned())]);
        assert_eq!(seed.task_id, None);
        assert_eq!(checked(&seed.tags), [(20, true)]);
        assert_eq!(seed.kept_tag_ids, [99]);
    }

    #[test]
    fn filling_in_keeps_what_the_project_does_not_show() {
        let s = session();
        let fill = TimeFill { project_id: 6, project_task_id: Some(7), notes: "Deploy".into(), tag_ids: vec![10, 20] };
        let seed = time_fill(&s, fill, None);
        assert_eq!((seed.project_id, seed.project.task_id, seed.notes.as_str()), (6, Some(7), "Deploy"));
        assert_eq!(checked(&seed.project.tags), [(20, true)]);
        assert_eq!(seed.project.kept_tag_ids, [10]);
    }

    #[test]
    fn trip_forms() {
        let s = session();
        let new = trip_form(&s, None, None);
        assert_eq!((new.from.as_str(), new.to.as_str()), ("Home", "Office"), "the first saved addresses");
        assert_eq!((new.date, new.project_id, new.mileage_type, new.billable), (date(4), Some(3), Some(MileageType::Business), true));
        assert_eq!(new.addresses, ["Home", "Office", "A", "B"]);

        let preset = TripPreset {
            name: "Commute".into(),
            project: "other".into(),
            from: "X".into(),
            to: "Y".into(),
            distance: Some(23.5),
            return_trip: true,
            mileage_type: Some("home-work".into()),
            billable: false,
            notes: None,
        };
        let from_preset = trip_form(&s, None, Some(&preset));
        assert_eq!((from_preset.from.as_str(), from_preset.to.as_str(), from_preset.distance.as_str()), ("X", "Y", "23.5"));
        assert_eq!(
            (from_preset.project_id, from_preset.mileage_type, from_preset.billable, from_preset.return_trip),
            (Some(6), Some(MileageType::HomeWork), false, true)
        );

        let edited = trip_form(&s, Some(&s.mileage()[0]), None);
        assert_eq!((edited.date, edited.from.as_str(), edited.distance.as_str()), (date(3), "A", "10"));
    }
}
