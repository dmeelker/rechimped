use std::sync::Arc;

use chrono::NaiveDate;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::auth::Authenticator;
use crate::error::{Error, Result};
use crate::models::*;

const API_BASE: &str = "https://web.timechimp.com/api/";
const AI_API_BASE: &str = "https://ai.timechimp.com/api/v1/";

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    auth: Arc<Authenticator>,
    base: String,
    ai_base: String,
}

impl Client {
    pub fn new(auth: Authenticator) -> Self {
        Self { http: auth.http.clone(), auth: Arc::new(auth), base: API_BASE.into(), ai_base: AI_API_BASE.into() }
    }

    /// `base` needs a trailing `/`.
    #[doc(hidden)]
    pub fn with_base_url(auth: Authenticator, base: impl Into<String>) -> Self {
        let base = base.into();
        Self { http: auth.http.clone(), auth: Arc::new(auth), ai_base: base.clone(), base }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn authenticator(&self) -> &Authenticator {
        &self.auth
    }

    pub async fn current_user(&self) -> Result<CurrentUser> {
        self.get("user/current").await
    }

    pub async fn current_user_permissions(&self) -> Result<Vec<String>> {
        self.get("user/current/permissions").await
    }

    pub async fn portal_routing(&self) -> Result<PortalRouting> {
        self.get("user/portalrouting").await
    }

    pub async fn two_factor_status(&self) -> Result<TwoFactorStatus> {
        self.get("user/twofactorauthentication").await
    }

    pub async fn sso_enforced(&self) -> Result<bool> {
        self.get("user/ssoenforced").await
    }

    pub async fn validate_user(&self, user_id: i64) -> Result<bool> {
        self.get(&format!("user/validate/{user_id}")).await
    }

    pub async fn users(&self) -> Result<Vec<UserSummary>> {
        self.get("user/nonactive").await
    }

    pub async fn company(&self) -> Result<Company> {
        self.get("company").await
    }

    pub async fn companies(&self) -> Result<Vec<CompanySummary>> {
        self.get("company/uiselect").await
    }

    pub async fn subscription(&self) -> Result<Subscription> {
        self.get("subscription/current/lite").await
    }

    pub async fn settings(&self) -> Result<CompanySettings> {
        self.get("setting").await
    }

    pub async fn theme(&self) -> Result<Theme> {
        self.get("theme").await
    }

    pub async fn holidays(&self) -> Result<Vec<Holiday>> {
        self.get("holiday").await
    }

    pub async fn teams(&self) -> Result<Vec<Team>> {
        self.get("team/uiselect").await
    }

    pub async fn vat_rates(&self) -> Result<Vec<VatRate>> {
        self.get("vatRate").await
    }

    pub async fn tags(&self, tag_type: TagType) -> Result<Vec<Tag>> {
        self.get(&format!("tag/type/{}", i32::from(tag_type))).await
    }

    pub async fn customers_for_time(&self, user_id: i64) -> Result<Vec<Customer>> {
        self.get(&format!("customer/{user_id}/time/uiselectbyuser")).await
    }

    pub async fn customers_for_expenses(&self, user_id: i64) -> Result<Vec<Customer>> {
        self.get(&format!("customer/{user_id}/expense/uiselectbyuser")).await
    }

    pub async fn all_customers(&self) -> Result<Vec<Customer>> {
        self.get("customer/time/uiselectwithnonactive").await
    }

    pub async fn projects_for_user(&self, user_id: i64) -> Result<Vec<Project>> {
        self.get(&format!("project/{user_id}/uiselectbyuser")).await
    }

    pub async fn all_projects(&self) -> Result<Vec<Project>> {
        self.get("project/uiselectwithnonactive").await
    }

    pub async fn tasks_for_time(&self, user_id: i64) -> Result<Vec<Task>> {
        self.get(&format!("task/{user_id}/time/uiselectbyuser")).await
    }

    pub async fn tasks_for_user(&self, user_id: i64) -> Result<Vec<Task>> {
        self.get(&format!("task/{user_id}/uiselectbyuser")).await
    }

    pub async fn time_entries_for_week(&self, user_id: i64, date: NaiveDate) -> Result<Vec<TimeEntry>> {
        self.get(&format!("time/week/{user_id}/{date}")).await
    }

    pub async fn expenses_for_week(&self, user_id: i64, date: NaiveDate) -> Result<Vec<Value>> {
        self.get(&format!("expense/week/{user_id}/{date}")).await
    }

    pub async fn expense_categories(&self) -> Result<Vec<ExpenseCategory>> {
        self.get("expensecategory/uiselect").await
    }

    pub async fn running_timer(&self) -> Result<Option<Value>> {
        let url = self.url("v2/timer");
        let response = self.send(|| self.http.get(&url)).await?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        decode(&url, response).await.map(Some)
    }

    pub async fn suggestions(&self, date: NaiveDate) -> Result<Vec<Value>> {
        let url = format!("{}suggestions", self.ai_base);
        let body = json!({ "date": date.to_string() });
        let response = self.send(|| self.http.post(&url).json(&body)).await?;
        let value: Value = decode(&url, response).await?;
        Ok(value["suggestions"].as_array().cloned().unwrap_or_default())
    }

    pub async fn favorites(&self) -> Result<Vec<Value>> {
        self.get("favorite").await
    }

    pub async fn active_connectors(&self) -> Result<Vec<Value>> {
        self.get("connector/activeconnectorlist").await
    }

    pub async fn calendar_connections(&self) -> Result<Vec<Value>> {
        self.get("userCalendarConnection").await
    }

    pub async fn customers(&self) -> Result<Vec<Customer>> {
        self.get("customer/uiselect").await
    }

    pub async fn projects(&self) -> Result<Vec<Project>> {
        self.get("project/uiselect").await
    }

    pub async fn tasks(&self) -> Result<Vec<Task>> {
        self.get("task/uiselect").await
    }

    pub async fn project_tasks(&self, project_id: i64) -> Result<Vec<ProjectTask>> {
        self.get(&format!("projecttask/uiselect/project/{project_id}")).await
    }

    pub async fn managed_projects(&self) -> Result<Vec<Value>> {
        self.get("project/projectmanagerprojects").await
    }

    pub async fn is_team_manager(&self, user_id: i64) -> Result<bool> {
        self.get(&format!("team/isTeamManager/{user_id}")).await
    }

    pub async fn saved_views(&self, kind: &str) -> Result<Vec<Value>> {
        self.get(&format!("view/type/{kind}")).await
    }

    pub async fn time_entry(&self, id: i64) -> Result<TimeEntry> {
        self.get(&format!("time/{id}")).await
    }

    pub async fn create_time_entry(&self, entry: &NewTimeEntry) -> Result<TimeEntry> {
        self.post("time", &entry.to_json()).await
    }

    pub async fn update_time_entry(&self, id: i64, entry: &NewTimeEntry) -> Result<TimeEntry> {
        let mut body = entry.to_json();
        body["id"] = id.into();
        self.put(&format!("time/{id}"), &body).await
    }

    pub async fn delete_time_entry(&self, id: i64) -> Result<()> {
        self.delete(&format!("time/{id}")).await
    }

    pub async fn query_time(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        rows: std::ops::Range<u32>,
    ) -> Result<QueryResult<TimeReportRow>> {
        let mut body = report_query(from, to, rows);
        body["valueCols"] = json!([{ "id": "time", "aggFunc": "sum", "displayName": "Hours", "field": "time" }]);
        self.post("time/query", &body).await
    }

    pub async fn mileage_for_week(&self, user_id: i64, date: NaiveDate) -> Result<Vec<Mileage>> {
        self.get(&format!("mileage/week/{user_id}/{date}")).await
    }

    pub async fn create_mileage(&self, mileage: &NewMileage) -> Result<Mileage> {
        self.post("mileage", &mileage.to_json()).await
    }

    /// Mirrors the web app, which carries the approval statuses over rather than resetting them.
    pub async fn update_mileage(&self, original: &Mileage, mileage: &NewMileage) -> Result<Mileage> {
        let mut body = mileage.to_json();
        body["id"] = original.id.into();
        body["status"] = original.status.into();
        body["statusIntern"] = original.status_intern.into();
        body["statusExtern"] = original.status_extern.into();
        self.put(&format!("mileage/{}", original.id), &body).await
    }

    pub async fn delete_mileage(&self, id: i64) -> Result<()> {
        self.delete(&format!("mileage/{id}")).await
    }

    pub async fn query_mileage(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        rows: std::ops::Range<u32>,
    ) -> Result<QueryResult<MileageReportRow>> {
        self.post("mileage/query", &report_query(from, to, rows)).await
    }

    pub async fn customers_for_mileage(&self, user_id: i64) -> Result<Vec<Customer>> {
        self.get(&format!("customer/{user_id}/mileage/uiselectbyuser")).await
    }

    pub async fn all_mileage_customers(&self) -> Result<Vec<Customer>> {
        self.get("customer/mileage/uiselectwithnonactive").await
    }

    pub async fn mileage_vehicles(&self, user_id: i64) -> Result<Vec<Value>> {
        self.get(&format!("mileageVehicle/user/{user_id}")).await
    }

    pub async fn place_predictions(&self, query: &str) -> Result<Vec<PlacePrediction>> {
        let maps = self.get_maps(&["places", query]).await?;
        let url = self.url("googlemaps/places");
        serde_json::from_value(maps["predictions"].clone()).map_err(|source| Error::Decode { url, source })
    }

    pub async fn route_distance(&self, from_place_id: &str, to_place_id: &str) -> Result<RouteDistance> {
        let maps = self.get_maps(&["distance", from_place_id, to_place_id]).await?;
        let element = &maps["rows"][0]["elements"][0];
        if element["status"] != "OK" {
            let status = element["status"].as_str().unwrap_or("no result");
            return Err(Error::NotFound(format!("No route found between these addresses ({status})")));
        }
        Ok(RouteDistance {
            origin: maps["origin_addresses"][0].as_str().unwrap_or_default().to_owned(),
            destination: maps["destination_addresses"][0].as_str().unwrap_or_default().to_owned(),
            meters: element["distance"]["value"].as_u64().unwrap_or_default(),
            seconds: element["duration"]["value"].as_u64().unwrap_or_default(),
        })
    }

    async fn get_maps(&self, segments: &[&str]) -> Result<Value> {
        let mut url = reqwest::Url::parse(&self.base).expect("valid base url");
        url.path_segments_mut().expect("base url").pop_if_empty().push("googlemaps").extend(segments);
        let url = url.to_string();
        let response = self.send(|| self.http.get(&url)).await?;
        // TimeChimp's Google Maps proxy returns the JSON inside a JSON string.
        let inner: String = decode(&url, response).await?;
        serde_json::from_str(&inner).map_err(|source| Error::Decode { url, source })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.url(path);
        let response = self.send(|| self.http.get(&url)).await?;
        decode(&url, response).await
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = self.url(path);
        let response = self.send(|| self.http.post(&url).json(body)).await?;
        decode(&url, response).await
    }

    async fn put<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        let url = self.url(path);
        let response = self.send(|| self.http.put(&url).json(body)).await?;
        decode(&url, response).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let url = self.url(path);
        self.send(|| self.http.delete(&url)).await?;
        Ok(())
    }

