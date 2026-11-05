//! UI-independent pieces: data loading, date/time helpers and the session token on disk.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, Months, NaiveDate, NaiveTime};
use timechimp::{
    Client, CurrentUser, Mileage, MileageReportRow, Project, ProjectTask, Tag, Task, TimeEntry, TimeReportRow,
};

/// Mileage trip types as observed in the API (names are a best guess).
pub const MILEAGE_TYPES: [(i64, &str); 2] = [(1, "Business"), (3, "Home – work")];
/// Tag type for time entry tags.
const TIME_TAG_TYPE: i32 = 1;
pub const FALLBACK_TIMEZONE: &str = "Europe/Amsterdam";

/// The tokio runtime the HTTP client runs on; gpui has its own executor that reqwest can't use.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime"))
}

/// Everything that only changes on a full reload.
pub struct Base {
    pub me: CurrentUser,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub project_tasks: HashMap<i64, Vec<ProjectTask>>,
    pub tags: Vec<Tag>,
}

pub struct Week {
    pub entries: Vec<TimeEntry>,
    pub mileage: Vec<Mileage>,
}

#[derive(Default)]
pub struct Report {
    pub month: Option<NaiveDate>,
    pub time: Vec<TimeReportRow>,
    pub mileage: Vec<MileageReportRow>,
}

pub async fn load_base(client: &Client) -> timechimp::Result<Base> {
    let me = client.current_user().await?;
    let (projects, tasks, tags) =
        tokio::try_join!(client.projects_for_user(me.id), client.tasks_for_time(me.id), client.tags(TIME_TAG_TYPE))?;
    let project_tasks = futures::future::try_join_all(projects.iter().map(|p| client.project_tasks(p.id))).await?;
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
    mileage.sort_by_key(|m| (m.date, m.id));
    Ok(Week { entries, mileage })
}

/// How many weeks (this one included) the Recent picker in the time form looks back.
const HISTORY_WEEKS: i64 = 5;

/// The user's entries of the last `HISTORY_WEEKS` weeks up to `today`, for the Recent picker.
pub async fn load_history(client: &Client, user_id: i64, today: NaiveDate) -> timechimp::Result<Vec<TimeEntry>> {
    let this_week = monday_of(today);
    let weeks = (0..HISTORY_WEEKS).map(|k| client.time_entries_for_week(user_id, this_week - Duration::weeks(k)));
    Ok(futures::future::try_join_all(weeks).await?.into_iter().flatten().collect())
}

/// Something worked on recently: a distinct project / task / notes combination. `entry` is its
/// latest registration, which also supplies the tags.
#[derive(Clone)]
pub struct RecentItem {
    pub entry: TimeEntry,
    pub uses: usize,
}

impl RecentItem {
    pub fn notes(&self) -> &str {
        self.entry.notes.as_deref().unwrap_or("").trim()
    }

    /// Whether every word of `query` occurs (case-insensitively) in the notes, project, task,
    /// customer or tags.
    pub fn matches(&self, query: &str) -> bool {
        let e = &self.entry;
        let haystack =
            format!("{} {} {} {} {}", self.notes(), e.project_name, e.task_name, e.customer_name, e.tag_names.join(" "));
        matches_words(&haystack, query)
    }
}

fn matches_words(haystack: &str, query: &str) -> bool {
    let haystack = haystack.to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
}

/// Groups `entries` by project, task and notes; the most recently used first, then the most used.
pub fn recent_items<'a>(entries: impl IntoIterator<Item = &'a TimeEntry>) -> Vec<RecentItem> {
    let mut groups: HashMap<(i64, i64, String), RecentItem> = HashMap::new();
    for e in entries {
        let key = (e.project_id, e.project_task_id, e.notes.as_deref().unwrap_or("").trim().to_lowercase());
        let item = groups.entry(key).or_insert_with(|| RecentItem { entry: e.clone(), uses: 0 });
        item.uses += 1;
        if (e.date, e.modified) > (item.entry.date, item.entry.modified) {
            item.entry = e.clone();
        }
    }
    let mut items: Vec<RecentItem> = groups.into_values().collect();
    items.sort_by(|a, b| {
        (b.entry.date.date(), b.uses, b.entry.modified).cmp(&(a.entry.date.date(), a.uses, a.entry.modified))
    });
    items
}

