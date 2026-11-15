use chrono::NaiveDate;
use timechimp_api::{Mileage, TimeEntry};

use super::{Week, reselect, step};
use crate::domain;

/// The week the Time and Travel tabs show: the selected day, that week's entries and trips, and
/// what is selected among them. Changed only through `Session`.
pub struct WeekState {
    day: NaiveDate,
    /// Always the Monday of `day`'s week.
    week_start: NaiveDate,
    /// Moving to another week empties `entries`/`travel` right away, and `apply` takes only
    /// the week shown, so they never hold another week than `week_start`.
    loaded_week: Option<NaiveDate>,
    entries: Vec<TimeEntry>,
    /// Always one of the selected day's entries, if it has any.
    selected_entry: Option<i64>,
    travel: Vec<Mileage>,
    selected_travel: Option<i64>,
    /// Bumped on every week load and on moving to another week, so a slow response for a week
    /// we already left is dropped, even if no load for the new week could start yet.
    generation: u64,
}

impl WeekState {
    pub(super) fn new(today: NaiveDate) -> Self {
        Self {
            day: today,
            week_start: domain::time::monday_of(today),
            loaded_week: None,
            entries: Vec::new(),
            selected_entry: None,
            travel: Vec::new(),
            selected_travel: None,
            generation: 0,
        }
    }

    pub fn day(&self) -> NaiveDate {
        self.day
    }

    pub fn week_start(&self) -> NaiveDate {
        self.week_start
    }

    /// Whether `entries` and `travel` are loaded for the week shown.
    pub fn loaded(&self) -> bool {
        self.loaded_week == Some(self.week_start)
    }

    pub fn entries(&self) -> &[TimeEntry] {
        &self.entries
    }

    pub fn travel(&self) -> &[Mileage] {
        &self.travel
    }

    pub fn visible_entries(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.entries[i].date == self.day).collect()
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

    pub fn hours_per_day(&self) -> [f64; 7] {
        domain::week::hours_per_day(&self.entries, self.week_start)
    }

    pub(super) fn begin_load(&mut self) -> (u64, NaiveDate) {
        self.generation += 1;
        (self.generation, self.week_start)
    }

    /// Drops what an older load or one for another week brings back.
    pub(super) fn apply(&mut self, generation: u64, week: Week, select_id: Option<i64>) -> bool {
        if generation != self.generation || week.start != self.week_start {
            return false;
        }
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

    /// Returns whether `day` is in another week, which then needs loading.
    pub(super) fn select_day(&mut self, day: NaiveDate) -> bool {
        self.day = day;
        let week_start = domain::time::monday_of(day);
        let moved = week_start != self.week_start;
        if moved {
            self.week_start = week_start;
            self.generation += 1;
            self.loaded_week = None;
            self.entries.clear();
            self.travel.clear();
            self.selected_travel = None;
        }
        self.fix_selection();
        moved
    }

    fn fix_selection(&mut self) {
        let visible = self.visible_entries();
        if !visible.iter().any(|&i| Some(self.entries[i].id) == self.selected_entry) {
            self.selected_entry = visible.first().map(|&i| self.entries[i].id);
        }
    }

    pub(super) fn select_entry(&mut self, id: i64) {
        if let Some(day) = self.entries.iter().find(|e| e.id == id).map(|e| e.date) {
            self.selected_entry = Some(id);
            self.day = day;
        }
    }

    pub(super) fn select_travel(&mut self, id: i64) {
        if self.travel.iter().any(|m| m.id == id) {
            self.selected_travel = Some(id);
        }
    }

    /// Moves the selection `delta` entries within the selected day, stopping at either end.
    pub(super) fn move_entry_selection(&mut self, delta: isize) {
        let ids: Vec<i64> = self.visible_entries().into_iter().map(|i| self.entries[i].id).collect();
        if let Some(id) = step(&ids, self.selected_entry, delta) {
            self.selected_entry = Some(id);
        }
    }

    /// Moves the selection `delta` trips, stopping at either end.
    pub(super) fn move_travel_selection(&mut self, delta: isize) {
        let ids: Vec<i64> = self.travel.iter().map(|m| m.id).collect();
        if let Some(id) = step(&ids, self.selected_travel, delta) {
            self.selected_travel = Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn only_the_latest_load_of_the_week_shown_applies() {
        let mut w = WeekState::new(date(4));
        let week = |monday: u32, id| Week {
            start: date(monday),
            entries: vec![entry(id, monday + 1, "09:00-10:00")],
            travel: vec![trip(id, monday + 1)],
        };
        let load = |w: &mut WeekState, week| {
            let (generation, _) = w.begin_load();
            w.apply(generation, week, None)
        };

        let (first, monday) = w.begin_load();
        let (second, _) = w.begin_load();
        assert_eq!(monday, date(2));
        assert!(!w.apply(first, week(2, 1), None), "an older load");
        assert!(!w.loaded());
        assert!(!w.apply(second, week(9, 1), None), "another week");
        assert!(!w.loaded() && w.entries().is_empty());
        assert!(w.apply(second, week(2, 1), None));
        assert!(w.loaded() && w.entries().len() == 1);

        // A load for this week is underway when the user moves on before any load for the next
        // could start (as while the user isn't known yet): its week must not show under the next.
        let (pending, _) = w.begin_load();
        assert!(w.select_day(date(11)));
        assert!(!w.apply(pending, week(2, 2), None));
        assert!(!w.loaded() && w.entries().is_empty() && w.travel().is_empty());
        assert!(load(&mut w, week(9, 3)));
        assert_eq!(w.entries()[0].id, 3);

        let (generation, _) = w.begin_load();
        assert!(!w.select_day(date(10)), "the same week");
        assert!(w.apply(generation, week(9, 4), None), "a day within the week keeps its load");
    }

    #[test]
    fn selection_follows_the_day() {
        let mut w = WeekState::new(date(4));
        let entries = vec![entry(1, 3, "09:00-10:00"), entry(2, 4, "09:00-10:00"), entry(3, 4, "10:00-11:30")];
        let (generation, _) = w.begin_load();
        w.apply(generation, Week { start: date(2), entries, travel: vec![trip(7, 3), trip(8, 4)] }, None);
        assert_eq!(w.selected_entry_id(), Some(2), "the first entry of the selected day");
        assert_eq!(w.selected_travel_id(), Some(7), "the first trip of the week");
        w.move_entry_selection(-1);
        assert_eq!(w.selected_entry_id(), Some(2), "stops at the day's first");
        w.move_travel_selection(5);
        assert_eq!(w.selected_travel_id(), Some(8));

        w.select_entry(1);
        assert_eq!((w.selected_entry_id(), w.day()), (Some(1), date(3)), "selecting an entry moves to its day");
        w.select_entry(9);
        assert_eq!(w.selected_entry_id(), Some(1), "not an entry");
        assert!(!w.select_day(date(4)));
        assert_eq!(w.selected_entry_id(), Some(2), "the new day's first");
    }
}
