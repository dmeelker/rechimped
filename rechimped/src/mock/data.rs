//! The mock account: a fixed catalogue (customers, projects, tasks, tags) and about twelve weeks of
//! time entries and trips up to now, from a fixed seed so every run looks the same on the same day.
//! It covers what the app has screens for: leave and sick days (the default excluded tasks), gaps
//! and an overlap, entries without notes, differing tags, a rejected entry, and approved/invoiced
//! (locked) months before the one usually reviewed.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use chrono_tz::Tz;
use serde_json::json;
use timechimp_api::{MileageType, NewMileage, NewTimeEntry, Project, ProjectTask, Tag};

use super::{State, Status};
use crate::domain::time::{first_of_month, monday_of};

const USER: i64 = 1001;
const COLLEAGUE_ID: i64 = 1002;
pub const COLLEAGUE: &str = "Charlie Colleague";
const WEEKS: i64 = 12;
/// The mock user's timezone.
pub const TZ: Tz = chrono_tz::Europe::Amsterdam;
/// The mileage report's reimbursement per kilometre.
pub const RATE_PER_KM: f64 = 0.23;

const HOME: &str = "Kerkstraat 12, 3511 LX Utrecht";
const OFFICE: &str = "Stationsplein 1, 3818 LE Amersfoort";
const ACME: &str = "Keizersgracht 100, 1015 CV Amsterdam";
const GLOBEX: &str = "Weena 500, 3013 AL Rotterdam";
const ROUTES: &[(&str, &str, f64)] = &[
    (HOME, OFFICE, 23.4),
    (HOME, ACME, 41.2),
    (HOME, GLOBEX, 57.8),
    (OFFICE, ACME, 52.6),
    (OFFICE, GLOBEX, 79.1),
    (ACME, GLOBEX, 78.3),
];

const INTERN: i64 = 10;
const ACME_SHOP: i64 = 11;
const GLOBEX_DATA: i64 = 12;
const MIGRATION: i64 = 14;

/// id, name, customer id, customer, internal, active
const PROJECTS: &[(i64, &str, i64, &str, bool, bool)] = &[
    (INTERN, "Intern", 1, "Mockery B.V.", true, true),
    (ACME_SHOP, "Acme Webshop", 2, "Acme Corp", false, true),
    (GLOBEX_DATA, "Globex Data Platform", 3, "Globex", false, true),
    (13, "Initech Support", 4, "Initech", false, true),
    (MIGRATION, "Initech Migration", 4, "Initech", false, false),
    (15, "R&D Rechimped", 1, "Mockery B.V.", true, true),
];

const LEAVE: i64 = 101;
const SICK: i64 = 102;

/// project task id, project id, task id, name, billable, active
const TASKS: &[(i64, i64, i64, &str, bool, bool)] = &[
    (LEAVE, INTERN, 6, "Verlof", false, true),
    (SICK, INTERN, 7, "Ziek", false, true),
    (103, INTERN, 8, "Opleiding", false, true),
    (104, INTERN, 9, "Administratie", false, true),
    (105, INTERN, 3, "Meeting", false, true),
    (111, ACME_SHOP, 1, "Development", true, true),
    (112, ACME_SHOP, 2, "Design", true, true),
    (113, ACME_SHOP, 3, "Meeting", false, true),
    (114, ACME_SHOP, 11, "Legacy", true, false),
    (121, GLOBEX_DATA, 1, "Development", true, true),
    (122, GLOBEX_DATA, 4, "Consultancy", true, true),
    (123, GLOBEX_DATA, 3, "Meeting", false, true),
    (131, 13, 5, "Support", true, true),
    (141, MIGRATION, 1, "Development", true, true),
    (151, 15, 10, "Research", false, true),
    (152, 15, 1, "Development", false, true),
];

/// id, name, active
const TAGS: &[(i64, &str, bool)] = &[
    (1, "wbso_rechimped", true),
    (2, "wbso_platform", true),
    (3, "remote", true),
    (4, "onsite", true),
    (5, "z_archived", true),
    (6, "old", false),
];

/// project task id, notes to pick from, tags
type Work = (i64, &'static [&'static str], &'static [i64]);
const ACME_WORK: &[Work] = &[
    (111, &["Checkout flow", "Product search", "Fix cart rounding bug", "Code review"], &[]),
    (111, &["Checkout flow", "Payment provider integration"], &[]),
    (112, &["Checkout redesign", "Style guide"], &[]),
];
const GLOBEX_WORK: &[Work] = &[
    (121, &["Ingest pipeline", "Query performance", "Schema migration"], &[2]),
    (121, &["Ingest pipeline", "Dashboard API"], &[2]),
    (122, &["Architecture workshop", "Advice on data retention"], &[]),
];
const OTHER_WORK: &[Work] = &[
    (151, &["Prototype sync engine", "Offline mode research"], &[1]),
    (152, &["Sync engine", "Test harness"], &[1]),
    (131, &["Ticket triage", "Incident follow-up"], &[]),
    (104, &["Expenses", "Timesheets"], &[]),
    (103, &["Rust course"], &[]),
    (105, &["Company meeting"], &[]),
];
const MIGRATION_WORK: Work = (141, &["Data migration scripts", "Cut-over rehearsal"], &[]);