/// Share of hours that should be billable, unless `billable_goal` in `config.toml` says otherwise.
pub const DEFAULT_BILLABLE_GOAL: f64 = 0.75;

/// Page through the server-side grid until all of the month's rows are in (own rows only).
async fn load_month_time(client: &Client, user_id: i64, month: NaiveDate) -> timechimp::Result<Vec<TimeReportRow>> {
    let to = last_of_month(month);
    let mut time = Vec::new();
    loop {
        let start = time.len() as u32;
        let page = client.query_time(month, to, start..start + PAGE).await?;
        let done = page.row_data.is_empty() || (start as i64 + page.row_data.len() as i64) >= page.row_count;
        time.extend(page.row_data);
        if done {
            break;
        }
    }
    // Reports include everyone the caller can see.
    time.retain(|r: &TimeReportRow| r.user_id == user_id);
    Ok(time)
}

const PAGE: u32 = 500;

fn last_of_month(month: NaiveDate) -> NaiveDate {
    (month + Months::new(1)).pred_opt().expect("valid date")
}

/// Billable hours among `entries`.
pub fn billable_hours(entries: &[TimeEntry]) -> f64 {
    entries.iter().filter(|e| e.billable).map(|e| e.hours).sum()
}

pub async fn load_report(client: &Client, user_id: i64, month: NaiveDate) -> timechimp::Result<Report> {
    let to = last_of_month(month);
    let time = load_month_time(client, user_id, month).await?;
    let mut mileage = Vec::new();
    loop {
        let start = mileage.len() as u32;
        let page = client.query_mileage(month, to, start..start + PAGE).await?;
        let done = page.row_data.is_empty() || (start as i64 + page.row_data.len() as i64) >= page.row_count;
        mileage.extend(page.row_data);
        if done {
            break;
        }
    }
    mileage.retain(|r: &MileageReportRow| r.user_id == user_id);
    Ok(Report { month: Some(month), time, mileage })
}

pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

pub fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("day 1 exists")
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Parses a time of day, leniently: `12` → 12:00, `930` → 9:30, `1230` → 12:30, and `12:30` or
/// `12.30` as written. Empty means "not given".
pub fn parse_time(s: &str) -> Result<Option<NaiveTime>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let invalid = || format!("Invalid time '{s}', use e.g. 9, 930, 12:30");
    let (hours, minutes) = match s.split_once([':', '.']) {
        Some((h, m)) => (h, m),
        None if !s.chars().all(|c| c.is_ascii_digit()) => return Err(invalid()),
        None if s.len() <= 2 => (s, "0"),
        None if s.len() <= 4 => s.split_at(s.len() - 2),
        None => return Err(invalid()),
    };
    let hours: u32 = hours.parse().map_err(|_| invalid())?;
    let minutes: u32 = match minutes {
        "0" => 0,
        m if m.len() == 2 => m.parse().map_err(|_| invalid())?,
        _ => return Err(invalid()),
    };
    NaiveTime::from_hms_opt(hours, minutes, 0).map(Some).ok_or_else(invalid)
}

/// A duration after `+` in the end field: `15` / `15m` minutes, `1h`, `1.5h`, `1h30` / `1h30m`.
fn parse_offset(s: &str) -> Option<i64> {
    let s = s.trim().to_ascii_lowercase();
    let minutes = |m: &str| -> Option<f64> {
        let m = m.trim().trim_end_matches('m').trim();
        if m.is_empty() { Some(0.0) } else { m.parse().ok() }
    };
    let total = match s.split_once('h') {
        Some((h, m)) => h.trim().replace(',', ".").parse::<f64>().ok()? * 60.0 + minutes(m)?,
        None if s.is_empty() => return None,
        None => minutes(&s)?,
    };
    (total.is_finite() && total >= 0.0).then(|| total.round() as i64)
}

