use chrono::NaiveDate;
use gpui::{Context, Window};

use crate::domain::week::touches_week;

use super::{Overlay, Tab, TimeChimpApp};

/// Where a saved or deleted entry was changed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Change {
    /// The Time or Travel tab, whose week holds the entry.
    Week,
    /// The Review tab; `dates` are the entry's dates (the old one too, when it moved).
    Review { dates: Vec<NaiveDate> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Load {
    Week { select: Option<i64> },
    Review { select: Option<i64> },
    Report,
}

impl TimeChimpApp {
    /// After a save or delete: drops what it made stale (cancelling loads still running for it)
    /// and reloads what shows it. `select` is the saved entry's id.
    pub(super) fn after_entries_changed(
        &mut self,
        change: &Change,
        select: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.invalidate_derived();
        for load in plan_reloads(self.tab, change, self.session.week().week_start(), select) {
            match load {
                Load::Week { select } => self.load_week(select, window, cx),
                Load::Review { select } => self.load_review(select, window, cx),
                Load::Report => self.load_report(window, cx),
            }
        }
        if matches!(self.overlay, Overlay::Chart(_)) {
            self.load_chart(window, cx);
        }
    }
}

/// What to reload after a change made from `change`, with `tab` showing and the Time tab on the
/// week of `week_start`.
pub(super) fn plan_reloads(tab: Tab, change: &Change, week_start: NaiveDate, select: Option<i64>) -> Vec<Load> {
    let mut loads = Vec::new();
    match change {
        Change::Week => loads.push(Load::Week { select }),
        Change::Review { dates } => {
            // The Time tab keeps its day: its week reloads only when it holds one of the dates.
            if touches_week(week_start, dates) {
                loads.push(Load::Week { select: None });
            }
            loads.push(Load::Review { select });
        }
    }
    // The tab showing (switched to while the write ran) would read "Not loaded" until R.
    match (tab, change) {
        (Tab::Report, _) => loads.push(Load::Report),
        (Tab::Review, Change::Week) => loads.push(Load::Review { select: None }),
        _ => {}
    }
    loads
}

#[cfg(test)]
mod tests {
    use super::Load::*;
    use super::*;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }

    /// The week of Monday 2 November.
    fn plan(tab: Tab, change: &Change) -> Vec<Load> {
        plan_reloads(tab, change, date(2), Some(7))
    }

    #[test]
    fn a_week_change_reloads_the_week_selecting_the_entry() {
        assert_eq!(plan(Tab::Week, &Change::Week), [Week { select: Some(7) }]);
        assert_eq!(plan(Tab::Travel, &Change::Week), [Week { select: Some(7) }]);
    }

    #[test]
    fn a_review_change_reloads_the_review_and_the_week_holding_it() {
        let inside = Change::Review { dates: vec![date(4)] };
        let outside = Change::Review { dates: vec![date(20)] };
        let moved_out = Change::Review { dates: vec![date(20), date(4)] };
        assert_eq!(plan(Tab::Review, &inside), [Week { select: None }, Review { select: Some(7) }]);
        assert_eq!(plan(Tab::Review, &outside), [Review { select: Some(7) }], "the Time tab's week is elsewhere");
        assert_eq!(plan(Tab::Review, &moved_out), [Week { select: None }, Review { select: Some(7) }], "the old date");
        assert_eq!(plan(Tab::Week, &outside), [Review { select: Some(7) }], "switched to Time meanwhile");
        assert_eq!(plan(Tab::Travel, &inside), [Week { select: None }, Review { select: Some(7) }]);
    }

    #[test]
    fn the_report_or_review_showing_reloads() {
        assert_eq!(plan(Tab::Report, &Change::Week), [Week { select: Some(7) }, Report], "switched to Report meanwhile");
        assert_eq!(
            plan(Tab::Review, &Change::Week),
            [Week { select: Some(7) }, Review { select: None }],
            "switched to Review meanwhile: the entry's id may be a trip's"
        );
        let outside = Change::Review { dates: vec![date(20)] };
        assert_eq!(plan(Tab::Report, &outside), [Review { select: Some(7) }, Report]);
    }
}
