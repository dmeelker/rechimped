//! Network loading: what each screen needs, fetched on the tokio runtime that reqwest needs.

use std::future::Future;
use std::sync::OnceLock;

use chrono::{Duration, NaiveDate};
use futures::{StreamExt, TryStreamExt};
use timechimp::{Client, MileageReportRow, ProjectTask, QueryResult, TagType, TimeEntry, TimeReportRow};

use crate::domain::report::Report;
use crate::domain::time::{last_of_month, monday_of};
use crate::session::{Base, Week};

/// The tokio runtime the HTTP client runs on; gpui has its own executor that reqwest can't use.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime"))
}

pub async fn load_base(client: &Client) -> timechimp::Result<Base> {
    let me = client.current_user().await?;
    let (projects, tasks, tags) =
        tokio::try_join!(client.projects_for_user(me.id), client.tasks_for_time(me.id), client.tags(TagType::Time))?;
    // One request per project, but not all at once.
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
        tasks,
        tags: tags.into_iter().filter(|t| t.active).collect(),
    })
}

pub async fn load_week(client: &Client, user_id: i64, week_start: NaiveDate) -> timechimp::Result<Week> {
    let (mut entries, mut mileage) =
        tokio::try_join!(client.time_entries_for_week(user_id, week_start), client.mileage_for_week(user_id, week_start))?;
    entries.sort_by_key(|e| (e.date, e.start));
    // Per day in the order they were registered (outbound before return), so the Mileage tab can
    // group consecutive rows by day and ↑/↓ follow what's on screen.
    mileage.sort_by_key(|m| (m.date, m.id));
    Ok(Week { start: week_start, entries, mileage })
}

/// How many weeks (this one included) the Recent picker in the time form looks back.
const HISTORY_WEEKS: i64 = 5;

/// How many requests of one batch (e.g. the tasks of every project) run at the same time.
const MAX_CONCURRENT_REQUESTS: usize = 6;

/// The user's entries of the last `HISTORY_WEEKS` weeks up to `today`, for the Recent picker,
/// except the week starting on `loaded`: that one is fetched with `load_week` already.
pub async fn load_history(
    client: &Client,
    user_id: i64,
    today: NaiveDate,
    loaded: NaiveDate,
) -> timechimp::Result<Vec<TimeEntry>> {
    let this_week = monday_of(today);
    let mondays: Vec<NaiveDate> =
        (0..HISTORY_WEEKS).map(|k| this_week - Duration::weeks(k)).filter(|monday| *monday != loaded).collect();
    load_weeks(client, user_id, &mondays).await
}

/// The user's entries of the weeks starting on `mondays`.
pub async fn load_weeks(client: &Client, user_id: i64, mondays: &[NaiveDate]) -> timechimp::Result<Vec<TimeEntry>> {
    let weeks = mondays.iter().map(|&monday| client.time_entries_for_week(user_id, monday));
    Ok(futures::future::try_join_all(weeks).await?.into_iter().flatten().collect())
}

/// Pages through a server-side grid query until all rows are in.
async fn page_all<T, Fut>(mut fetch: impl FnMut(std::ops::Range<u32>) -> Fut) -> timechimp::Result<Vec<T>>
where
    Fut: Future<Output = timechimp::Result<QueryResult<T>>>,
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

/// The month's report rows (own rows only).
pub async fn load_report(client: &Client, user_id: i64, month: NaiveDate) -> timechimp::Result<Report> {
    let to = last_of_month(month);
    let (mut time, mut mileage) = tokio::try_join!(
        page_all(|rows| client.query_time(month, to, rows)),
        page_all(|rows| client.query_mileage(month, to, rows)),
    )?;
    // Reports include everyone the caller can see.
    time.retain(|r: &TimeReportRow| r.user_id == user_id);
    mileage.retain(|r: &MileageReportRow| r.user_id == user_id);
    Ok(Report { month: Some(month), time, mileage })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid of `total` rows (their indexes) that claims `row_count` rows in all.
    fn grid(total: u32, row_count: i64) -> impl FnMut(std::ops::Range<u32>) -> std::future::Ready<timechimp::Result<QueryResult<u32>>> {
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
        // Exactly one page: no second request for an empty one.
        let mut calls = 0;
        let mut fetch = grid(500, 500);
        let rows = futures::executor::block_on(page_all(|range| {
            calls += 1;
            fetch(range)
        }))
        .unwrap();
        assert_eq!((rows.len(), calls), (500, 1));
    }
}
