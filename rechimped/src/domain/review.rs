use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use timechimp_api::TimeReportRow;

use super::is_rejected;
use super::week::{TaskNames, is_excluded_task};

/// Whether an entry's notes say anything: missing, empty and whitespace-only notes don't.
pub fn has_notes(notes: Option<&str>) -> bool {
    notes.is_some_and(|n| !n.trim().is_empty())
}

/// A report row's tags are one comma-separated string of names.
pub fn tag_names(tags: Option<&str>) -> Vec<String> {
    tags.into_iter().flat_map(|t| t.split(',')).map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

/// The No-notes finding. Leave (the excluded tasks) usually has no notes, so it is never flagged.
pub fn lacks_notes(notes: Option<&str>, project: &str, task: &str, excluded: &TaskNames) -> bool {
    !has_notes(notes) && !is_excluded_task(project, task, excluded)
}

fn row_lacks_notes(row: &TimeReportRow, excluded: &TaskNames) -> bool {
    lacks_notes(row.notes.as_deref(), &row.project, &row.task, excluded)
}

fn row_rejected(row: &TimeReportRow) -> bool {
    is_rejected(row.status, row.client_status)
}

/// Notes that differ only in case or whitespace are the same notes; punctuation counts.
pub fn note_key(notes: &str) -> String {
    notes.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Why a row's tags stand out among the rows with the same project, task and notes.
#[derive(Debug, Clone, PartialEq)]
pub enum TagMismatch {
    /// The row differs from the one most common set of tags, which `others` rows have.
    Majority { others: usize, tags: Vec<String> },
    /// No set is the most common: every set with its count, in the label's order.
    Tie(Vec<(usize, Vec<String>)>),
}

impl TagMismatch {
    pub fn label(&self) -> String {
        let tags = |tags: &[String]| if tags.is_empty() { "no tags".to_owned() } else { tags.join(" + ") };
        match self {
            Self::Majority { others, tags: set } => format!("Tags differ: {others} others have {}", tags(set)),
            Self::Tie(variants) => {
                let variants: Vec<_> = variants.iter().map(|(n, set)| format!("{n}× {}", tags(set))).collect();
                format!("Tags differ: {}", variants.join(", "))
            }
        }
    }
}

/// A row's tags by lowercased name, each spelled as on the row: compared as a set, ignoring case
/// and order.
fn tag_set(row: &TimeReportRow) -> BTreeMap<String, String> {
    let mut set = BTreeMap::new();
    for name in tag_names(row.tags.as_deref()) {
        set.entry(name.to_lowercase()).or_insert(name);
    }
    set
}

/// The rows whose tags differ from others with the same project, task and notes, by row id. Rows
/// without notes are left out: they have a finding of their own.
pub fn tag_mismatches(rows: &[TimeReportRow]) -> HashMap<i64, TagMismatch> {
    let mut groups: HashMap<(i64, i64, String), Vec<&TimeReportRow>> = HashMap::new();
    for row in rows {
        if let Some(notes) = row.notes.as_deref().filter(|n| has_notes(Some(n))) {
            groups.entry((row.project_id, row.task_id, note_key(notes))).or_default().push(row);
        }
    }
    let mut marked = HashMap::new();
    for group in groups.values() {
        let sets: Vec<_> = group.iter().map(|r| tag_set(r)).collect();
        // Each distinct set (spelled as its first row has it) with its count.
        let mut variants: Vec<(&BTreeMap<String, String>, usize)> = Vec::new();
        for set in &sets {
            match variants.iter_mut().find(|(v, _)| v.keys().eq(set.keys())) {
                Some((_, n)) => *n += 1,
                None => variants.push((set, 1)),
            }
        }
        if variants.len() < 2 {
            continue;
        }
        let shown = |set: &BTreeMap<String, String>| set.values().cloned().collect::<Vec<_>>();
        let top = variants.iter().map(|&(_, n)| n).max().unwrap_or_default();
        let leaders: Vec<_> = variants.iter().filter(|&&(_, n)| n == top).collect();
        if let [&(majority, others)] = leaders[..] {
            let mismatch = TagMismatch::Majority { others, tags: shown(majority) };
            for (row, set) in group.iter().zip(&sets) {
                if !set.keys().eq(majority.keys()) {
                    marked.insert(row.id, mismatch.clone());
                }
            }
        } else {
            variants
                .sort_by(|(a, m), (b, n)| n.cmp(m).then(a.is_empty().cmp(&b.is_empty())).then_with(|| a.keys().cmp(b.keys())));
            let tie = TagMismatch::Tie(variants.iter().map(|&(set, n)| (n, shown(set))).collect());
            for row in group {
                marked.insert(row.id, tie.clone());
            }
        }
    }
    marked
}

pub fn has_finding(row: &TimeReportRow, mismatches: &HashMap<i64, TagMismatch>, excluded: &TaskNames) -> bool {
    row_rejected(row) || row_lacks_notes(row, excluded) || mismatches.contains_key(&row.id)
}

/// How many rows have each finding, for the summary line.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Findings {
    pub rejected: usize,
    pub no_notes: usize,
    pub tags_differ: usize,
}

impl Findings {
    pub fn of(rows: &[TimeReportRow], mismatches: &HashMap<i64, TagMismatch>, excluded: &TaskNames) -> Self {
        Self {
            rejected: rows.iter().filter(|r| row_rejected(r)).count(),
            no_notes: rows.iter().filter(|r| row_lacks_notes(r, excluded)).count(),
            tags_differ: rows.iter().filter(|r| mismatches.contains_key(&r.id)).count(),
        }
    }

    pub fn any(&self) -> bool {
        self.rejected + self.no_notes + self.tags_differ > 0
    }

    pub fn label(&self) -> String {
        let parts: Vec<_> =
            [(self.rejected, "rejected"), (self.no_notes, "without notes"), (self.tags_differ, "with different tags")]
                .into_iter()
                .filter(|&(n, _)| n > 0)
                .map(|(n, what)| format!("{n} {what}"))
                .collect();
        if parts.is_empty() { "No findings".into() } else { parts.join(" · ") }
    }
}

/// One line of the review list: drawn in this order, so its position is the list's item index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Line {
    Day {
        date: NaiveDate,
        total: f64,
    },
    /// An index into the rows.
    Entry(usize),
}