/// Parses the end time; `+15`, `+1h`, `+1h30` mean that long after `start`, otherwise as `parse_time`.
pub fn parse_end(start: Option<NaiveTime>, s: &str) -> Result<Option<NaiveTime>, String> {
    let Some(offset) = s.trim().strip_prefix('+') else { return parse_time(s) };
    let minutes = parse_offset(offset).ok_or_else(|| format!("Invalid end '{}', use e.g. +15 or +1h30", s.trim()))?;
    let start = start.ok_or("Fill in the start to use +duration")?;
    let (end, wrapped) = start.overflowing_add_signed(chrono::Duration::minutes(minutes));
    if wrapped != 0 {
        return Err("End must be on the same day".into());
    }
    Ok(Some(end))
}

/// The `HH:MM` an end field of `+N` stands for, given the start field; None when it isn't one
/// (or can't be resolved yet).
pub fn expand_end(start: &str, end: &str) -> Option<String> {
    if !end.trim().starts_with('+') {
        return None;
    }
    let end = parse_end(parse_time(start).ok()?, end).ok()??;
    Some(end.format("%H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_end() {
        let at = |s: &str, e: &str| expand_end(s, e);
        assert_eq!(at("9", "+15"), Some("09:15".into()));
        assert_eq!(at("9:50", "+ 90"), Some("11:20".into()));
        assert_eq!(at("", "+15"), None);
        assert_eq!(at("9", "10"), None);
        assert_eq!(at("23:50", "+15"), None);
        assert_eq!(at("9", "+1h"), Some("10:00".into()));
        assert_eq!(at("9", "+1H30"), Some("10:30".into()));
        assert_eq!(at("9", "+1h30m"), Some("10:30".into()));
        assert_eq!(at("9", "+1.5h"), Some("10:30".into()));
        assert_eq!(at("9", "+1,25h"), Some("10:15".into()));
        assert_eq!(at("9", "+45m"), Some("09:45".into()));
        assert_eq!(at("9", "+"), None);
        assert_eq!(at("9", "+h"), None);
        assert!(parse_end(None, "+15").is_err());
        assert!(parse_end(None, "+x").is_err());
    }

    fn t(s: &str) -> Option<String> {
        parse_time(s).ok().flatten().map(|t| t.format("%H:%M").to_string())
    }

    #[test]
    fn junctions() {
        let r = |a: &str, b: &str| (parse_time(a).unwrap().unwrap(), parse_time(b).unwrap().unwrap());
        let tm = |s: &str| parse_time(s).unwrap().unwrap();
        assert_eq!(range_junction(r("830", "9"), r("9", "915")), None);
        assert_eq!(range_junction(r("830", "9"), r("930", "10")), Some(Junction::Gap { from: tm("9"), to: tm("930") }));
        assert_eq!(
            range_junction(r("830", "1000"), r("930", "1100")),
            Some(Junction::Overlap { from: tm("930"), to: tm("1000") })
        );
        // Fully contained: the overlap is the inner entry.
        assert_eq!(
            range_junction(r("8", "12"), r("9", "10")),
            Some(Junction::Overlap { from: tm("9"), to: tm("10") })
        );
    }

    #[test]
    fn config_file() {
        let config = parse_config(
            "# comment\nbillable_goal = 80\n[projects]\n\"Tournament Software\" = [\"wbso_tournament\"]\nIntern = []\n",
        )
        .unwrap();
        assert_eq!(config.billable_goal, 0.8);
        assert_eq!(config.projects["Tournament Software"], vec!["wbso_tournament".to_owned()]);
        assert!(config.projects["Intern"].is_empty());

        let empty = parse_config("").unwrap();
        assert!(empty.projects.is_empty());
        assert_eq!(empty.billable_goal, DEFAULT_BILLABLE_GOAL);
        assert!(parse_config("billable_goal = 150").is_err());
    }

    #[test]
    fn recent_filter() {
        let haystack = "Code reviews Tournament Software Software development Visual Reality wbso_tournament";
        assert!(matches_words(haystack, ""));
        assert!(matches_words(haystack, "code"));
        assert!(matches_words(haystack, "REV tourn"));
        assert!(matches_words(haystack, "  visual   wbso "));
        assert!(!matches_words(haystack, "code daily"));
    }

    #[test]
    fn lenient_times() {
        assert_eq!(t("12"), Some("12:00".into()));
        assert_eq!(t("9"), Some("09:00".into()));
        assert_eq!(t("930"), Some("09:30".into()));
        assert_eq!(t("1230"), Some("12:30".into()));
        assert_eq!(t("0815"), Some("08:15".into()));
        assert_eq!(t("12:30"), Some("12:30".into()));
        assert_eq!(t("12.30"), Some("12:30".into()));
        assert_eq!(t(" 8:05 "), Some("08:05".into()));
        assert_eq!(parse_time("").unwrap(), None);
        for bad in ["24", "1260", "12345", "12:3", "abc", "12:", "2500"] {
            assert!(parse_time(bad).is_err(), "{bad} should be rejected");
        }
    }
}

pub fn local(date: NaiveDate, time: NaiveTime) -> Result<DateTime<FixedOffset>, String> {
    date.and_time(time)
        .and_local_timezone(Local)
        .earliest()
        .map(|t| t.fixed_offset())
        .ok_or_else(|| format!("{date} {time} does not exist in the local timezone"))
}

/// The local start and end time of an entry, from its `HH:mm-HH:mm` range.
pub fn time_range(entry: &TimeEntry) -> Option<(NaiveTime, NaiveTime)> {
    let (start, end) = entry.start_end.as_deref()?.split_once('-')?;
    Some((parse_time(start).ok()??, parse_time(end).ok()??))
}

/// How two consecutive entries on a day fit together, when they don't line up exactly.
#[derive(Debug, PartialEq, Eq)]
pub enum Junction {
    /// Nothing registered between `from` and `to`.
    Gap { from: NaiveTime, to: NaiveTime },
    /// Both entries cover `from`–`to`.
    Overlap { from: NaiveTime, to: NaiveTime },
}

/// Compares the end of `prev` with the start of `next` (entries without times are ignored).
pub fn junction(prev: &TimeEntry, next: &TimeEntry) -> Option<Junction> {
    if prev.date.date() != next.date.date() {
        return None;
    }
    range_junction(time_range(prev)?, time_range(next)?)
}

/// `prev` must not start after `next` (entries are sorted by start time).
fn range_junction((_, prev_end): (NaiveTime, NaiveTime), (next_start, next_end): (NaiveTime, NaiveTime)) -> Option<Junction> {
    if prev_end == next_start {
        return None;
    }
    Some(if prev_end < next_start {
        Junction::Gap { from: prev_end, to: next_start }
    } else {
        Junction::Overlap { from: next_start, to: prev_end.min(next_end) }
    })
}

pub fn json_strings(value: &serde_json::Value) -> Option<Vec<String>> {
    Some(value.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
}

pub fn mileage_type_name(mileage_type: i32) -> String {
    MILEAGE_TYPES
        .iter()
        .find(|(id, _)| *id == mileage_type as i64)
        .map_or_else(|| format!("type {mileage_type}"), |(_, n)| n.to_string())
}

// ---- Config directory ---------------------------------------------------------------------
// Lives under `timechimp-tui` for historical reasons (the former terminal UI); keeping the
// path means existing sessions stay valid.

fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("timechimp-tui"))
}

