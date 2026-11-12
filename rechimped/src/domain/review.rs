use chrono::NaiveDate;
use timechimp_api::TimeReportRow;

/// Whether an entry's notes say anything: missing, empty and whitespace-only notes don't.
pub fn has_notes(notes: Option<&str>) -> bool {
    notes.is_some_and(|n| !n.trim().is_empty())
}

/// A report row's tags are one comma-separated string of names.
pub fn tag_names(tags: Option<&str>) -> Vec<String> {
    tags.into_iter().flat_map(|t| t.split(',')).map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

/// One line of the review list: drawn in this order, so its position is the list's child index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Line {
    Day { date: NaiveDate, total: f64 },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{date, report_row};

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
        let rows = [
            report_row("P", "T", 2, 1.5, true),
            report_row("P", "T", 2, 0.25, true),
            report_row("P", "T", 4, 8.0, false),
        ];
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
}
