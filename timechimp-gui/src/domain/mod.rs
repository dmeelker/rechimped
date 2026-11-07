pub mod report;
pub mod time;
pub mod week;

use timechimp::MileageType;

/// Names are a best guess from what the API returns.
pub const MILEAGE_TYPES: [(MileageType, &str); 2] =
    [(MileageType::Business, "Business"), (MileageType::HomeWork, "Home – work")];

pub fn project_label(project: &str, customer: &str) -> String {
    format!("{project} — {customer}")
}

pub fn mileage_type_name(mileage_type: MileageType) -> String {
    MILEAGE_TYPES
        .iter()
        .find(|(t, _)| *t == mileage_type)
        .map_or_else(|| format!("type {}", i32::from(mileage_type)), |(_, n)| n.to_string())
}

pub fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}

pub fn plain_number(value: f64) -> String {
    let s = format!("{value:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
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
    fn matching_addresses_filters_case_insensitively() {
        let addresses = ["Stationsplein 1, Utrecht".to_owned(), "Damrak 1, Amsterdam".to_owned()];
        assert_eq!(matching_addresses(&addresses, "  utrecht "), ["Stationsplein 1, Utrecht"]);
        assert_eq!(matching_addresses(&addresses, ""), ["Stationsplein 1, Utrecht", "Damrak 1, Amsterdam"]);
        assert!(matching_addresses(&addresses, "Rotterdam").is_empty());
    }
}