    async fn send(&self, build: impl Fn() -> RequestBuilder) -> Result<Response> {
        let token = self.auth.access_token().await?;
        let mut response = build().bearer_auth(&token).header("Accept", "application/json").send().await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let token = self.auth.refresh_after_rejection(&token).await?;
            response = build().bearer_auth(token).header("Accept", "application/json").send().await?;
        }
        if !response.status().is_success() {
            let status = response.status();
            let url = response.url().to_string();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Api { status, url, body });
        }
        Ok(response)
    }
}

/// The report endpoints take ag-Grid's server-side row model request.
fn report_query(from: NaiveDate, to: NaiveDate, rows: std::ops::Range<u32>) -> Value {
    json!({
        "startRow": rows.start,
        "endRow": rows.end,
        "rowGroupCols": [],
        "valueCols": [],
        "pivotCols": [],
        "pivotMode": false,
        "groupKeys": [],
        "filterModel": {
            "Date": {
                "dateFrom": format!("{from} 00:00:00"),
                "dateTo": format!("{to} 00:00:00"),
                "filterType": "date",
                "type": "inRange",
            }
        },
        "sortModel": [],
    })
}

async fn decode<T: DeserializeOwned>(url: &str, response: Response) -> Result<T> {
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Decode { url: url.to_owned(), source })
}
