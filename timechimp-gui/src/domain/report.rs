//! The monthly report and its totals.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use timechimp::{MileageReportRow, TimeReportRow};

#[derive(Default)]
pub struct Report {
    pub month: Option<NaiveDate>,
    pub time: Vec<TimeReportRow>,
    pub mileage: Vec<MileageReportRow>,
}

/// Hours on one project / task in the report.
#[derive(Debug, PartialEq)]
pub struct HoursGroup {
    pub project: String,
    pub task: String,
    pub hours: f64,
    pub billable: bool,
}

/// What the Report tab shows.
#[derive(Debug, PartialEq)]
pub struct Summary {
    /// Hours per project / task, most first.
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
            total: self.time.iter().map(|r| r.time).sum(),
            billable: self.time.iter().filter(|r| r.billable).map(|r| r.time).sum(),
            entries: self.time.len(),
            days: self.time.iter().map(|r| r.date).collect::<BTreeSet<_>>().len(),
            trips: self.mileage.len(),
            km: self.mileage.iter().map(|r| r.distance).sum(),
            reimbursement: self.mileage.iter().filter_map(|r| r.amount).sum(),
        }
    }
}

impl Summary {
    /// `hours` as a fraction of the month's total (0 when nothing was registered).
    pub fn share(&self, hours: f64) -> f64 {
        if self.total > 0.0 { hours / self.total } else { 0.0 }
    }

    /// The billable fraction of the hours registered; None without any.
    pub fn billable_share(&self) -> Option<f64> {
        (self.total > 0.0).then(|| self.billable / self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(project: &str, task: &str, day: u32, hours: f64, billable: bool) -> TimeReportRow {
        serde_json::from_value(serde_json::json!({
            "id": 1, "week": 45, "date": format!("2026-11-{day:02}T00:00:00"), "billable": billable,
            "customer": "C", "customerId": 1, "project": project, "projectId": 1, "projectCode": null,
            "task": task, "taskId": 1, "user": "U", "userId": 1, "team": null, "teamId": null,
            "time": hours, "notes": null, "startEnd": null, "pause": 0.0, "amount": null, "tags": null,
            "status": 0, "clientStatus": 0, "invoiceId": null,
        }))
        .unwrap()
    }

    #[test]
    fn summary() {
        let report = Report {
            month: NaiveDate::from_ymd_opt(2026, 11, 1),
            time: vec![
                row("A", "Dev", 2, 2.0, true),
                row("B", "Meeting", 2, 1.0, false),
                row("A", "Dev", 3, 4.0, true),
            ],
            mileage: Vec::new(),
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
}
