//! Models inferred from observed responses. Large objects map the most useful fields and keep the
//! rest in `extra`. Dates are returned without an offset; `start`/`end` on time entries are UTC.

use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

/// Parses dates both with and without offset: reads return `2026-11-04T00:00:00`,
/// writes return `2026-11-04T00:00:00Z` / `2026-11-04T09:23:00.7720557Z`.
fn parse_datetime(s: &str) -> Result<NaiveDateTime, String> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.naive_utc());
    }
    s.parse::<NaiveDateTime>().map_err(|e| format!("invalid date '{s}': {e}"))
}

fn datetime<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDateTime, D::Error> {
    parse_datetime(&String::deserialize(d)?).map_err(serde::de::Error::custom)
}

fn opt_datetime<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveDateTime>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| parse_datetime(&s).map_err(serde::de::Error::custom))
        .transpose()
}

/// Treats `null` as the type's default (e.g. an empty `Vec`).
fn null_default<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUser {
    pub id: i64,
    pub user_name: String,
    pub display_name: String,
    pub company_id: i64,
    pub company_name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub company_apps: Vec<String>,
    pub account_type_id: i32,
    pub account_type_name: Option<String>,
    pub role_id: i32,
    pub role_name: Option<String>,
    pub contract_hours: Option<f64>,
    #[serde(default, deserialize_with = "opt_datetime")]
    pub contract_start_date: Option<NaiveDateTime>,
    pub language: Option<String>,
    pub timezone: Option<String>,
    pub start_week_on: Option<String>,
    pub currency: Option<String>,
    pub customer_required: bool,
    pub project_required: bool,
    pub task_required: bool,
    pub time_description_required: bool,
    pub access_all_projects: bool,
    pub is_approver: bool,
    pub is_contractor: bool,
    pub is_locked: bool,
    pub user_availability: Option<UserAvailability>,
    #[serde(default, deserialize_with = "null_default")]
    pub manager_of_team_ids: Vec<i64>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Contracted hours per weekday.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserAvailability {
    pub id: i64,
    pub monday: Option<f64>,
    pub tuesday: Option<f64>,
    pub wednesday: Option<f64>,
    pub thursday: Option<f64>,
    pub friday: Option<f64>,
    pub saturday: Option<f64>,
    pub sunday: Option<f64>,
}

impl UserAvailability {
    /// Hours for a weekday, Monday = 0.
    pub fn for_day(&self, days_from_monday: usize) -> Option<f64> {
        [self.monday, self.tuesday, self.wednesday, self.thursday, self.friday, self.saturday, self.sunday]
            .get(days_from_monday)
            .copied()
            .flatten()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    pub id: i64,
    pub display_name: String,
    pub user_name: String,
    pub team_id: Option<i64>,
    pub team_name: Option<String>,
    pub contract_type_name: Option<String>,
    pub active: bool,
    pub is_locked: bool,
    pub account_type_id: i32,
    pub role_id: i32,
    #[serde(default, deserialize_with = "null_default")]
    pub tags: Vec<String>,
    pub is_contractor: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalRouting {
    pub portal: Option<String>,
    pub active_company_is_shell: bool,
    pub has_own_organisation: bool,
    pub own_organisation_company_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoFactorStatus {
    pub two_factor_authentication: bool,
    pub two_factor_up_to_date: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub name: String,
    pub address: Option<String>,
    pub postal_code: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    pub kvk_number: Option<String>,
    pub vat_number: Option<String>,
    pub logo: Option<String>,
    pub customer_required: bool,
    pub project_required: bool,
    pub task_required: bool,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanySummary {
    pub id: i64,
    pub name: String,
    pub allow_admin_access: bool,
    pub allow_master_access: bool,
    pub is_sandbox_company: bool,
    pub is_master_company: Option<bool>,
    pub logo: Option<String>,
    pub user_role: Option<CompanyRole>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanyRole {
    pub id: i64,
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub has_subscription: bool,
    pub has_subscription_trial: bool,
    pub has_payed_subscription: bool,
    #[serde(default, deserialize_with = "opt_datetime")]
    pub subscription_start_date: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "opt_datetime")]
    pub subscription_end_date: Option<NaiveDateTime>,
    pub subscription_plan: i32,
    pub user_count: i32,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Company-wide form settings (which fields are visible/required) and feature toggles.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanySettings {
    pub time_form_customer_required: bool,
    pub time_form_project_required: bool,
    pub time_form_task_required: bool,
    pub time_form_description_required: bool,
    pub time_form_tag_required: bool,
    pub time_form_start_end_required: bool,
    pub use_timer: bool,
    pub use_breaks: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub time_tracking_device_restrictions: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub id: i64,
    pub main_color: String,
    pub menu_background_color: Option<String>,
    pub menu_color: Option<String>,
    pub menu_logo: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holiday {
    pub id: i64,
    pub company_id: i64,
    pub name: String,
    #[serde(deserialize_with = "datetime")]
    pub date: NaiveDateTime,
    pub blocked: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: i64,
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub user_ids: Vec<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub manager_ids: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VatRate {
    pub id: i64,
    pub company_id: i64,
    pub name: String,
    pub percentage: f64,
    pub show_on_invoice: bool,
    pub active: bool,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: i64,
    pub company_id: i64,
    pub name: String,
    pub active: bool,
    #[serde(rename = "type")]
    pub tag_type: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub active: bool,
    pub relation_id: Option<String>,
    pub hourly_rate: Option<f64>,
    pub intern: bool,
    pub vat_rate_id: Option<i64>,
    pub address: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub customer_id: i64,
    pub customer_name: String,
    pub intern: bool,
    pub unspecified: bool,
    pub active: bool,
    #[serde(default, deserialize_with = "opt_datetime")]
    pub start_date: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "opt_datetime")]
    pub end_date: Option<NaiveDateTime>,
    pub external_url: Option<String>,
    pub external_name: Option<String>,
    pub main_project_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub name: String,
    pub billable: bool,
    pub active: bool,
    pub hourly_rate: Option<f64>,
    pub hourly_rates: Option<Value>,
    pub intern: bool,
    pub common: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntry {
    pub id: i64,
    pub time_row_id: i64,
    pub user_id: i64,
    pub user_display_name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub user_tag_names: Vec<String>,
    pub customer_id: i64,
    pub customer_name: String,
    pub project_id: i64,
    pub project_name: String,
    pub project_code: Option<String>,
    pub project_task_id: i64,
    pub task_id: i64,
    pub task_name: String,
    pub task_code: Option<String>,
    #[serde(deserialize_with = "datetime")]
    pub date: NaiveDateTime,
    pub hours: f64,
    pub notes: Option<String>,
    /// Local `HH:mm-HH:mm` range, if start/end times were registered.
    pub start_end: Option<String>,
    /// UTC.
    #[serde(default, deserialize_with = "opt_datetime")]
    pub start: Option<NaiveDateTime>,
    /// UTC.
    #[serde(default, deserialize_with = "opt_datetime")]
    pub end: Option<NaiveDateTime>,
    pub pause: f64,
    pub billable: bool,
    pub status: i32,
    pub status_intern: i32,
    pub status_extern: i32,
    #[serde(default, deserialize_with = "null_default")]
    pub tag_ids: Vec<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub tag_names: Vec<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub tags: Vec<Tag>,
    pub external_url: Option<String>,
    pub external_name: Option<String>,
    #[serde(deserialize_with = "datetime")]
    pub modified: NaiveDateTime,
    pub timezone: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseCategory {
    pub id: i64,
    pub company_id: i64,
    pub name: String,
    pub active: bool,
    pub code: Option<String>,
    pub ledger_code: Option<String>,
    pub salary_code: Option<String>,
    pub rate: Option<f64>,
    pub cost_rate: Option<f64>,
}

// ---- Project tasks ----------------------------------------------------------------------

/// A task as linked to a specific project; its `id` is the `project_task_id` of a time entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTask {
    pub id: i64,
    pub name: String,
    pub billable: bool,
    pub active: bool,
    pub project_id: i64,
    pub task_id: i64,
}

// ---- Writing time entries ---------------------------------------------------------------

/// Input for creating or updating a time entry.
#[derive(Debug, Clone)]
pub struct NewTimeEntry {
    pub user_id: i64,
    pub customer_id: i64,
    pub project_id: i64,
    /// [`ProjectTask::id`].
    pub project_task_id: i64,
    /// [`ProjectTask::task_id`].
    pub task_id: i64,
    pub date: NaiveDate,
    pub hours: f64,
    pub notes: Option<String>,
    /// Optional start/end times. Use the user's local offset (e.g. via `chrono::Local`):
    /// the local `HH:mm` is shown in the app, the UTC value is stored.
    pub start: Option<DateTime<FixedOffset>>,
    pub end: Option<DateTime<FixedOffset>>,
    pub pause: f64,
    pub tag_ids: Vec<i64>,
    /// IANA timezone name, e.g. `Europe/Amsterdam`.
    pub timezone: Option<String>,
}

impl NewTimeEntry {
    /// An entry of `hours` on `task` of `project`, without start/end times.
    pub fn new(user_id: i64, project: &Project, task: &ProjectTask, date: NaiveDate, hours: f64) -> Self {
        Self {
            user_id,
            customer_id: project.customer_id,
            project_id: project.id,
            project_task_id: task.id,
            task_id: task.task_id,
            date,
            hours,
            notes: None,
            start: None,
            end: None,
            pause: 0.0,
            tag_ids: Vec::new(),
            timezone: None,
        }
    }

    /// Sets start and end, and derives `hours` from them (minus the pause).
    pub fn with_times(mut self, start: DateTime<FixedOffset>, end: DateTime<FixedOffset>) -> Self {
        self.hours = ((end - start).num_minutes() as f64 / 60.0 - self.pause).max(0.0);
        self.start = Some(start);
        self.end = Some(end);
        self
    }

    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }

    /// The request body, in the same shape the web app sends.
    pub(crate) fn to_json(&self) -> Value {
        let mut body = json!({
            "userId": self.user_id,
            "customerId": self.customer_id,
            "projectId": self.project_id,
            "projectTaskId": self.project_task_id,
            "taskId": self.task_id,
            "date": format!("{}T00:00:00.000Z", self.date),
            "hours": self.hours,
            "notes": self.notes.clone().unwrap_or_default(),
            "pause": self.pause,
            "tagIds": self.tag_ids,
            "timer": null,
        });
        if let (Some(start), Some(end)) = (self.start, self.end) {
            let utc = |t: DateTime<FixedOffset>| t.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            let (start_time, end_time) = (start.format("%H:%M").to_string(), end.format("%H:%M").to_string());
            body["start"] = utc(start).into();
            body["end"] = utc(end).into();
            body["startEnd"] = format!("{start_time}-{end_time}").into();
            body["startTime"] = start_time.into();
            body["endTime"] = end_time.into();
        }
        if let Some(tz) = &self.timezone {
            body["timezone"] = tz.clone().into();
        }
        body
    }
}

// ---- Mileage ----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mileage {
    pub id: i64,
    pub user_id: i64,
    pub user_display_name: String,
    pub customer_id: i64,
    pub customer_name: String,
    pub project_id: i64,
    pub project_name: String,
    pub project_code: Option<String>,
    #[serde(deserialize_with = "datetime")]
    pub date: NaiveDateTime,
    pub mileage_vehicle_id: Option<i64>,
    pub mileage_vehicle_name: Option<String>,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub notes: Option<String>,
    /// Kilometres.
    pub distance: f64,
    pub billable: bool,
    /// Trip type. Observed: 1 (business) and 3 (likely home–work); the company settings
    /// enable private, business and living–work types.
    #[serde(rename = "type")]
    pub mileage_type: i32,
    pub status: i32,
    pub status_intern: i32,
    pub status_extern: i32,
    #[serde(deserialize_with = "datetime")]
    pub modified: NaiveDateTime,
}

/// Input for creating a mileage registration. For a return trip the app creates two
/// registrations with the addresses swapped.
#[derive(Debug, Clone)]
pub struct NewMileage {
    pub user_id: i64,
    pub customer_id: i64,
    pub project_id: i64,
    pub date: NaiveDate,
    /// Kilometres.
    pub distance: f64,
    pub from_address: String,
    pub to_address: String,
    pub notes: Option<String>,
    pub billable: bool,
    /// See [`Mileage::mileage_type`].
    pub mileage_type: i32,
    pub mileage_vehicle_id: Option<i64>,
}

impl NewMileage {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "userId": self.user_id,
            "customerId": self.customer_id,
            "projectId": self.project_id,
            "date": format!("{}T00:00:00.000Z", self.date),
            "distance": self.distance,
            "fromAddress": self.from_address,
            "toAddress": self.to_address,
            "notes": self.notes.clone().unwrap_or_default(),
            "billable": self.billable,
            "type": self.mileage_type,
            "mileageVehicleId": self.mileage_vehicle_id,
            "status": 0,
            "statusIntern": 0,
            "statusExtern": 0,
        })
    }
}

// ---- Reports (server-side grid queries) -------------------------------------------------

/// A page of a report query. `row_count` is the total number of matching rows.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult<T> {
    pub row_data: Vec<T>,
    pub row_count: i64,
}

