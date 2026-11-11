use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, NaiveDate, NaiveTime};
use timechimp_api::TimeEntry;

use super::time::parse_time;

#[derive(Clone)]
pub struct RecentItem {
    pub entry: TimeEntry,
    pub uses: usize,
}

impl RecentItem {
    pub fn notes(&self) -> &str {
        self.entry.notes.as_deref().unwrap_or("").trim()
    }

    pub fn matches(&self, query: &str) -> bool {
        let e = &self.entry;
        let haystack =
            format!("{} {} {} {} {}", self.notes(), e.project_name, e.task_name, e.customer_name, e.tag_names.join(" "));
        matches_words(&haystack, query)
    }
}

fn matches_words(haystack: &str, query: &str) -> bool {
    let haystack = haystack.to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
}

pub fn recent_items<'a>(entries: impl IntoIterator<Item = &'a TimeEntry>) -> Vec<RecentItem> {
    let mut groups: HashMap<(i64, i64, String), RecentItem> = HashMap::new();
    for e in entries {
        let key = (e.project_id, e.project_task_id, e.notes.as_deref().unwrap_or("").trim().to_lowercase());
        let item = groups.entry(key).or_insert_with(|| RecentItem { entry: e.clone(), uses: 0 });
        item.uses += 1;
        if (e.date, e.modified) > (item.entry.date, item.entry.modified) {
            item.entry = e.clone();
        }
    }
    let mut items: Vec<RecentItem> = groups.into_values().collect();
    items.sort_by(|a, b| {
        (b.entry.date, b.uses, b.entry.modified).cmp(&(a.entry.date, a.uses, a.entry.modified))
    });
    items
}

pub fn billable_hours(entries: &[TimeEntry]) -> f64 {
    // A fold from +0.0: `sum` of nothing is -0.0, which shows as "-0%".
    entries.iter().filter(|e| e.billable).fold(0.0, |total, e| total + e.hours)
}

/// Per project, by name, as the config holds them.
pub type TaskNames = BTreeMap<String, Vec<String>>;

pub fn is_excluded(entry: &TimeEntry, excluded: &TaskNames) -> bool {
    excluded.iter().any(|(project, tasks)| {
        super::same_name(project, &entry.project_name) && tasks.iter().any(|t| super::same_name(t, &entry.task_name))
    })
}

/// Excluded tasks (leave) count neither as hours worked nor towards the contract, so a week with a
/// day off isn't short.
#[derive(Debug, PartialEq)]
pub struct WeekCard {
    pub hours: f64,
    pub billable: f64,
    pub contract: f64,
    pub share: Option<f64>,
}

pub fn week_card(entries: &[TimeEntry], contract_hours: f64, excluded: &TaskNames) -> WeekCard {
    let worked: Vec<TimeEntry> = entries.iter().filter(|e| !is_excluded(e, excluded)).cloned().collect();
    // Folds from +0.0: `sum` of nothing is -0.0, which shows as "-0".
    let hours = worked.iter().fold(0.0, |total, e| total + e.hours);
    let leave = entries.iter().filter(|e| is_excluded(e, excluded)).fold(0.0, |total, e| total + e.hours);
    let contract = (contract_hours - leave).max(0.0);
    let billable = billable_hours(&worked);
    let of = if contract > 0.0 { contract } else { hours };
    WeekCard { hours, billable, contract, share: (of > 0.0).then(|| billable / of) }
}

pub const CHART_WEEKS: i64 = 5;

pub fn weeks_up_to(monday: NaiveDate, count: i64) -> Vec<NaiveDate> {
    (0..count).rev().map(|k| monday - chrono::Duration::weeks(k)).collect()
}

pub fn week_cards(
    entries: &[TimeEntry],
    mondays: &[NaiveDate],
    contract_hours: f64,
    excluded: &TaskNames,
) -> Vec<(NaiveDate, WeekCard)> {
    mondays
        .iter()
        .map(|&monday| {
            let week = monday..monday + chrono::Duration::days(7);
            let in_week: Vec<TimeEntry> = entries.iter().filter(|e| week.contains(&e.date)).cloned().collect();
            (monday, week_card(&in_week, contract_hours, excluded))
        })
        .collect()
}

/// The selected day too, so a day jumped to (e.g. after saving on a Saturday) still has its card.
pub fn shows_day(day: NaiveDate, work_days: [bool; 7], today: NaiveDate, selected: NaiveDate) -> bool {
    work_days[day.weekday().num_days_from_monday() as usize] || day == today || day == selected
}

