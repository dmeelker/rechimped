use chrono::{Months, NaiveDate};
use timechimp_api::TimeReportRow;

use super::{reselect, step};
use crate::domain::{self, review, week::TaskNames};

/// The Review tab's month, kept apart from the report's.
pub(super) struct Review {
    /// Always the first of the month.
    month: NaiveDate,
    /// The month `rows` are for; None while they are stale, as after `invalidate`.
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

impl Review {
    pub(super) fn new(today: NaiveDate) -> Self {
        Self {
            // The month usually handed in.
            month: domain::time::first_of_month(today) - Months::new(1),
            loaded: None,
            rows: Vec::new(),
            selected: None,
            generation: 0,
            analysis: review::Analysis::default(),
            revision: 0,
        }
    }

    pub(super) fn month(&self) -> NaiveDate {
        self.month
    }

    /// Another month's rows go at once, so they never show under its title. Returns whether the
    /// month changed, after which the analysis must be redone.
    pub(super) fn set_month(&mut self, day: NaiveDate) -> bool {
        let month = domain::time::first_of_month(day);
        if month == self.month {
            return false;
        }
        let Review { generation, revision, .. } = *self;
        let analysis = review::Analysis::default();
        *self = Review { month, loaded: None, rows: Vec::new(), selected: None, generation, analysis, revision };
        true
    }

    pub(super) fn begin_load(&mut self) -> (u64, NaiveDate) {
        self.generation += 1;
        (self.generation, self.month)
    }

    /// Drops what an older load or one for another month brings back. Once applied, the
    /// analysis must be redone.
    pub(super) fn apply(&mut self, generation: u64, month: NaiveDate, rows: Vec<TimeReportRow>, select_id: Option<i64>) -> bool {
        if generation != self.generation || month != self.month {
            return false;
        }
        let ids = |rows: &[TimeReportRow]| rows.iter().map(|r| r.id).collect::<Vec<_>>();
        self.selected = reselect(self.selected, select_id, &ids(&self.rows), &ids(&rows));
        self.rows = rows;
        self.loaded = Some(month);
        true
    }

    pub(super) fn analyze(&mut self, excluded: &TaskNames) {
        self.analysis = review::Analysis::of(&self.rows, excluded);
        self.revision += 1;
    }

    /// The lines, tag mismatches and findings of `rows`; None while those are.
    pub(super) fn analysis(&self) -> Option<&review::Analysis> {
        self.rows().is_some().then_some(&self.analysis)
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// None unless loaded for the month shown and not invalidated since.
    pub(super) fn rows(&self) -> Option<&[TimeReportRow]> {
        (self.loaded == Some(self.month)).then_some(self.rows.as_slice())
    }

    pub(super) fn invalidate(&mut self) {
        self.loaded = None;
        self.generation += 1;
    }

    pub(super) fn selected_id(&self) -> Option<i64> {
        self.selected
    }

    pub(super) fn selected(&self) -> Option<&TimeReportRow> {
        self.rows()?.iter().find(|r| Some(r.id) == self.selected)
    }

    pub(super) fn select(&mut self, id: i64) {
        if self.rows().is_some_and(|rows| rows.iter().any(|r| r.id == id)) {
            self.selected = Some(id);
        }
    }

    /// Moves the selection `delta` rows, stopping at either end; not while stale.
    pub(super) fn move_selection(&mut self, delta: isize) {
        let ids: Vec<i64> = self.rows().unwrap_or_default().iter().map(|r| r.id).collect();
        if let Some(id) = step(&ids, self.selected, delta) {
            self.selected = Some(id);
        }
    }

    /// The next (`direction` 1) or previous (-1) row with a finding, wrapping around.
    pub(super) fn next_finding(&mut self, direction: isize, excluded: &TaskNames) {
        let Some(rows) = self.rows() else { return };
        let mismatches = &self.analysis.mismatches;
        let len = rows.len() as isize;
        let pos = rows.iter().position(|r| Some(r.id) == self.selected);
        let start = pos.map_or(if direction < 0 { len } else { -1 }, |p| p as isize);
        let next = (1..=len)
            .map(|step| (start + direction * step).rem_euclid(len) as usize)
            .find(|&i| review::has_finding(&rows[i], mismatches, excluded));
        if let Some(i) = next {
            self.selected = Some(rows[i].id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn month_and_rows() {
        let mut r = Review::new(date(4));
        assert_eq!(r.month(), NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), "the previous month");
        assert!(r.rows().is_none());

        let november = date(1);
        let (stale, _) = r.begin_load();
        assert!(r.set_month(date(20)));
        let (generation, month) = r.begin_load();
        assert_eq!(month, november);
        assert!(!r.apply(stale, month, vec![time_row(1, 2)], None), "an older load");
        assert!(!r.apply(generation, november - Months::new(1), vec![time_row(1, 2)], None), "another month");
        assert!(r.rows().is_none());

        assert!(r.apply(generation, month, vec![time_row(1, 2), time_row(2, 3)], None));
        assert_eq!(r.rows().map(<[_]>::len), Some(2));
        assert_eq!(r.selected_id(), Some(1), "the first row");

        assert!(!r.set_month(date(30)));
        assert!(r.rows().is_some(), "the same month keeps its rows");
        r.invalidate();
        assert!(r.rows().is_none());

        assert!(r.set_month(november - Months::new(1)));
        assert!(r.rows.is_empty() && r.selected_id().is_none(), "stepping clears the rows");
    }
}