// ---- Config file --------------------------------------------------------------------------
// `config.toml`: the billable goal, and which tags each project may use (shown as checkboxes in
// the time form). Earlier versions called it `projects.toml`; that file is renamed on first load.

pub fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("config.toml"))
}

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    /// Percentage, e.g. `75`.
    billable_goal: Option<f64>,
    /// Project name → tag names.
    #[serde(default)]
    projects: HashMap<String, Vec<String>>,
}

pub struct Config {
    /// Fraction, e.g. `0.75`.
    pub billable_goal: f64,
    /// Project name → tag names.
    pub projects: HashMap<String, Vec<String>>,
}

fn parse_config(text: &str) -> Result<Config, String> {
    let file: ConfigFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let billable_goal = match file.billable_goal {
        None => DEFAULT_BILLABLE_GOAL,
        Some(pct) if (0.0..=100.0).contains(&pct) => pct / 100.0,
        Some(pct) => return Err(format!("billable_goal must be a percentage between 0 and 100, not {pct}")),
    };
    Ok(Config { billable_goal, projects: file.projects })
}

/// Reads `config.toml`. `Ok(None)` if the file doesn't exist.
pub fn load_config() -> Result<Option<Config>, String> {
    let Some(path) = config_path() else { return Ok(None) };
    let legacy = path.with_file_name("projects.toml");
    if !path.exists() && legacy.exists() {
        // Move it over with the (then new) goal setting on top, where top-level keys must go.
        let old = std::fs::read_to_string(&legacy).map_err(|e| format!("Could not read {}: {e}", legacy.display()))?;
        let migrated = format!(
            "# Percentage of your hours that should be billable.\nbillable_goal = {}\n\n{old}",
            DEFAULT_BILLABLE_GOAL * 100.0
        );
        std::fs::write(&path, migrated).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        let _ = std::fs::remove_file(&legacy);
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
    };
    parse_config(&text).map(Some).map_err(|e| format!("Error in {}: {e}", path.display()))
}

