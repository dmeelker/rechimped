//! The TimeChimp calls the app makes, so a session can run against the real API (`Client`) or an
//! in-memory stand-in (`crate::mock::Mock`, `--mock`).

use std::ops::Range;

use chrono::NaiveDate;
use futures::future::BoxFuture;
use timechimp_api::{
    Client, CurrentUser, Mileage, NewMileage, NewTimeEntry, PlacePrediction, Project, ProjectTask, QueryResult,
    RouteDistance, Tag, TagType, TimeEntry, TimeReportRow, MileageReportRow,
};

pub type Call<'a, T> = BoxFuture<'a, timechimp_api::Result<T>>;

pub trait Backend: Send + Sync {
    fn current_user(&self) -> Call<'_, CurrentUser>;
    fn projects_for_user(&self, user_id: i64) -> Call<'_, Vec<Project>>;
    fn tags(&self, tag_type: TagType) -> Call<'_, Vec<Tag>>;
    fn project_tasks(&self, project_id: i64) -> Call<'_, Vec<ProjectTask>>;

    fn time_entries_for_week(&self, user_id: i64, monday: NaiveDate) -> Call<'_, Vec<TimeEntry>>;
    fn time_entry(&self, id: i64) -> Call<'_, TimeEntry>;
    fn create_time_entry<'a>(&'a self, entry: &'a NewTimeEntry) -> Call<'a, TimeEntry>;
    fn update_time_entry<'a>(&'a self, id: i64, entry: &'a NewTimeEntry) -> Call<'a, TimeEntry>;
    fn delete_time_entry(&self, id: i64) -> Call<'_, ()>;
    fn query_time(&self, user_id: i64, from: NaiveDate, to: NaiveDate, rows: Range<u32>)
    -> Call<'_, QueryResult<TimeReportRow>>;

    fn mileage_for_week(&self, user_id: i64, monday: NaiveDate) -> Call<'_, Vec<Mileage>>;
    fn create_mileage<'a>(&'a self, mileage: &'a NewMileage) -> Call<'a, Mileage>;
    fn update_mileage<'a>(&'a self, original: &'a Mileage, mileage: &'a NewMileage) -> Call<'a, Mileage>;
    fn delete_mileage(&self, id: i64) -> Call<'_, ()>;
    fn query_mileage(&self, from: NaiveDate, to: NaiveDate, rows: Range<u32>) -> Call<'_, QueryResult<MileageReportRow>>;

    fn place_predictions<'a>(&'a self, query: &'a str) -> Call<'a, Vec<PlacePrediction>>;
    fn route_distance<'a>(&'a self, from_place_id: &'a str, to_place_id: &'a str) -> Call<'a, RouteDistance>;
}

impl Backend for Client {
    fn current_user(&self) -> Call<'_, CurrentUser> {
        Box::pin(Client::current_user(self))
    }

    fn projects_for_user(&self, user_id: i64) -> Call<'_, Vec<Project>> {
        Box::pin(Client::projects_for_user(self, user_id))
    }

    fn tags(&self, tag_type: TagType) -> Call<'_, Vec<Tag>> {
        Box::pin(Client::tags(self, tag_type))
    }

    fn project_tasks(&self, project_id: i64) -> Call<'_, Vec<ProjectTask>> {
        Box::pin(Client::project_tasks(self, project_id))
    }

    fn time_entries_for_week(&self, user_id: i64, monday: NaiveDate) -> Call<'_, Vec<TimeEntry>> {
        Box::pin(Client::time_entries_for_week(self, user_id, monday))
    }

    fn time_entry(&self, id: i64) -> Call<'_, TimeEntry> {
        Box::pin(Client::time_entry(self, id))
    }

    fn create_time_entry<'a>(&'a self, entry: &'a NewTimeEntry) -> Call<'a, TimeEntry> {
        Box::pin(Client::create_time_entry(self, entry))
    }

    fn update_time_entry<'a>(&'a self, id: i64, entry: &'a NewTimeEntry) -> Call<'a, TimeEntry> {
        Box::pin(Client::update_time_entry(self, id, entry))
    }

    fn delete_time_entry(&self, id: i64) -> Call<'_, ()> {
        Box::pin(Client::delete_time_entry(self, id))
    }

    fn query_time(
        &self,
        user_id: i64,
        from: NaiveDate,
        to: NaiveDate,
        rows: Range<u32>,
    ) -> Call<'_, QueryResult<TimeReportRow>> {
        Box::pin(Client::query_time(self, user_id, from, to, rows))
    }

    fn mileage_for_week(&self, user_id: i64, monday: NaiveDate) -> Call<'_, Vec<Mileage>> {
        Box::pin(Client::mileage_for_week(self, user_id, monday))
    }

    fn create_mileage<'a>(&'a self, mileage: &'a NewMileage) -> Call<'a, Mileage> {
        Box::pin(Client::create_mileage(self, mileage))
    }

    fn update_mileage<'a>(&'a self, original: &'a Mileage, mileage: &'a NewMileage) -> Call<'a, Mileage> {
        Box::pin(Client::update_mileage(self, original, mileage))
    }

    fn delete_mileage(&self, id: i64) -> Call<'_, ()> {
        Box::pin(Client::delete_mileage(self, id))
    }

    fn query_mileage(&self, from: NaiveDate, to: NaiveDate, rows: Range<u32>) -> Call<'_, QueryResult<MileageReportRow>> {
        Box::pin(Client::query_mileage(self, from, to, rows))
    }

    fn place_predictions<'a>(&'a self, query: &'a str) -> Call<'a, Vec<PlacePrediction>> {
        Box::pin(Client::place_predictions(self, query))
    }

    fn route_distance<'a>(&'a self, from_place_id: &'a str, to_place_id: &'a str) -> Call<'a, RouteDistance> {
        Box::pin(Client::route_distance(self, from_place_id, to_place_id))
    }
}

/// What a session talks to.
pub type Api = std::sync::Arc<dyn Backend>;
