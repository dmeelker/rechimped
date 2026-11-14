pub mod report;
pub mod review;
pub mod time;
pub mod week;

use timechimp_api::MileageType;

/// Names are a best guess from what the API returns.
pub const TRAVEL_TYPES: [(MileageType, &str); 2] = [(MileageType::Business, "Business"), (MileageType::HomeWork, "Home – work")];

pub fn project_label(project: &str, customer: &str) -> String {
    format!("{project} — {customer}")
}

pub fn travel_type_name(travel_type: MileageType) -> String {
    TRAVEL_TYPES
        .iter()
        .find(|(t, _)| *t == travel_type)
        .map_or_else(|| format!("type {}", i32::from(travel_type)), |(_, n)| n.to_string())
}

pub fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}

/// `Iterator::sum` of no floats is -0.0, which formats as "-0.00"; starting from 0.0 keeps it positive.
pub fn total(values: impl IntoIterator<Item = f64>) -> f64 {
    values.into_iter().fold(0.0, |sum, v| sum + v)
}

pub fn plain_number(value: f64) -> String {
    let s = format!("{value:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Hours as `hh:mm`, rounded to the minute: 1.5 → "01:30", 40 → "40:00".
pub fn hh_mm(hours: f64) -> String {
    let minutes = (hours * 60.0).round() as i64;
    let sign = if minutes < 0 { "-" } else { "" };
    let minutes = minutes.abs();
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// TimeChimp's approval status of a time entry, kept internally and for the client. Codes it
/// doesn't know are kept as `Other`: from 1 up they lock the entry, as the web app's check does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Rejected,
    Open,
    PendingApproval,
    Approved,
    Invoiced,
    WrittenOff,
    Other(i32),
}

impl ApprovalStatus {
    pub fn from_code(code: i32) -> Self {
        match code {
            -1 => Self::Rejected,
            0 => Self::Open,
            1 => Self::PendingApproval,
            2 => Self::Approved,
            3 => Self::Invoiced,
            4 => Self::WrittenOff,
            other => Self::Other(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rejected => "Rejected",
            Self::Open => "Open",
            Self::PendingApproval => "Pending approval",
            Self::Approved => "Approved",
            Self::Invoiced => "Invoiced",
            Self::WrittenOff => "Written off",
            Self::Other(_) => "Locked",
        }
    }
}

/// The status that locks an entry (the higher code when the internal or the client one is 1 or
/// more), or None when it can still be changed.
pub fn lock_status(internal: i32, client: i32) -> Option<ApprovalStatus> {
    let code = internal.max(client);
    (code >= 1).then(|| ApprovalStatus::from_code(code))
}

pub fn is_rejected(internal: i32, client: i32) -> bool {
    [internal, client].contains(&-1)
}

pub fn matching_addresses<'a>(addresses: &'a [String], query: &str) -> Vec<&'a str> {
    let query = query.trim().to_lowercase();
    addresses.iter().filter(|a| a.to_lowercase().contains(&query)).map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_trimmed_and_ignoring_case() {
        assert!(same_name(" Intern ", "intern"));
        assert!(same_name("Ébène", "ébène"));
        assert!(!same_name("Intern", "Internal"));
    }

    #[test]
    fn plain_numbers() {
        assert_eq!(plain_number(40.0), "40");
        assert_eq!(plain_number(32.5), "32.5");
        assert_eq!(plain_number(24.75), "24.75");
        assert_eq!(plain_number(0.825 * 100.0), "82.5");
    }

    #[test]
    fn hours_as_hh_mm() {
        assert_eq!(hh_mm(1.5), "01:30");
        assert_eq!(hh_mm(1.0), "01:00");
        assert_eq!(hh_mm(0.0), "00:00");
        assert_eq!(hh_mm(-0.0), "00:00");
        assert_eq!(hh_mm(0.25), "00:15");
        assert_eq!(hh_mm(7.99999), "08:00");
        assert_eq!(hh_mm(40.0), "40:00");
        assert_eq!(hh_mm(160.75), "160:45");
        assert_eq!(hh_mm(-1.25), "-01:15");
    }

    #[test]
    fn approval_statuses() {
        let labels: Vec<_> = (-1..=4).map(|c| ApprovalStatus::from_code(c).label()).collect();
        assert_eq!(labels, ["Rejected", "Open", "Pending approval", "Approved", "Invoiced", "Written off"]);
        assert_eq!(ApprovalStatus::from_code(7), ApprovalStatus::Other(7));
        assert_eq!(ApprovalStatus::Other(7).label(), "Locked");
    }

    #[test]
    fn locked_from_pending_approval_on() {
        assert_eq!(lock_status(0, 0), None);
        assert_eq!(lock_status(-1, 0), None, "rejected stays editable");
        assert_eq!(lock_status(-1, -1), None);
        assert_eq!(lock_status(1, 0), Some(ApprovalStatus::PendingApproval));
        assert_eq!(lock_status(0, 2), Some(ApprovalStatus::Approved), "by the client alone");
        assert_eq!(lock_status(-1, 2), Some(ApprovalStatus::Approved));
        assert_eq!(lock_status(3, 2), Some(ApprovalStatus::Invoiced), "the higher of the two");
        assert_eq!(lock_status(2, 4), Some(ApprovalStatus::WrittenOff));
        assert_eq!(lock_status(9, 0), Some(ApprovalStatus::Other(9)));
        assert_eq!(lock_status(-5, 0), None);
    }

    #[test]
    fn rejected_internally_or_by_the_client() {
        assert!(is_rejected(-1, 0));
        assert!(is_rejected(0, -1));
        assert!(is_rejected(-1, 2));
        assert!(!is_rejected(0, 0));
        assert!(!is_rejected(2, 1));
    }

    #[test]
    fn matching_addresses_filters_case_insensitively() {
        let addresses = ["Stationsplein 1, Utrecht".to_owned(), "Damrak 1, Amsterdam".to_owned()];
        assert_eq!(matching_addresses(&addresses, "  utrecht "), ["Stationsplein 1, Utrecht"]);
        assert_eq!(matching_addresses(&addresses, ""), ["Stationsplein 1, Utrecht", "Damrak 1, Amsterdam"]);
        assert!(matching_addresses(&addresses, "Rotterdam").is_empty());
    }
}