/// A row of the time report (`time/query`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeReportRow {
    pub id: i64,
    pub week: i32,
    #[serde(deserialize_with = "datetime")]
    pub date: NaiveDateTime,
    pub billable: bool,
    pub customer: String,
    pub customer_id: i64,
    pub project: String,
    pub project_id: i64,
    pub project_code: Option<String>,
    pub task: String,
    pub task_id: i64,
    pub user: String,
    pub user_id: i64,
    pub team: Option<String>,
    pub team_id: Option<i64>,
    /// Hours.
    pub time: f64,
    pub notes: Option<String>,
    pub start_end: Option<String>,
    #[serde(default, deserialize_with = "opt_datetime")]
    pub start: Option<NaiveDateTime>,
    #[serde(default, deserialize_with = "opt_datetime")]
    pub end: Option<NaiveDateTime>,
    pub pause: f64,
    pub amount: Option<f64>,
    /// Comma-separated tag names.
    pub tags: Option<String>,
    pub status: i32,
    pub client_status: i32,
    pub invoice_id: Option<i64>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// A row of the mileage report (`mileage/query`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MileageReportRow {
    pub id: i64,
    pub week: i32,
    #[serde(deserialize_with = "datetime")]
    pub date: NaiveDateTime,
    pub user: String,
    pub user_id: i64,
    pub customer: String,
    pub customer_id: i64,
    pub project: String,
    pub project_id: i64,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub notes: Option<String>,
    pub distance: f64,
    pub billable: bool,
    #[serde(rename = "type")]
    pub mileage_type: i32,
    /// Reimbursement amount.
    pub amount: Option<f64>,
    pub status: i32,
    pub team: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ---- Google Maps (proxied by TimeChimp) -------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PlacePrediction {
    pub description: String,
    pub place_id: String,
}

#[derive(Debug, Clone)]
pub struct RouteDistance {
    pub origin: String,
    pub destination: String,
    pub meters: u64,
    pub seconds: u64,
}

impl RouteDistance {
    pub fn kilometres(&self) -> f64 {
        self.meters as f64 / 1000.0
    }
}