pub fn step_day(day: NaiveDate, direction: i64, work_days: [bool; 7], today: NaiveDate) -> NaiveDate {
    let next = |d: NaiveDate| d + chrono::Duration::days(direction);
    let mut candidate = next(day);
    for _ in 0..7 {
        if shows_day(candidate, work_days, today, today) {
            return candidate;
        }
        candidate = next(candidate);
    }
    next(day)
}

pub fn hours_per_day(entries: &[TimeEntry], monday: NaiveDate) -> [f64; 7] {
    let mut totals = [0.0; 7];
    for e in entries {
        let offset = (e.date - monday).num_days();
        if (0..7).contains(&offset) {
            totals[offset as usize] += e.hours;
        }
    }
    totals
}

pub fn time_range(entry: &TimeEntry) -> Option<(NaiveTime, NaiveTime)> {
    let (start, end) = entry.start_end.as_deref()?.split_once('-')?;
    Some((parse_time(start).ok()??, parse_time(end).ok()??))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Junction {
    Gap { from: NaiveTime, to: NaiveTime },
    Overlap { from: NaiveTime, to: NaiveTime },
}

pub fn junction(prev: &TimeEntry, next: &TimeEntry) -> Option<Junction> {
    if prev.date != next.date {
        return None;
    }
    range_junction(time_range(prev)?, time_range(next)?)
}

/// `prev` must not start after `next` (entries are sorted by start time).
fn range_junction((_, prev_end): (NaiveTime, NaiveTime), (next_start, next_end): (NaiveTime, NaiveTime)) -> Option<Junction> {
    if prev_end == next_start {
        return None;
    }
    Some(if prev_end < next_start {
        Junction::Gap { from: prev_end, to: next_start }
    } else {
        Junction::Overlap { from: next_start, to: prev_end.min(next_end) }
    })
}

pub fn contract_summary(contract_hours: f64) -> String {
    if contract_hours <= 0.0 {
        return "No contract hours in TimeChimp".into();
    }
    format!("{} a week", super::hh_mm(contract_hours))
}

/// What a billable goal comes to in hours, for a full week without leave.
pub fn goal_summary(contract_hours: f64, goal: f64) -> Option<String> {
    (contract_hours > 0.0).then(|| {
        format!("= {} / {}", super::hh_mm(contract_hours * goal), super::hh_mm(contract_hours))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junctions() {
        let r = |a: &str, b: &str| (parse_time(a).unwrap().unwrap(), parse_time(b).unwrap().unwrap());
        let tm = |s: &str| parse_time(s).unwrap().unwrap();
        assert_eq!(range_junction(r("830", "9"), r("9", "915")), None);
        assert_eq!(range_junction(r("830", "9"), r("930", "10")), Some(Junction::Gap { from: tm("9"), to: tm("930") }));
        assert_eq!(
            range_junction(r("830", "1000"), r("930", "1100")),
            Some(Junction::Overlap { from: tm("930"), to: tm("1000") })
        );
        assert_eq!(
            range_junction(r("8", "12"), r("9", "10")),
            Some(Junction::Overlap { from: tm("9"), to: tm("10") })
        );
    }

    const WEEKDAYS: [bool; 7] = [true, true, true, true, true, false, false];

    #[test]
    fn billable_share_of_the_week() {
        use crate::test_support::entry;
        let mut unbilled = entry(2, 3, "10:00-13:00");
        (unbilled.billable, unbilled.hours) = (false, 3.0);
        let entries = [entry(1, 2, "9:00-10:00"), unbilled];
        assert_eq!(week_card(&entries, 40.0, &TaskNames::new()).share, Some(1.0 / 40.0));
        assert_eq!(week_card(&entries, 0.0, &TaskNames::new()).share, Some(1.0 / 4.0), "of the hours logged");
        assert_eq!(week_card(&[], 0.0, &TaskNames::new()).share, None);
        assert!(week_card(&[], 40.0, &TaskNames::new()).share.unwrap().is_sign_positive(), "not -0");
    }

    #[test]
    fn leave_is_left_out_of_the_week_card() {
        use crate::test_support::entry;
        let mut leave = entry(2, 3, "");
        (leave.project_name, leave.task_name, leave.billable, leave.hours) = ("intern".into(), "Verlof".into(), false, 8.0);
        let mut meeting = entry(3, 4, "");
        (meeting.project_name, meeting.task_name, meeting.billable, meeting.hours) = ("Intern".into(), "Meeting".into(), false, 2.0);
        let entries = [entry(1, 2, "9:00-10:00"), leave.clone(), meeting.clone()];
        let excluded = TaskNames::from([("Intern".into(), vec!["verlof".into()])]);
        assert!(is_excluded(&leave, &excluded) && !is_excluded(&meeting, &excluded), "by name, case-insensitively");
        assert_eq!(
            week_card(&entries, 40.0, &excluded),
            WeekCard { hours: 3.0, billable: 1.0, contract: 32.0, share: Some(1.0 / 32.0) }
        );
        assert_eq!(week_card(&entries, 0.0, &excluded).share, Some(1.0 / 3.0), "of the hours logged, leave excluded");
        assert_eq!(week_card(&entries, 40.0, &TaskNames::new()).hours, 11.0, "nothing excluded");
        let card = week_card(&[leave], 8.0, &excluded);
        assert_eq!((card.hours, card.contract, card.share), (0.0, 0.0, None));
        assert!(card.hours.is_sign_positive(), "not -0");
    }

    #[test]
    fn cards_per_week() {
        use crate::test_support::{date, entry};
        let mondays = weeks_up_to(date(9), 3);
        assert_eq!(mondays, [date(9) - chrono::Duration::weeks(2), date(2), date(9)]);
        let mut unbilled = entry(3, 8, "");
        unbilled.billable = false;
        let entries = [entry(1, 2, ""), entry(2, 8, ""), unbilled, entry(4, 9, "")];
        let shares: Vec<_> = week_cards(&entries, &mondays, 4.0, &TaskNames::new()).into_iter().map(|(m, c)| (m, c.hours, c.share)).collect();
        assert_eq!(
            shares,
            [(mondays[0], 0.0, Some(0.0)), (date(2), 3.0, Some(2.0 / 4.0)), (date(9), 1.0, Some(1.0 / 4.0))],
            "Sunday the 8th counts for the week of the 2nd"
        );
    }

    #[test]
    fn shown_days() {
        use crate::test_support::date;
        let (fri, sat, sun) = (date(6), date(7), date(8));
        assert!(shows_day(fri, WEEKDAYS, fri, fri));
        assert!(!shows_day(sat, WEEKDAYS, fri, fri));
        assert!(shows_day(sat, WEEKDAYS, sat, fri), "today");
        assert!(shows_day(sun, WEEKDAYS, fri, sun), "selected");
    }

    #[test]
    fn stepping_skips_hidden_days() {
        use crate::test_support::date;
        let (wed, fri, sat, mon) = (date(4), date(6), date(7), date(9));
        assert_eq!(step_day(fri, 1, WEEKDAYS, wed), mon);
        assert_eq!(step_day(mon, -1, WEEKDAYS, wed), fri);
        assert_eq!(step_day(fri, 1, WEEKDAYS, sat), sat, "today shows");
        assert_eq!(step_day(mon, -1, WEEKDAYS, sat), sat);
        assert_eq!(step_day(sat, 1, WEEKDAYS, sat), mon, "leaving today, the weekend is skipped");
        assert_eq!(step_day(wed, 1, [false; 7], wed), date(5));
    }

    #[test]
    fn recent_filter() {
        let haystack = "Code reviews Tournament Software Software development Visual Reality wbso_tournament";
        assert!(matches_words(haystack, ""));
        assert!(matches_words(haystack, "code"));
        assert!(matches_words(haystack, "REV tourn"));
        assert!(matches_words(haystack, "  visual   wbso "));
        assert!(!matches_words(haystack, "code daily"));
    }

    #[test]
    fn goal_in_hours() {
        assert_eq!(goal_summary(32.0, 0.75).as_deref(), Some("= 24:00 / 32:00"));
        assert_eq!(goal_summary(32.0, 0.7).as_deref(), Some("= 22:24 / 32:00"));
        assert_eq!(goal_summary(0.0, 0.8), None);
    }

    #[test]
    fn contract_summary_per_week() {
        assert_eq!(contract_summary(32.0), "32:00 a week");
        assert_eq!(contract_summary(36.5), "36:30 a week");
        assert_eq!(contract_summary(0.0), "No contract hours in TimeChimp");
    }
}
