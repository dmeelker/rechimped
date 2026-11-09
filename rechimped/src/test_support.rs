//! Dates are in November 2026; the 2nd is a Monday.

use std::collections::HashMap;

use chrono::NaiveDate;
use serde_json::json;
use timechimp_api::{CurrentUser, Mileage, Project, ProjectTask, Tag, TimeEntry, TimeReportRow};

use crate::session::Base;

pub fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
}

pub fn user() -> CurrentUser {
    serde_json::from_value(json!({
        "id": 1, "userName": "u", "displayName": "U", "companyId": 1, "companyName": "C",
        "accountTypeId": 1, "roleId": 1, "timezone": "Europe/Amsterdam",
        "customerRequired": false, "projectRequired": false, "taskRequired": false,
        "timeDescriptionRequired": false, "accessAllProjects": false, "isApprover": false,
        "isContractor": false, "isLocked": false, "userAvailability": null,
    }))
    .unwrap()
}

pub fn project(id: i64, name: &str) -> Project {
    serde_json::from_value(json!({
        "id": id, "name": name, "customerId": 2, "customerName": "Us",
        "intern": true, "unspecified": false, "active": true,
    }))
    .unwrap()
}

pub fn task(id: i64, project_id: i64, name: &str) -> ProjectTask {
    serde_json::from_value(json!({
        "id": id, "name": name, "billable": false, "active": true, "projectId": project_id, "taskId": id + 1,
    }))
    .unwrap()
}

pub fn tag(id: i64, name: &str) -> Tag {
    serde_json::from_value(json!({ "id": id, "companyId": 1, "name": name, "active": true, "type": 1 })).unwrap()
}

pub fn entry(id: i64, day: u32, start_end: &str) -> TimeEntry {
    serde_json::from_value(json!({
        "id": id, "timeRowId": id, "userId": 1, "userDisplayName": "U",
        "customerId": 2, "customerName": "C", "projectId": 3, "projectName": "P",
        "projectTaskId": 4, "taskId": 5, "taskName": "T",
        "date": format!("2026-11-{day:02}T00:00:00"), "hours": 1.0, "notes": null,
        "startEnd": start_end, "pause": 0.0, "billable": true,
        "status": 0, "statusIntern": 0, "statusExtern": 0,
        "modified": "2026-11-01T00:00:00", "timezone": null,
    }))
    .unwrap()
}

pub fn trip(id: i64, day: u32) -> Mileage {
    serde_json::from_value(json!({
        "id": id, "userId": 1, "userDisplayName": "U", "customerId": 2, "customerName": "C",
        "projectId": 3, "projectName": "P", "projectCode": null,
        "date": format!("2026-11-{day:02}T00:00:00"), "mileageVehicleId": null, "mileageVehicleName": null,
        "fromAddress": "A", "toAddress": "B", "notes": null, "distance": 10.0, "billable": true,
        "type": 1, "status": 0, "statusIntern": 0, "statusExtern": 0,
        "modified": "2026-11-01T00:00:00",
    }))
    .unwrap()
}

pub fn report_row(project: &str, task: &str, day: u32, hours: f64, billable: bool) -> TimeReportRow {
    serde_json::from_value(json!({
        "id": 1, "week": 45, "date": format!("2026-11-{day:02}T00:00:00"), "billable": billable,
        "customer": "C", "customerId": 1, "project": project, "projectId": 1, "projectCode": null,
        "task": task, "taskId": 1, "user": "U", "userId": 1, "team": null, "teamId": null,
        "time": hours, "notes": null, "startEnd": null, "pause": 0.0, "amount": null, "tags": null,
        "status": 0, "clientStatus": 0, "invoiceId": null,
    }))
    .unwrap()
}

pub fn base(projects: Vec<Project>, project_tasks: HashMap<i64, Vec<ProjectTask>>, tags: Vec<Tag>) -> Base {
    Base { me: user(), projects, project_tasks, tags }
}
