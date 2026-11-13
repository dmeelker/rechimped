use std::future::Future;
use std::sync::OnceLock;

use chrono::{Duration, NaiveDate};
use futures::{StreamExt, TryStreamExt};
use timechimp_api::{MileageReportRow, ProjectTask, QueryResult, TagType, TimeEntry, TimeReportRow};

use crate::backend::Api;
use crate::domain::report::Report;
use crate::domain::time::{last_of_month, monday_of};
use crate::session::{Base, Week};

/// gpui has its own executor, which reqwest can't use.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime"))
}

pub async fn load_base(client: &Api) -> timechimp_api::Result<Base> {
    let me = client.current_user().await?;
    let (projects, tags) = tokio::try_join!(client.projects_for_user(me.id), client.tags(TagType::Time))?;
    // The stream owns everything it uses: borrowed ids or client trip up the Send check of the
    // spawned future (a higher-ranked lifetime).
    let ids: Vec<i64> = projects.iter().map(|p| p.id).collect();
    let owned = client.clone();
    let project_tasks: Vec<Vec<ProjectTask>> = futures::stream::iter(ids)
        .map(move |id| {
            let client = owned.clone();
            async move { client.project_tasks(id).await }
        })
        .buffered(MAX_CONCURRENT_REQUESTS)
        .try_collect()
        .await?;
    Ok(Base {
        project_tasks: projects.iter().map(|p| p.id).zip(project_tasks).collect(),
        me,
        projects,
        tags: tags.into_iter().filter(|t| t.active).collect(),
    })
}

pub async fn load_week(client: &Api, user_id: i64, week_start: NaiveDate) -> timechimp_api::Result<Week> {
    let (mut entries, mut travel) =
        tokio::try_join!(client.time_entries_for_week(user_id, week_start), client.mileage_for_week(user_id, week_start))?;
    entries.sort_by_key(|e| (e.date, e.start));
    // Per day in the order they were registered (outbound before return), so the Travel tab can
    // group consecutive rows by day and ↑/↓ follow what's on screen.
    travel.sort_by_key(|m| (m.date, m.id));
    Ok(Week { start: week_start, entries, travel })
}

const HISTORY_WEEKS: i64 = 5;

const MAX_CONCURRENT_REQUESTS: usize = 6;

/// Skips the week starting on `loaded`: `load_week` fetches that one already.
pub async fn load_history(
    client: &Api,
    user_id: i64,
    today: NaiveDate,
    loaded: NaiveDate,
) -> timechimp_api::Result<Vec<TimeEntry>> {
    let this_week = monday_of(today);
    let mondays: Vec<NaiveDate> =
        (0..HISTORY_WEEKS).map(|k| this_week - Duration::weeks(k)).filter(|monday| *monday != loaded).collect();
    load_weeks(client, user_id, &mondays).await
}

pub async fn load_weeks(client: &Api, user_id: i64, mondays: &[NaiveDate]) -> timechimp_api::Result<Vec<TimeEntry>> {
    let weeks = mondays.iter().map(|&monday| client.time_entries_for_week(user_id, monday));
    Ok(futures::future::try_join_all(weeks).await?.into_iter().flatten().collect())
}

async fn page_all<T, Fut>(mut fetch: impl FnMut(std::ops::Range<u32>) -> Fut) -> timechimp_api::Result<Vec<T>>
where
    Fut: Future<Output = timechimp_api::Result<QueryResult<T>>>,
{
    let mut rows = Vec::new();
    loop {
        let start = rows.len() as u32;
        let page = fetch(start..start + PAGE).await?;
        let done = page.row_data.is_empty() || (start as i64 + page.row_data.len() as i64) >= page.row_count;
        rows.extend(page.row_data);
        if done {
            return Ok(rows);
        }
    }
}

const PAGE: u32 = 500;

