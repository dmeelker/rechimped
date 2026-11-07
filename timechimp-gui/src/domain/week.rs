//! What the Week tab and the Recent picker derive from the loaded entries.

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveTime};
use timechimp::TimeEntry;

use super::time::parse_time;

/// Something worked on recently: a distinct project / task / notes combination. `entry` is its
/// latest registration, which also supplies the tags.
#[derive(Clone)]
pub struct RecentItem {
    pub entry: TimeEntry,
    pub uses: usize,
}

impl RecentItem {
    pub fn notes(&self) -> &str {
        self.entry.notes.as_deref().unwrap_or("").trim()
    }

    /// Whether every word of `query` occurs (case-insensitively) in the notes, project, task,
    /// customer or tags.
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

/// Groups `entries` by project, task and notes; the most recently used first, then the most used.
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

/// Billable hours among `entries`.
pub fn billable_hours(entries: &[TimeEntry]) -> f64 {
    entries.iter().filter(|e| e.billable).map(|e| e.hours).sum()
}

/// The week's billable hours as a fraction of the contract hours, or (without a contract) of the
/// hours logged; None when there is neither.
pub fn billable_share(entries: &[TimeEntry], contract_hours: f64) -> Option<f64> {
    let logged: f64 = entries.iter().map(|e| e.hours).sum();
    let of = if contract_hours > 0.0 { contract_hours } else { logged };
    (of > 0.0).then(|| billable_hours(entries) / of)
}

/// Hours registered per day of the week starting on `monday`, Monday first.
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

/// The local start and end time of an entry, from its `HH:mm-HH:mm` range.
pub fn time_range(entry: &TimeEntry) -> Option<(NaiveTime, NaiveTime)> {
    let (start, end) = entry.start_end.as_deref()?.split_once('-')?;
    Some((parse_time(start).ok()??, parse_time(end).ok()??))
}

/// How two consecutive entries on a day fit together, when they don't line up exactly.
#[derive(Debug, PartialEq, Eq)]
pub enum Junction {
    /// Nothing registered between `from` and `to`.
    Gap { from: NaiveTime, to: NaiveTime },
    /// Both entries cover `from`–`to`.
    Overlap { from: NaiveTime, to: NaiveTime },
}

/// Compares the end of `prev` with the start of `next` (entries without times are ignored).
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
        // Fully contained: the overlap is the inner entry.
        assert_eq!(
            range_junction(r("8", "12"), r("9", "10")),
            Some(Junction::Overlap { from: tm("9"), to: tm("10") })
        );
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
}