/// splitmix64: a fixed seed, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// The mock's `config.toml`, written fresh on every start: presets to pick from, and the travel
/// reminder on (it shows on a work day; Escape closes it).
pub fn config() -> String {
    format!(
        r#"# rechimped --mock: written fresh on every start.
travel_reminder = true

[[trips]]
name = "Office"
project = "Intern"
from = "{HOME}"
to = "{OFFICE}"
return_trip = true
type = "home-work"

[[trips]]
name = "Acme on site"
project = "Acme Webshop"
from = "{HOME}"
to = "{ACME}"
return_trip = true
billable = true

[[time_entries]]
project = "Intern"
task = "Meeting"
notes = "Company meeting"

[[time_entries]]
project = "R&D Rechimped"
task = "Research"
notes = "Prototype sync engine"
tags = ["wbso_rechimped"]
"#
    )
}

/// Kilometres between two addresses: the known routes, else a stable made-up figure.
pub fn distance(from: &str, to: &str) -> f64 {
    let known = ROUTES.iter().find(|(a, b, _)| (*a == from && *b == to) || (*a == to && *b == from));
    if let Some((_, _, km)) = known {
        return *km;
    }
    // FNV-1a over both, order-independent, so a return trip is as long as the way out.
    let hash = |s: &str| s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let h = hash(from) ^ hash(to);
    5.0 + (h % 900) as f64 / 10.0
}

fn catalogue() -> (Vec<Project>, Vec<ProjectTask>, Vec<Tag>) {
    let projects = PROJECTS
        .iter()
        .map(|&(id, name, customer_id, customer, intern, active)| {
            serde_json::from_value(json!({
                "id": id, "name": name, "customerId": customer_id, "customerName": customer,
                "intern": intern, "unspecified": false, "active": active,
            }))
            .expect("mock project")
        })
        .collect();
    let tasks = TASKS
        .iter()
        .map(|&(id, project_id, task_id, name, billable, active)| ProjectTask {
            id,
            name: name.into(),
            billable,
            active,
            project_id,
            task_id,
        })
        .collect();
    let tags = TAGS
        .iter()
        .map(|&(id, name, active)| {
            serde_json::from_value(json!({ "id": id, "companyId": 1, "name": name, "active": active, "type": 1 }))
                .expect("mock tag")
        })
        .collect();
    (projects, tasks, tags)
}

fn me() -> timechimp_api::CurrentUser {
    serde_json::from_value(json!({
        "id": USER, "userName": "mock@example.com", "displayName": "Morgan Mock",
        "companyId": 1, "companyName": "Mockery B.V.", "accountTypeId": 1, "roleId": 2, "roleName": "Employee",
        "contractHours": 40.0, "timezone": "Europe/Amsterdam", "startWeekOn": "monday", "currency": "EUR",
        "customerRequired": true, "projectRequired": true, "taskRequired": true,
        "timeDescriptionRequired": false, "accessAllProjects": false, "isApprover": false,
        "isContractor": false, "isLocked": false,
        "userAvailability": {
            "id": 1, "monday": 8.0, "tuesday": 8.0, "wednesday": 8.0, "thursday": 8.0, "friday": 8.0,
            "saturday": null, "sunday": null,
        },
        "mileageAddresses": [HOME, OFFICE, ACME, GLOBEX],
    }))
    .expect("mock user")
}

pub(super) fn generate(now: NaiveDateTime) -> State {
    let (projects, tasks, tags) = catalogue();
    let mut state = State { me: me(), projects, tasks, tags, entries: Vec::new(), trips: Vec::new(), next_id: 10_000 };
    let mut rng = Rng(0x5EED_CAFE);
    let today = now.date();
    let first = monday_of(today) - Duration::weeks(WEEKS - 1);
    // Handed in and approved; the month before this one is the one usually reviewed.
    let reviewed = first_of_month(first_of_month(today) - Duration::days(1));
    let mut day = first;
    while day <= today {
        if day.weekday() == Weekday::Mon {
            colleague_trip(&mut state, day);
        }
        if day.weekday().num_days_from_monday() < 5 {
            let status = |billable: bool| match day {
                d if d < reviewed => Status { intern: 2, client: if billable { 3 } else { 0 } },
                // The first week of the reviewed month is waiting for approval already.
                d if d < reviewed + Duration::days(7) => Status { intern: 1, client: 0 },
                _ => Status::default(),
            };
            work_day(&mut state, &mut rng, day, (day == today).then_some(now.time()), first, &status);
        }
        day += Duration::days(1);
    }
    reject(&mut state, reviewed + Duration::days(14));
    reject(&mut state, monday_of(today));
    state
}

