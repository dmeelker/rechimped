use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use timechimp_api::{Mileage, MileageType, Tag, TimeEntry};

use crate::domain::{self, same_name};
use crate::session::Session;
use crate::store::config::TripPreset;
use crate::validate::TimeKind;

#[derive(Debug, Clone, PartialEq)]
pub struct TagSeed {
    pub id: i64,
    pub name: String,
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectSeed {
    pub tasks: Vec<(i64, String)>,
    pub task_id: Option<i64>,
    pub tags: Vec<TagSeed>,
    pub kept_tag_ids: Vec<i64>,
}

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
    pub recent: bool,
}

pub struct TimeFill {
    pub project_id: i64,
    pub project_task_id: Option<i64>,
    pub notes: String,
    pub tag_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FillSeed {
    pub projects: Vec<(i64, String)>,
    pub project_id: i64,
    pub project: ProjectSeed,
    pub notes: String,
}

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
    pub travel_type: Option<MileageType>,
    pub billable: bool,
    pub addresses: Vec<String>,
}

pub fn is_hidden(session: &Session, project_id: i64) -> bool {
    let hidden = &session.config().settings().hidden_projects;
    session.project(project_id).is_some_and(|p| hidden.iter().any(|name| same_name(name, &p.name)))
}

pub fn project_options(session: &Session, current: Option<(i64, &str, &str)>) -> Vec<(i64, String)> {
    let mut options: Vec<(i64, String)> = session
        .projects()
        .iter()
        .filter(|p| p.active && !is_hidden(session, p.id))
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
pub fn is_task_hidden(session: &Session, project_id: i64, task_name: &str) -> bool {
    session.project(project_id).is_some_and(|p| {
        session
            .config()
            .settings()
            .hidden_tasks
            .iter()
            .any(|(project, tasks)| same_name(project, &p.name) && tasks.iter().any(|t| same_name(t, task_name)))
    })
}

/// `entry`'s task is offered even if hidden or inactive, and `keep` (a task chosen elsewhere,
/// as by a preset) even if hidden.
pub fn task_options(
    session: &Session,
    project_id: Option<i64>,
    entry: Option<&TimeEntry>,
    keep: Option<i64>,
) -> Vec<(i64, String)> {
    let mut tasks: Vec<(i64, String)> = project_id
        .map(|id| {
            session
                .project_tasks(id)
                .iter()
                .filter(|t| t.active && (Some(t.id) == keep || !is_task_hidden(session, id, &t.name)))
                .map(|t| (t.id, t.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    if let Some(e) = entry.filter(|e| Some(e.project_id) == project_id)
        && !tasks.iter().any(|(id, _)| *id == e.project_task_id)
    {
        tasks.push((e.project_task_id, e.task_name.clone()));
    }
    tasks
}

/// The tags `project_id` offers: those configured for it, or all of them (but the `z_` ones)
/// until no project has any configured.
pub fn tags_for(session: &Session, project_id: Option<i64>) -> Vec<&Tag> {
    let Some(project_id) = project_id else { return Vec::new() };
    let config = session.config();
    if config.tags_configured() {
        config.project_tags().get(&project_id).map(|tags| tags.iter().collect()).unwrap_or_default()
    } else {
        session.tags().iter().filter(|t| !t.name.starts_with("z_")).collect()
    }
}

/// The time form has no checkbox for these, so it leaves them untouched.
pub fn kept_tags(session: &Session, project_id: Option<i64>, tag_ids: &[i64]) -> Vec<i64> {
    let offered = tags_for(session, project_id);
    tag_ids.iter().copied().filter(|id| !offered.iter().any(|t| t.id == *id)).collect()
}

/// The account's saved addresses, then those of the week's trips.
pub fn saved_addresses(session: &Session) -> Vec<String> {
    let mut addresses: Vec<String> = session.me().map(|m| m.mileage_addresses.clone()).unwrap_or_default();
    for m in session.week().travel() {
        for a in [&m.from_address, &m.to_address].into_iter().flatten() {
            if !addresses.contains(a) {
                addresses.push(a.clone());
            }
        }
    }
    addresses
}

pub fn tags(session: &Session, project_id: Option<i64>, checked: &[i64]) -> Vec<TagSeed> {
    tags_for(session, project_id)
        .into_iter()
        .map(|t| TagSeed { id: t.id, name: t.name.clone(), checked: checked.contains(&t.id) })
        .collect()
}

/// A new or copied entry is dated on `date`. `now` is the wall-clock time in the user's timezone:
/// a new entry for today that follows an earlier one ends at now, filling the gap since.
pub fn time_form(session: &Session, entry: Option<&TimeEntry>, copy: bool, date: NaiveDate, now: NaiveDateTime) -> TimeSeed {
    let projects = project_options(session, entry.map(|e| (e.project_id, &*e.project_name, &*e.customer_name)));
    // Only an offered project (not hidden or inactive) may become the default.
    let template = entry.or_else(|| {
        session.week().entries().iter().filter(|e| projects.iter().any(|(id, _)| *id == e.project_id)).max_by_key(|e| e.modified)
    });
    let project_id = template.map(|e| e.project_id).or(projects.first().map(|(id, _)| *id));
    let project = ProjectSeed {
        tasks: task_options(session, project_id, entry, None),
        task_id: template.map(|e| e.project_task_id),
        tags: tags(session, project_id, template.map_or(&[], |e| &e.tag_ids)),
        kept_tag_ids: entry.map(|e| kept_tags(session, project_id, &e.tag_ids)).unwrap_or_default(),
    };

    let editing = entry.filter(|_| !copy);
    let date = editing.map(|e| e.date).unwrap_or(date);
    let (start, end) = editing
        .and_then(|e| e.start_end.as_deref())
        .and_then(|s| s.split_once('-'))
        .map(|(s, e)| (s.to_owned(), e.to_owned()))
        .unwrap_or_else(|| new_slot(session, date, now));
    TimeSeed {
        kind: match editing {
            None => TimeKind::New,
            Some(e) => {
                TimeKind::Edit { id: e.id, pause: e.pause, timezone: e.timezone.clone(), task: (e.project_task_id, e.task_id) }
            }
        },
        date,
        projects,
        project_id,
        project,
        start,
        end,
        hours: editing.map(|e| domain::hh_mm(e.hours)).unwrap_or_default(),
        notes: entry.and_then(|e| e.notes.clone()).unwrap_or_default(),
        recent: entry.is_none() && !session.recent().is_empty(),
    }
}

/// Start where the day's last entry ended; on today, end now (to the quarter) if that is later.
fn new_slot(session: &Session, date: NaiveDate, now: NaiveDateTime) -> (String, String) {
    let Some(start) = session.next_start_time(date) else { return Default::default() };
    let end = domain::time::round_minutes(now.time(), 15);
    let fills_gap = date == now.date() && domain::time::parse_time(&start).ok().flatten().is_some_and(|s| s < end);
    (start, if fills_gap { end.format("%H:%M").to_string() } else { String::new() })
}

/// A new entry filling the gap `from`–`to` on `date`.
pub fn gap_form(session: &Session, date: NaiveDate, from: NaiveTime, to: NaiveTime, now: NaiveDateTime) -> TimeSeed {
    TimeSeed {
        start: from.format("%H:%M").to_string(),
        end: to.format("%H:%M").to_string(),
        ..time_form(session, None, false, date, now)
    }
}

/// `from` is the entry the fill comes from, so its project and task are offered even when no
/// longer active.
pub fn time_fill(session: &Session, fill: TimeFill, from: Option<&TimeEntry>) -> FillSeed {
    let project_id = Some(fill.project_id);
    FillSeed {
        projects: project_options(session, from.map(|e| (e.project_id, &*e.project_name, &*e.customer_name))),
        project_id: fill.project_id,
        project: ProjectSeed {
            tasks: task_options(session, project_id, from, fill.project_task_id),
            task_id: fill.project_task_id,
            tags: tags(session, project_id, &fill.tag_ids),
            kept_tag_ids: kept_tags(session, project_id, &fill.tag_ids),
        },
        notes: fill.notes,
    }
}

pub fn project_changed(session: &Session, project_id: Option<i64>, kept_tag_ids: &[i64]) -> ProjectSeed {
    let last = session.week().entries().iter().filter(|e| Some(e.project_id) == project_id).max_by_key(|e| e.modified);
    let mut checked: Vec<i64> = last.map_or(&[][..], |e| &e.tag_ids).to_vec();
    checked.extend(kept_tag_ids);
    ProjectSeed {
        tasks: task_options(session, project_id, None, None),
        task_id: None,
        tags: tags(session, project_id, &checked),
        kept_tag_ids: kept_tags(session, project_id, kept_tag_ids),
    }
}

pub fn trip_form(session: &Session, editing: Option<&Mileage>, preset: Option<&TripPreset>) -> TripSeed {
    let template = editing.or(session.week().travel().last());
    let addresses = saved_addresses(session);
    let projects = project_options(session, editing.map(|m| (m.project_id, &*m.project_name, &*m.customer_name)));
    let (mut project_id, mut travel_type, mut billable, mut return_trip) =
        (template.map(|m| m.project_id), template.map(|m| m.mileage_type), template.is_some_and(|m| m.billable), false);
    if let Some(p) = preset {
        project_id = session.project_named(&p.project).map(|p| p.id).or(project_id);
        travel_type = p.trip_type();
        billable = p.billable;
        return_trip = p.return_trip;
    }
    let (date, from, to, distance, notes) = match (editing, preset) {
        (None, Some(p)) => (
            session.week().day(),
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
            session.week().day(),
            addresses.first().cloned().unwrap_or_default(),
            addresses.get(1).cloned().unwrap_or_default(),
            String::new(),
            String::new(),
        ),
    };
    TripSeed { date, projects, project_id, from, to, distance, notes, return_trip, travel_type, billable, addresses }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::*;
    use crate::session::Base;
    use crate::store::config::{Settings, TripPreset};
    use crate::test_support::*;

    fn session() -> Session {
        let mut s = Session::new(None, date(4));
        let mut me = user();
        me.mileage_addresses = vec!["Home".into(), "Office".into()];
        let tasks = HashMap::from([(3, vec![task(4, 3, "Dev")]), (6, vec![task(7, 6, "Ops")])]);
        s.apply_base(Base { me, ..base(vec![project(3, "P"), project(6, "Other")], tasks, Vec::new()) }, None, &[]);
        s.set_project_tags(HashMap::from([(3, vec![tag(10, "a")]), (6, vec![tag(20, "b")])]));
        let mut first = entry(1, 4, "09:00-10:00");
        first.tag_ids = vec![10, 20, 99];
        first.modified = date(3).and_hms_opt(12, 0, 0).unwrap();
        let mut second = entry(2, 4, "10:00-11:30");
        (second.project_id, second.project_task_id, second.tag_ids) = (6, 7, vec![20]);
        let (generation, _) = s.begin_week_load();
        s.apply_week(
            generation,
            crate::session::Week { start: date(2), entries: vec![first, second], travel: vec![trip(5, 3)] },
            None,
        );
        s
    }

    fn at(day: u32, h: u32, m: u32) -> NaiveDateTime {
        date(day).and_hms_opt(h, m, 0).unwrap()
    }

    /// The day before the session's, so no gap to now is filled.
    fn earlier() -> NaiveDateTime {
        at(3, 23, 0)
    }

    #[test]
    fn a_new_entry_today_fills_the_gap_to_now() {
        let s = session();
        let seed = time_form(&s, None, false, date(4), at(4, 14, 7));
        assert_eq!((seed.start.as_str(), seed.end.as_str()), ("11:30", "14:00"), "now, to the quarter");
        let copy = time_form(&s, Some(&s.week().entries()[0].clone()), true, date(4), at(4, 14, 8));
        assert_eq!((copy.start.as_str(), copy.end.as_str()), ("11:30", "14:15"), "a copy too");
        let edit = time_form(&s, Some(&s.week().entries()[0].clone()), false, date(4), at(4, 14, 7));
        assert_eq!(edit.end, "10:00", "an edit keeps its times");
        assert_eq!(time_form(&s, None, false, date(4), at(4, 11, 37)).end, "", "no gap left once rounded");
        assert_eq!(time_form(&s, None, false, date(4), at(4, 11, 38)).end, "11:45");
        assert_eq!(time_form(&s, None, false, date(4), at(4, 10, 0)).end, "", "the last entry ends later");
        assert_eq!(time_form(&s, None, false, date(4), at(5, 14, 0)).end, "", "another day");
    }

    fn checked(tags: &[TagSeed]) -> Vec<(i64, bool)> {
        tags.iter().map(|t| (t.id, t.checked)).collect()
    }

    #[test]
    fn new_time_entry_follows_the_last_modified_one() {
        let s = session();
        let seed = time_form(&s, None, false, date(4), earlier());
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
        let first = s.week().entries()[0].clone();
        let seed = time_form(&s, Some(&first), false, date(4), earlier());
        assert!(matches!(seed.kind, TimeKind::Edit { id: 1, .. }));
        assert_eq!((seed.start.as_str(), seed.end.as_str(), seed.hours.as_str()), ("09:00", "10:00", "01:00"));
        assert_eq!(checked(&seed.project.tags), [(10, true)]);
        assert_eq!(seed.project.kept_tag_ids, [20, 99]);
        assert!(!seed.recent);

        let copy = time_form(&s, Some(&first), true, date(4), earlier());
        assert_eq!(copy.kind, TimeKind::New);
        assert_eq!(time_form(&s, Some(&first), false, date(9), earlier()).date, date(4), "an edit keeps its date");
        let elsewhere = time_form(&s, Some(&first), true, date(3), earlier());
        assert_eq!((elsewhere.date, elsewhere.start.as_str()), (date(3), ""), "a copy on the given day, which has no entries");
        assert_eq!((copy.start.as_str(), copy.hours.as_str()), ("11:30", ""));
        assert_eq!(copy.project.kept_tag_ids, [20, 99]);
    }

    #[test]
    fn a_gap_seeds_its_slot() {
        let s = session();
        let (from, to) = (NaiveTime::from_hms_opt(12, 0, 0).unwrap(), NaiveTime::from_hms_opt(13, 15, 0).unwrap());
        let seed = gap_form(&s, date(4), from, to, at(4, 14, 2));
        assert_eq!(seed.kind, TimeKind::New);
        assert_eq!((seed.date, seed.start.as_str(), seed.end.as_str(), seed.hours.as_str()), (date(4), "12:00", "13:15", ""));
        assert_eq!((seed.project_id, seed.project.task_id), (Some(3), Some(4)), "the usual default project");
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
        assert_eq!(
            (new.date, new.project_id, new.travel_type, new.billable),
            (date(4), Some(3), Some(MileageType::Business), true)
        );
        assert_eq!(new.addresses, ["Home", "Office", "A", "B"]);

        let preset = TripPreset {
            name: "Commute".into(),
            project: "other".into(),
            from: "X".into(),
            to: "Y".into(),
            distance: Some(23.5),
            return_trip: true,
            travel_type: Some("home-work".into()),
            billable: false,
            notes: None,
        };
        let from_preset = trip_form(&s, None, Some(&preset));
        assert_eq!((from_preset.from.as_str(), from_preset.to.as_str(), from_preset.distance.as_str()), ("X", "Y", "23.5"));
        assert_eq!(
            (from_preset.project_id, from_preset.travel_type, from_preset.billable, from_preset.return_trip),
            (Some(6), Some(MileageType::HomeWork), false, true)
        );

        let edited = trip_form(&s, Some(&s.week().travel()[0]), None);
        assert_eq!((edited.date, edited.from.as_str(), edited.distance.as_str()), (date(3), "A", "10"));
    }

    #[test]
    fn tags_without_a_checkbox_are_kept() {
        let mut s = Session::new(None, date(4));
        s.set_project_tags(HashMap::from([(1, vec![tag(10, "a")]), (2, vec![tag(20, "b")])]));
        assert_eq!(kept_tags(&s, Some(2), &[10, 20, 30]), [10, 30]);
        assert_eq!(kept_tags(&s, Some(1), &[10, 20, 30]), [20, 30]);
        assert_eq!(kept_tags(&s, None, &[10, 20]), [10, 20]);
    }

    #[test]
    fn all_tags_until_some_are_configured() {
        let mut s = Session::new(None, date(4));
        let tags = vec![tag(10, "a"), tag(20, "z_old"), tag(30, "b")];
        s.apply_base(base(vec![project(1, "P")], HashMap::new(), tags), None, &[]);
        let ids = |s: &Session, p| tags_for(s, p).iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(&s, Some(1)), [10, 30], "nothing configured: all but z_");
        assert_eq!(ids(&s, None), Vec::<i64>::new(), "no project, no tags");
        assert_eq!(kept_tags(&s, Some(1), &[10, 20]), [20]);

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
        assert!(is_hidden(&s, 2) && !is_hidden(&s, 1));
        assert_eq!(ids(project_options(&s, None)), [1]);
        assert_eq!(ids(project_options(&s, Some((2, "Old", "Us")))), [1, 2], "still offered when editing its entry");
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
        assert!(is_task_hidden(&s, 1, "OLD") && !is_task_hidden(&s, 1, "Dev") && !is_task_hidden(&s, 2, "Old"));
        assert_eq!(ids(task_options(&s, Some(1), None, None)), [10]);
        assert_eq!(ids(task_options(&s, Some(1), None, Some(30))), [10, 30], "kept when a preset picks it");
        let mut e = entry(5, 4, "09:00-10:00");
        (e.project_id, e.project_task_id, e.task_name) = (1, 20, "Old".into());
        assert_eq!(ids(task_options(&s, Some(1), Some(&e), None)), [10, 20], "still offered when editing its entry");
    }
}
