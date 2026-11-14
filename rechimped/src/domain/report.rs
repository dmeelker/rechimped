use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use timechimp_api::{MileageReportRow, TimeReportRow};

#[derive(Default)]
pub struct Report {
    pub month: Option<NaiveDate>,
    pub time: Vec<TimeReportRow>,
    pub travel: Vec<MileageReportRow>,
}

#[derive(Debug, PartialEq)]
pub struct HoursGroup {
    pub project: String,
    pub task: String,
    pub hours: f64,
    pub billable: bool,
}

#[derive(Debug, PartialEq)]
pub struct Summary {
    pub groups: Vec<HoursGroup>,
    pub total: f64,
    pub billable: f64,
    pub entries: usize,
    pub days: usize,
    pub trips: usize,
    pub km: f64,
    pub reimbursement: f64,
}

impl Report {
    pub fn summary(&self) -> Summary {
        let mut groups: BTreeMap<(&str, &str), (f64, bool)> = BTreeMap::new();
        for r in &self.time {
            groups.entry((&r.project, &r.task)).or_insert((0.0, r.billable)).0 += r.time;
        }
        let mut groups: Vec<HoursGroup> = groups
            .into_iter()
            .map(|((project, task), (hours, billable))| HoursGroup {
                project: project.to_owned(),
                task: task.to_owned(),
                hours,
                billable,
            })
            .collect();
        groups.sort_by(|a, b| b.hours.total_cmp(&a.hours));
        Summary {
            groups,
            total: super::total(self.time.iter().map(|r| r.time)),
            billable: super::total(self.time.iter().filter(|r| r.billable).map(|r| r.time)),
            entries: self.time.len(),
            days: self.time.iter().map(|r| r.date).collect::<BTreeSet<_>>().len(),
            trips: self.travel.len(),
            km: super::total(self.travel.iter().map(|r| r.distance)),
            reimbursement: super::total(self.travel.iter().filter_map(|r| r.amount)),
        }
    }
}

impl Summary {
    pub fn share(&self, hours: f64) -> f64 {
        if self.total > 0.0 { hours / self.total } else { 0.0 }
    }

    pub fn billable_share(&self) -> Option<f64> {
        (self.total > 0.0).then(|| self.billable / self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::report_row as row;

    #[test]
    fn summary() {
        let report = Report {
            month: Some(crate::test_support::date(1)),
            time: vec![row("A", "Dev", 2, 2.0, true), row("B", "Meeting", 2, 1.0, false), row("A", "Dev", 3, 4.0, true)],
            travel: Vec::new(),
        };
        let s = report.summary();
        assert_eq!(
            s.groups,
            [
                HoursGroup { project: "A".into(), task: "Dev".into(), hours: 6.0, billable: true },
                HoursGroup { project: "B".into(), task: "Meeting".into(), hours: 1.0, billable: false },
            ]
        );
        assert_eq!((s.total, s.billable, s.entries, s.days, s.trips), (7.0, 6.0, 3, 2, 0));
        assert_eq!(s.billable_share(), Some(6.0 / 7.0));
        assert_eq!(Report::default().summary().billable_share(), None);
        assert_eq!(Report::default().summary().share(1.0), 0.0);
    }

    #[test]
    fn empty_sums_are_positive_zero() {
        // An empty `Iterator::sum` of floats is -0.0, shown as "-0.00".
        let s = Report::default().summary();
        for value in [s.total, s.billable, s.km, s.reimbursement] {
            assert_eq!(format!("{value:.2}"), "0.00");
        }
    }
}