/// `until` is the time now when `day` is today: nothing later is registered yet.
fn work_day(
    state: &mut State,
    rng: &mut Rng,
    day: NaiveDate,
    until: Option<NaiveTime>,
    first_day: NaiveDate,
    status: &dyn Fn(bool) -> Status,
) {
    if until.is_none() {
        for (task, p) in [(LEAVE, 0.03), (SICK, 0.02)] {
            if rng.chance(p) {
                let entry = full_day(state, task, day);
                state.add_entry(&entry, status(false)).expect("mock leave");
                return;
            }
        }
    }

    // A customer per week, and the migration in the oldest weeks only.
    let acme_week = day.iso_week().week().is_multiple_of(2);
    let main = if acme_week { ACME_WORK } else { GLOBEX_WORK };
    let migration = (day - first_day).num_weeks() < 4;

    let mut time = NaiveTime::from_hms_opt(8, 30, 0).unwrap() + Duration::minutes(15 * rng.below(4) as i64);
    let mut left = 32 + rng.below(5) as i64 - 2;
    let mut lunch = false;
    let mut first = true;
    while left > 0 {
        if !lunch && time >= NaiveTime::from_hms_opt(12, 0, 0).unwrap() {
            time += Duration::minutes(30);
            lunch = true;
        } else if !first && rng.chance(0.05) {
            time += Duration::minutes(15);
        } else if !first && rng.chance(0.02) {
            time -= Duration::minutes(15);
        }
        let work = match rng.below(10) {
            0..=4 => rng.pick(main),
            5..=6 if migration => &MIGRATION_WORK,
            _ => rng.pick(OTHER_WORK),
        };
        let quarters = (2 + rng.below(9) as i64).min(left);
        let end = time + Duration::minutes(15 * quarters);
        if until.is_some_and(|now| end > now) {
            break;
        }
        let (task_id, notes, tag_ids) = work;
        let task = state.tasks.iter().find(|t| t.id == *task_id).expect("work task");
        let project = state.project(task.project_id).expect("work project");
        let at = |t: NaiveTime| TZ.from_local_datetime(&day.and_time(t)).earliest().expect("local time").fixed_offset();
        let mut entry = NewTimeEntry::new(USER, project, task, day, 0.0).with_times(at(time), at(end));
        entry.timezone = Some(TZ.name().into());
        if !rng.chance(0.04) {
            entry.notes = Some(rng.pick(notes).to_string());
        }
        // Now and then the tags are forgotten, which the review flags.
        if !rng.chance(0.12) {
            entry.tag_ids = tag_ids.to_vec();
        }
        let billable = task.billable;
        state.add_entry(&entry, status(billable)).expect("mock entry");
        time = end;
        left -= quarters;
        first = false;
    }

    if until.is_none() {
        let customer = if acme_week { (ACME_SHOP, ACME) } else { (GLOBEX_DATA, GLOBEX) };
        if rng.chance(0.25) {
            round_trip(state, day, customer.0, HOME, customer.1, MileageType::Business, true, status(true));
        } else if rng.chance(0.4) {
            round_trip(state, day, INTERN, HOME, OFFICE, MileageType::HomeWork, false, status(false));
        }
    }
}

/// Leave or sick: hours only, no times or notes.
fn full_day(state: &State, task: i64, day: NaiveDate) -> NewTimeEntry {
    let task = state.tasks.iter().find(|t| t.id == task).expect("leave task");
    let project = state.project(task.project_id).expect("leave project");
    NewTimeEntry::new(USER, project, task, day, 8.0)
}

#[allow(clippy::too_many_arguments)]
fn round_trip(
    state: &mut State,
    day: NaiveDate,
    project_id: i64,
    from: &str,
    to: &str,
    kind: MileageType,
    billable: bool,
    status: Status,
) {
    let customer_id = state.project(project_id).expect("trip project").customer_id;
    let out = NewMileage {
        user_id: USER,
        customer_id,
        project_id,
        date: day,
        distance: distance(from, to),
        from_address: from.into(),
        to_address: to.into(),
        notes: None,
        billable,
        mileage_type: kind,
        mileage_vehicle_id: None,
    };
    let back = NewMileage { from_address: to.into(), to_address: from.into(), ..out.clone() };
    state.add_trip(&out, status).expect("mock trip");
    state.add_trip(&back, status).expect("mock trip");
}

/// Shows only in the mileage report grid, which returns everyone's trips.
fn colleague_trip(state: &mut State, day: NaiveDate) {
    let trip = NewMileage {
        user_id: COLLEAGUE_ID,
        customer_id: 2,
        project_id: ACME_SHOP,
        date: day,
        distance: distance(OFFICE, ACME),
        from_address: OFFICE.into(),
        to_address: ACME.into(),
        notes: Some("Workshop".into()),
        billable: true,
        mileage_type: MileageType::Business,
        mileage_vehicle_id: None,
    };
    state.add_trip(&trip, Status::default()).expect("mock trip");
}

/// Rejects the first billable entry on or after `from`.
fn reject(state: &mut State, from: NaiveDate) {
    let mut found: Vec<_> = state.entries.iter_mut().filter(|e| e.billable && e.date >= from).collect();
    found.sort_by_key(|e| (e.date, e.start));
    if let Some(entry) = found.into_iter().next() {
        entry.status = -1;
        entry.status_intern = -1;
    }
}