/// Resolves the configured names against the user's projects and tags (case-insensitive).
/// Returns project id → tags, plus a warning for names that don't match anything.
pub fn resolve_project_tags(
    config: &HashMap<String, Vec<String>>,
    projects: &[Project],
    tags: &[Tag],
) -> (HashMap<i64, Vec<Tag>>, Option<String>) {
    let mut resolved = HashMap::new();
    let mut unknown = Vec::new();
    for (project_name, tag_names) in config {
        let Some(project) = projects.iter().find(|p| p.name.eq_ignore_ascii_case(project_name)) else {
            unknown.push(format!("project \"{project_name}\""));
            continue;
        };
        let mut project_tags = Vec::new();
        for name in tag_names {
            match tags.iter().find(|t| t.name.eq_ignore_ascii_case(name)) {
                Some(tag) => project_tags.push(tag.clone()),
                None => unknown.push(format!("tag \"{name}\"")),
            }
        }
        resolved.insert(project.id, project_tags);
    }
    let warning = (!unknown.is_empty()).then(|| format!("config.toml: unknown {}", unknown.join(", ")));
    (resolved, warning)
}

/// Writes a starter `config.toml` listing the user's projects and current (non-`z_`) tags.
pub fn write_config_template(projects: &[Project], tags: &[Tag]) -> std::io::Result<PathBuf> {
    let path = config_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    let mut text = format!(
        "# Reload the app (R) after editing.\n\n\
         # Percentage of your hours that should be billable.\n\
         billable_goal = {}\n\n\
         # Tags each project may use; the time entry form shows them as checkboxes.\n\
         # Names are matched case-insensitively.\n#\n# Available tags:\n",
        DEFAULT_BILLABLE_GOAL * 100.0
    );
    for tag in tags.iter().filter(|t| !t.name.starts_with("z_")) {
        text.push_str(&format!("#   {}\n", tag.name));
    }
    text.push_str("\n[projects]\n");
    for project in projects.iter().filter(|p| p.active) {
        text.push_str(&format!("# {:?} = []\n", project.name));
    }
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(&path, text)?;
    Ok(path)
}

// ---- Session token ------------------------------------------------------------------------

fn token_path() -> Option<PathBuf> {
    Some(config_dir()?.join("refresh_token"))
}

pub fn load_token() -> Option<String> {
    std::env::var("TIMECHIMP_REFRESH_TOKEN")
        .ok()
        .or_else(|| std::fs::read_to_string(token_path()?).ok())
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

pub fn save_token(token: &str) -> std::io::Result<()> {
    let path = token_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(path, token)
}

pub fn delete_token() {
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(path);
    }
}
