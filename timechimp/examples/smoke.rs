//! Exercises the client against the real API. Writes are self-cleaning: a test time entry
//! and mileage registration are created on the internal project and deleted again.
//!
//!     TIMECHIMP_REFRESH_TOKEN=... cargo run -p timechimp --example smoke [-- --write]

use chrono::{Datelike, Duration, Local, NaiveTime};
use timechimp::{Authenticator, Client, NewMileage, NewTimeEntry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("TIMECHIMP_REFRESH_TOKEN")?;
    let client = Client::new(Authenticator::from_refresh_token(token));
    let write = std::env::args().any(|a| a == "--write");

    let me = client.current_user().await?;
    println!("user: {} ({})", me.display_name, me.id);
    let today = Local::now().date_naive();
    let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let month_start = today.with_day(1).unwrap();

    println!("customers (active):   {}", client.customers().await?.len());
    println!("projects (active):    {}", client.projects().await?.len());
    println!("tasks (active):       {}", client.tasks().await?.len());
    println!("managed projects:     {}", client.managed_projects().await?.len());
    println!("is team manager:      {}", client.is_team_manager(me.id).await?);
    println!("saved views (time):   {}", client.saved_views("time").await?.len());
    println!("mileage customers:    {} mine, {} all", client.customers_for_mileage(me.id).await?.len(), client.all_mileage_customers().await?.len());
    println!("mileage vehicles:     {}", client.mileage_vehicles(me.id).await?.len());
    println!("mileage this week:    {}", client.mileage_for_week(me.id, monday).await?.len());

    let report = client.query_time(month_start, today, 0..1000).await?;
    let hours: f64 = report.row_data.iter().map(|r| r.time).sum();
    println!("time report month:    {} rows ({} total), {hours:.2}h", report.row_data.len(), report.row_count);
    let mileage = client.query_mileage(month_start, today, 0..1000).await?;
    println!("mileage report month: {} rows, {:.0} km", mileage.row_count, mileage.row_data.iter().map(|r| r.distance).sum::<f64>());

    let places = client.place_predictions("Arthur van Schendelstraat 500, Utrecht").await?;
    println!("places:               {:?}", places.iter().map(|p| &p.description).collect::<Vec<_>>());
    let home = client.place_predictions("Utrecht Centraal").await?;
    let route = client.route_distance(&home[0].place_id, &places[0].place_id).await?;
    println!("distance:             {} -> {}: {:.1} km, {} min", route.origin, route.destination, route.kilometres(), route.seconds / 60);

    if !write {
        println!("\n(pass --write to test creating/updating/deleting)");
        return Ok(());
    }

    // ---- Time entry: create, read, update, delete --------------------------------------
    let project = client.projects_for_user(me.id).await?.into_iter().find(|p| p.intern).ok_or("no internal project")?;
    let task = client.project_tasks(project.id).await?.into_iter().next().ok_or("project has no tasks")?;
    let at = |h, m| today.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap()).and_local_timezone(Local).unwrap().fixed_offset();

    let mut input = NewTimeEntry::new(me.id, &project, &task, today, 0.0)
        .with_times(at(22, 0), at(22, 15))
        .with_notes("timechimp-rs API test (will be deleted)");
    input.timezone = Some("Europe/Amsterdam".into());

    let created = client.create_time_entry(&input).await?;
    println!("\ncreated time entry {}: {} {:?} {}h '{}'", created.id, created.date, created.start_end, created.hours, created.notes.clone().unwrap_or_default());

    let fetched = client.time_entry(created.id).await?;
    assert_eq!(fetched.hours, 0.25);

    let updated = client
        .update_time_entry(created.id, &input.clone().with_times(at(22, 0), at(22, 30)).with_notes("timechimp-rs API test (updated)"))
        .await?;
    println!("updated time entry {}: {:?} {}h '{}'", updated.id, updated.start_end, updated.hours, updated.notes.clone().unwrap_or_default());
    let refetched = client.time_entry(created.id).await?;
    assert_eq!((refetched.hours, refetched.start_end.as_deref()), (0.5, Some("22:00-22:30")));

    client.delete_time_entry(created.id).await?;
    let gone = client.time_entries_for_week(me.id, monday).await?.iter().all(|e| e.id != created.id);
    println!("deleted time entry {}: gone from week = {gone}", created.id);

    // ---- Mileage: create, delete --------------------------------------------------------
    let mileage = client
        .create_mileage(&NewMileage {
            user_id: me.id,
            customer_id: project.customer_id,
            project_id: project.id,
            date: today,
            distance: 1.0,
            from_address: route.origin.clone(),
            to_address: route.destination.clone(),
            notes: Some("timechimp-rs API test (will be deleted)".into()),
            billable: false,
            mileage_type: 1,
            mileage_vehicle_id: None,
        })
        .await?;
    println!("created mileage {}: {} km {:?} -> {:?}", mileage.id, mileage.distance, mileage.from_address, mileage.to_address);
    client.delete_mileage(mileage.id).await?;
    let gone = client.mileage_for_week(me.id, monday).await?.iter().all(|m| m.id != mileage.id);
    println!("deleted mileage {}: gone from week = {gone}", mileage.id);

    Ok(())
}