/// A day header before each day's rows; `rows` are sorted by date.
pub fn lines(rows: &[TimeReportRow]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    for day in rows.chunk_by(|a, b| a.date == b.date) {
        lines.push(Line::Day { date: day[0].date, total: super::total(day.iter().map(|r| r.time)) });
        lines.extend((start..start + day.len()).map(Line::Entry));
        start += day.len();
    }
    lines
}

/// What the review list draws that is worked out from the whole month: kept with the rows and
/// redone only when they or the excluded tasks change, not per frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Analysis {
    pub lines: Vec<Line>,
    pub mismatches: HashMap<i64, TagMismatch>,
    pub findings: Findings,
}

impl Analysis {
    pub fn of(rows: &[TimeReportRow], excluded: &TaskNames) -> Self {
        let mismatches = tag_mismatches(rows);
        let findings = Findings::of(rows, &mismatches, excluded);
        Self { lines: lines(rows), mismatches, findings }
    }

    /// The line that draws `rows[index]`.
    pub fn line_of(&self, index: usize) -> Option<usize> {
        self.lines.iter().position(|l| *l == Line::Entry(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::config::default_excluded_tasks;
    use crate::test_support::{date, noted_row, report_row};

    /// Rows with these notes and tags, ids from 1, on project 1 / task 1.
    fn rows(rows: &[(Option<&str>, Option<&str>)]) -> Vec<TimeReportRow> {
        rows.iter().zip(1..).map(|(&(notes, tags), id)| noted_row(id, notes, tags)).collect()
    }

    fn marked(rows: &[TimeReportRow]) -> Vec<i64> {
        let mut ids: Vec<_> = tag_mismatches(rows).into_keys().collect();
        ids.sort();
        ids
    }

    #[test]
    fn leave_needs_no_notes() {
        let excluded = default_excluded_tasks();
        assert!(lacks_notes(None, "P", "T", &excluded));
        assert!(lacks_notes(Some(" "), "P", "T", &excluded));
        assert!(!lacks_notes(Some("x"), "P", "T", &excluded));
        assert!(!lacks_notes(None, "intern", "verlof", &excluded), "leave, by name, case-insensitively");
        assert!(!lacks_notes(None, "Intern", "Ziek", &excluded));
        assert!(lacks_notes(None, "Intern", "Meeting", &excluded));
        let leave = TimeReportRow { project: "Intern".into(), task: "Verlof".into(), ..noted_row(1, None, None) };
        let findings = Findings::of(&[leave, noted_row(2, None, None)], &HashMap::new(), &excluded);
        assert_eq!(findings.no_notes, 1);
    }

    #[test]
    fn notes_match_ignoring_case_and_whitespace() {
        assert_eq!(note_key("  Refactor 	 login "), note_key("refactor login"));
        assert_ne!(note_key("Single Sign On"), note_key("Single sign-on"), "punctuation counts");
        let rows =
            rows(&[(Some("Refactor  login "), Some("w")), (Some("refactor login"), None), (Some("Refactor login."), None)]);
        assert_eq!(marked(&rows), [1, 2], "the third has other notes");
    }

    #[test]
    fn groups_are_per_project_task_and_notes() {
        let mut rows =
            rows(&[(Some("x"), Some("w")), (Some("x"), None), (Some("x"), None), (None, Some("w")), (Some(" "), None)]);
        rows[1].project_id = 2;
        rows[2].task_id = 2;
        assert!(marked(&rows).is_empty(), "another project or task, or no notes");
    }

    #[test]
    fn tags_compare_as_sets_ignoring_case() {
        let rows = rows(&[(Some("x"), Some("a, B")), (Some("x"), Some("b,A")), (Some("x"), Some(" A ,b,a"))]);
        assert!(marked(&rows).is_empty());
    }

    #[test]
    fn the_odd_ones_out_of_a_majority() {
        let rows = rows(&[
            (Some("x"), Some("wbso_tournament")),
            (Some("x"), Some("WBSO_tournament")),
            (Some("x"), None),
            (Some("x"), Some("wbso_tournament")),
            (Some("x"), Some("other")),
        ]);
        let mismatches = tag_mismatches(&rows);
        let mut ids: Vec<_> = mismatches.keys().copied().collect();
        ids.sort();
        assert_eq!(ids, [3, 5], "3/1/1: only the 1s");
        assert_eq!(mismatches[&3], TagMismatch::Majority { others: 3, tags: vec!["wbso_tournament".into()] }, "first spelling");
        assert_eq!(mismatches[&3].label(), "Tags differ: 3 others have wbso_tournament");
        let none = TagMismatch::Majority { others: 2, tags: vec![] };
        assert_eq!(none.label(), "Tags differ: 2 others have no tags");
    }

    #[test]
    fn a_tie_marks_every_row() {
        let rows_1_1 = rows(&[(Some("Test performance"), Some("wbso_tournament")), (Some("test performance"), None)]);
        let mismatches = tag_mismatches(&rows_1_1);
        assert_eq!(marked(&rows_1_1), [1, 2]);
        assert_eq!(mismatches[&1].label(), "Tags differ: 1× wbso_tournament, 1× no tags");
        assert_eq!(mismatches[&1], mismatches[&2]);

        let rows_2_2_1 = rows(&[
            (Some("x"), None),
            (Some("x"), Some("b")),
            (Some("x"), Some("b, a")),
            (Some("x"), None),
            (Some("x"), Some("b")),
        ]);
        assert_eq!(marked(&rows_2_2_1), [1, 2, 3, 4, 5]);
        assert_eq!(
            tag_mismatches(&rows_2_2_1)[&3].label(),
            "Tags differ: 2× b, 2× no tags, 1× a + b",
            "by count, then alphabetically with no tags last"
        );
    }

    #[test]
    fn equal_tags_are_no_finding() {
        assert!(marked(&rows(&[(Some("x"), Some("a")), (Some("x"), Some("a"))])).is_empty());
        assert!(marked(&rows(&[(Some("x"), None), (Some("x"), None)])).is_empty());
        assert!(marked(&rows(&[(Some("x"), Some("a"))])).is_empty());
    }

    #[test]
    fn findings_label() {
        let label = |rejected, no_notes, tags_differ| Findings { rejected, no_notes, tags_differ }.label();
        assert_eq!(label(1, 4, 2), "1 rejected · 4 without notes · 2 with different tags");
        assert_eq!(label(0, 4, 2), "4 without notes · 2 with different tags");
        assert_eq!(label(0, 0, 2), "2 with different tags");
        assert_eq!(label(0, 1, 0), "1 without notes");
        assert_eq!(label(2, 0, 0), "2 rejected");
        assert_eq!(label(0, 0, 0), "No findings");
        assert!(!Findings::default().any());

        let mut rows = rows(&[(None, None), (Some("x"), Some("a")), (Some("x"), None), (Some("y"), None), (Some("z"), None)]);
        rows[3].status = -1;
        rows[4].client_status = -1;
        let mismatches = tag_mismatches(&rows);
        let findings = Findings::of(&rows, &mismatches, &TaskNames::new());
        assert_eq!(findings, Findings { rejected: 2, no_notes: 1, tags_differ: 2 });
        assert!(findings.any());
        let found: Vec<_> = rows.iter().map(|r| has_finding(r, &mismatches, &TaskNames::new())).collect();
        assert_eq!(found, [true, true, true, true, true], "rejected internally or by the client");
        rows[3].status = 0;
        assert!(!has_finding(&rows[3], &mismatches, &TaskNames::new()));
    }

    #[test]
    fn notes_need_more_than_whitespace() {
        assert!(has_notes(Some("Refactor login")));
        assert!(has_notes(Some("  x ")));
        assert!(!has_notes(None));
        assert!(!has_notes(Some("")));
        assert!(!has_notes(Some(" \t\n ")));
    }

    #[test]
    fn tags_are_split_on_commas() {
        assert_eq!(tag_names(None), Vec::<String>::new());
        assert_eq!(tag_names(Some("")), Vec::<String>::new());
        assert_eq!(tag_names(Some("wbso_tournament")), ["wbso_tournament"]);
        assert_eq!(tag_names(Some(" a , B,, c ,")), ["a", "B", "c"], "trimmed, empty parts dropped, case kept");
    }

    #[test]
    fn a_header_per_day() {
        let rows = [report_row("P", "T", 2, 1.5, true), report_row("P", "T", 2, 0.25, true), report_row("P", "T", 4, 8.0, false)];
        assert_eq!(
            lines(&rows),
            [
                Line::Day { date: date(2), total: 1.75 },
                Line::Entry(0),
                Line::Entry(1),
                Line::Day { date: date(4), total: 8.0 },
                Line::Entry(2),
            ]
        );
        assert_eq!(lines(&[]), []);
    }

    #[test]
    fn analysis_of_a_month() {
        let excluded = default_excluded_tasks();
        let mut rows = [noted_row(1, Some("x"), Some("a")), noted_row(2, Some("x"), None), noted_row(3, None, None)];
        rows[2].client_status = -1;
        let analysis = Analysis::of(&rows, &excluded);
        assert_eq!(analysis.findings, Findings { rejected: 1, no_notes: 1, tags_differ: 2 });
        assert_eq!(analysis.lines, lines(&rows));
        assert_eq!(analysis.mismatches, tag_mismatches(&rows));
        assert_eq!(analysis.findings, Findings::of(&rows, &analysis.mismatches, &excluded));
        assert_eq!(analysis.line_of(0), Some(1), "after the day header");
        assert_eq!(analysis.line_of(3), None);
        assert_eq!(Analysis::of(&[], &excluded), Analysis::default());
    }
}