pub async fn load_time_rows(
    client: &Api,
    user_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> timechimp_api::Result<Vec<TimeReportRow>> {
    let mut rows = page_all(|rows| client.query_time(user_id, from, to, rows)).await?;
    // The query filters on the user already; this guards against the server ignoring that.
    rows.retain(|r| r.user_id == user_id);
    rows.sort_by_key(|r| (r.date, r.start));
    Ok(rows)
}

pub async fn load_report(client: &Api, user_id: i64, month: NaiveDate) -> timechimp_api::Result<Report> {
    let to = last_of_month(month);
    let (time, mut travel) = tokio::try_join!(
        load_time_rows(client, user_id, month, to),
        page_all(|rows| client.query_mileage(month, to, rows)),
    )?;
    // The mileage report includes everyone the caller can see.
    travel.retain(|r: &MileageReportRow| r.user_id == user_id);
    Ok(Report { month: Some(month), time, travel })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use timechimp_api::{MileageType, NewMileage, NewTimeEntry};

    use crate::backend::Backend;
    use crate::domain::time::first_of_month;
    use crate::mock::Mock;

    fn grid(total: u32, row_count: i64) -> impl FnMut(std::ops::Range<u32>) -> std::future::Ready<timechimp_api::Result<QueryResult<u32>>> {
        move |rows: std::ops::Range<u32>| {
            let row_data = (rows.start..rows.end.min(total)).collect();
            std::future::ready(Ok(QueryResult { row_data, row_count }))
        }
    }

    #[test]
    fn pages_until_all_rows_are_in() {
        let mut asked = Vec::new();
        let mut fetch = grid(1200, 1200);
        let rows = futures::executor::block_on(page_all(|range: std::ops::Range<u32>| {
            asked.push(range.clone());
            fetch(range)
        }))
        .unwrap();
        assert_eq!(rows, (0..1200).collect::<Vec<_>>());
        assert_eq!(asked, [0..500, 500..1000, 1000..1500]);
    }

    #[test]
    fn stops_at_an_empty_page_or_an_exact_fit() {
        // The server counting more rows than it hands out doesn't loop forever.
        assert_eq!(futures::executor::block_on(page_all(grid(10, 9999))).unwrap().len(), 10);
        let mut calls = 0;
        let mut fetch = grid(500, 500);
        let rows = futures::executor::block_on(page_all(|range| {
            calls += 1;
            fetch(range)
        }))
        .unwrap();
        assert_eq!((rows.len(), calls), (500, 1));
    }

    /// Thursday 12 November 2026; the mock's data runs up to then.
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap()
    }

    fn mock() -> (Arc<Mock>, Api) {
        let mock = Arc::new(Mock::new(Utc.with_ymd_and_hms(2026, 11, 12, 13, 7, 0).unwrap()));
        let api: Api = mock.clone();
        (mock, api)
    }

    fn block_on<T>(future: impl Future<Output = timechimp_api::Result<T>>) -> T {
        futures::executor::block_on(future).unwrap()
    }

    #[test]
    fn base_holds_active_tags_and_every_projects_tasks() {
        let (mock, api) = mock();
        let base = block_on(load_base(&api));
        let all_tags = block_on(mock.tags(TagType::Time));
        assert!(all_tags.iter().any(|t| !t.active), "the mock has an inactive tag");
        assert!(!base.tags.is_empty() && base.tags.iter().all(|t| t.active));
        assert_eq!(base.tags.len(), all_tags.iter().filter(|t| t.active).count());

        assert_eq!(base.project_tasks.len(), base.projects.len());
        for project in &base.projects {
            let tasks = &base.project_tasks[&project.id];
            assert!(!tasks.is_empty(), "{}", project.name);
            assert!(tasks.iter().all(|t| t.project_id == project.id));
        }
    }

    #[test]
    fn week_is_sorted_by_day_then_start_and_trips_by_day_then_id() {
        let (mock, api) = mock();
        let base = block_on(load_base(&api));
        let me = base.me.id;
        let monday = monday_of(today());
        // Added last, dated first: the mock hands both back at the end of the week.
        let project = &base.projects[1];
        let task = &base.project_tasks[&project.id][0];
        let entry = block_on(mock.create_time_entry(&NewTimeEntry::new(me, project, task, monday, 1.0)));
        let trip = block_on(mock.create_mileage(&NewMileage {
            user_id: me,
            customer_id: project.customer_id,
            project_id: project.id,
            date: monday,
            distance: 5.0,
            from_address: "A".into(),
            to_address: "B".into(),
            notes: None,
            billable: false,
            mileage_type: MileageType::Business,
            mileage_vehicle_id: None,
        }));
        let raw_trips = block_on(mock.mileage_for_week(me, monday));
        assert!(raw_trips.iter().any(|m| m.date > monday), "a later trip this week, so the new one has to move");

        let week = block_on(load_week(&api, me, monday));
        assert_eq!(week.start, monday);
        assert_eq!(week.entries[0].id, entry.id, "no start time sorts first on its day");
        assert!(week.entries.is_sorted_by_key(|e| (e.date, e.start)));
        assert!(week.travel.is_sorted_by_key(|m| (m.date, m.id)));
        assert_eq!(week.travel.len(), raw_trips.len());
        assert!(week.travel.iter().position(|m| m.id == trip.id) < Some(week.travel.len() - 1));
    }

    #[test]
    fn history_skips_the_week_already_loaded() {
        let (_, api) = mock();
        let me = block_on(load_base(&api)).me.id;
        let this_week = monday_of(today());
        let mondays: Vec<NaiveDate> = (0..HISTORY_WEEKS).map(|k| this_week - Duration::weeks(k)).collect();
        let ids = |entries: Vec<TimeEntry>| entries.into_iter().map(|e| e.id).collect::<Vec<_>>();
        let all = ids(block_on(load_weeks(&api, me, &mondays)));
        assert!(!all.is_empty());

        // Not in the range: all five weeks.
        assert_eq!(ids(block_on(load_history(&api, me, today(), this_week - Duration::weeks(10)))), all);

        let skipped = mondays[2];
        let history = block_on(load_history(&api, me, today(), skipped));
        let in_skipped = ids(block_on(load_weeks(&api, me, &[skipped])));
        assert!(!in_skipped.is_empty());
        assert!(history.iter().all(|e| !(skipped..skipped + Duration::days(7)).contains(&e.date)));
        assert_eq!(history.len() + in_skipped.len(), all.len());
        // Any day of the week counts as today.
        assert_eq!(block_on(load_history(&api, me, this_week, skipped)).len(), history.len());
    }

    #[test]
    fn report_keeps_only_the_users_trips() {
        let (mock, api) = mock();
        let me = block_on(load_base(&api)).me.id;
        let month = first_of_month(today());
        let to = last_of_month(month);
        let grid = block_on(mock.query_mileage(month, to, 0..10_000)).row_data;
        assert!(grid.iter().any(|r| r.user_id != me), "the grid holds a colleague's trips");
        let mine = grid.iter().filter(|r| r.user_id == me).count();
        assert!(mine > 0);

        let report = block_on(load_report(&api, me, month));
        assert_eq!(report.month, Some(month));
        assert_eq!(report.travel.len(), mine);
        assert!(report.travel.iter().all(|r| r.user_id == me));
        assert!(!report.time.is_empty());
        assert!(report.time.iter().all(|r| r.user_id == me && (month..=to).contains(&r.date)));
        assert!(report.time.is_sorted_by_key(|r| (r.date, r.start)));
    }
}
