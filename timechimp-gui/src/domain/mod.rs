//! UI-free logic: parsing what is typed, and deriving what is shown from the loaded data.

pub mod report;
pub mod time;
pub mod week;

use timechimp::MileageType;

/// Mileage trip types as observed in the API (names are a best guess).
pub const MILEAGE_TYPES: [(MileageType, &str); 2] =
    [(MileageType::Business, "Business"), (MileageType::HomeWork, "Home – work")];

/// How a project is listed in the forms: "Project — Customer".
pub fn project_label(project: &str, customer: &str) -> String {
    format!("{project} — {customer}")
}

pub fn json_strings(value: &serde_json::Value) -> Option<Vec<String>> {
    Some(value.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
}

pub fn mileage_type_name(mileage_type: MileageType) -> String {
    MILEAGE_TYPES
        .iter()
        .find(|(t, _)| *t == mileage_type)
        .map_or_else(|| format!("type {}", i32::from(mileage_type)), |(_, n)| n.to_string())
}
