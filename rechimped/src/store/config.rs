use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use timechimp_api::{MileageType, Project, Tag};

use crate::domain::same_name;
use crate::domain::week::TaskNames;

pub const DEFAULT_BILLABLE_GOAL: f64 = 0.8;

pub const DAY_NAMES: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

fn day_key(i: usize) -> &'static str {
    &DAY_NAMES[i][..3]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub work_days: [bool; 7],
    pub billable_goal: f64,
    pub hidden_projects: Vec<String>,
    pub hidden_tasks: TaskNames,
    /// Per project, the tasks whose hours count neither as worked nor towards the contract (leave).
    pub excluded_tasks: TaskNames,
    /// At startup, remind to register today's trip while none is (off by default).
    pub travel_reminder: bool,
}

/// Stored as exclusions, so a task added in TimeChimp later counts towards the target by itself.
pub fn default_excluded_tasks() -> TaskNames {
    BTreeMap::from([("Intern".to_owned(), vec!["Verlof".to_owned(), "Ziek".to_owned()])])
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            work_days: [true, true, true, true, true, false, false],
            billable_goal: DEFAULT_BILLABLE_GOAL,
            hidden_projects: Vec::new(),
            hidden_tasks: BTreeMap::new(),
            excluded_tasks: default_excluded_tasks(),
            travel_reminder: false,
        }
    }
}

pub fn path() -> Result<PathBuf, String> {
    Ok(super::config_dir().ok_or("no config directory (APPDATA, XDG_CONFIG_HOME and HOME are unset)")?.join("config.toml"))
}

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    work_days: Option<Vec<String>>,
    billable_goal: Option<f64>,
    #[serde(default)]
    hidden_projects: Vec<String>,
    #[serde(default)]
    hidden_tasks: TaskNames,
    excluded_tasks: Option<TaskNames>,
    #[serde(default)]
    travel_reminder: bool,
    #[serde(default)]
    projects: HashMap<String, Vec<String>>,
    #[serde(default)]
    trips: Vec<TripPreset>,
    #[serde(default)]
    time_entries: Vec<TimePreset>,
}

/// Presets are appended to and removed from `config.toml` as text, so the rest of the file
/// (comments included) stays as it is.
pub trait Preset: serde::Serialize {
    const KEY: &'static str;
    const COMMENT: &'static str;
    fn is(&self, table: &toml_edit::Table) -> bool;
}

fn str_field<'a>(table: &'a toml_edit::Table, key: &str) -> Option<&'a str> {
    table.get(key).and_then(|v| v.as_str())
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct TripPreset {
    pub name: String,
    pub project: String,
    pub from: String,
    pub to: String,
    /// Optional because the form's Calculate can fill it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    #[serde(default)]
    pub return_trip: bool,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub travel_type: Option<String>,
    #[serde(default)]
    pub billable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Preset for TripPreset {
    const KEY: &'static str = "trips";
    const COMMENT: &'static str = TRIPS_COMMENT;
    fn is(&self, table: &toml_edit::Table) -> bool {
        str_field(table, "name").is_some_and(|n| same_name(n, &self.name))
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct TimePreset {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Preset for TimePreset {
    const KEY: &'static str = "time_entries";
    const COMMENT: &'static str = TIME_ENTRIES_COMMENT;
    /// Time presets have no name; project, task and notes tell them apart.
    fn is(&self, table: &toml_edit::Table) -> bool {
        str_field(table, "project") == Some(&self.project)
            && str_field(table, "task") == self.task.as_deref()
            && str_field(table, "notes") == self.notes.as_deref()
    }
}

impl TripPreset {
    pub fn trip_type(&self) -> Option<MileageType> {
        let Some(name) = &self.travel_type else { return Some(MileageType::Business) };
        let key = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        TRIP_TYPE_KEYS.iter().find(|(_, k)| key(k) == key(name)).map(|(t, _)| *t)
    }
}

/// Part of the file format, so kept apart from the labels the forms show (`domain::TRAVEL_TYPES`):
/// rewording those mustn't break existing files.
const TRIP_TYPE_KEYS: [(MileageType, &str); 2] = [(MileageType::Business, "business"), (MileageType::HomeWork, "home-work")];

pub fn travel_type_key(travel_type: MileageType) -> Option<String> {
    TRIP_TYPE_KEYS.iter().find(|(t, _)| *t == travel_type).map(|(_, key)| (*key).to_owned())
}

pub fn preset_name(from: &str, to: &str, notes: &str) -> String {
    if !notes.trim().is_empty() {
        return notes.trim().to_owned();
    }
    let short = |a: &str| a.split(',').next().unwrap_or(a).trim().to_owned();
    format!("{} → {}", short(from), short(to))
}

pub struct Config {
    pub settings: Settings,
    pub projects: HashMap<String, Vec<String>>,
    pub trips: Vec<TripPreset>,
    pub time_entries: Vec<TimePreset>,
}

fn parse_config(text: &str) -> Result<Config, String> {
    let file: ConfigFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let defaults = Settings::default();
    let billable_goal = match file.billable_goal {
        None => defaults.billable_goal,
        Some(pct) if (0.0..=100.0).contains(&pct) => pct / 100.0,
        Some(pct) => return Err(format!("billable_goal must be a percentage between 0 and 100, not {pct}")),
    };
    let work_days = match &file.work_days {
        None => defaults.work_days,
        Some(names) => {
            let mut days = [false; 7];
            for name in names {
                let index = day_index(name).ok_or_else(|| {
                    format!("work_days: unknown day \"{name}\" (use {})", (0..7).map(day_key).collect::<Vec<_>>().join(", "))
                })?;
                days[index] = true;
            }
            days
        }
    };
    let settings = Settings {
        work_days,
        billable_goal,
        hidden_projects: file.hidden_projects,
        hidden_tasks: file.hidden_tasks,
        excluded_tasks: file.excluded_tasks.unwrap_or_else(default_excluded_tasks),
        travel_reminder: file.travel_reminder,
    };
    for trip in &file.trips {
        if trip.trip_type().is_none() {
            let known: Vec<&str> = TRIP_TYPE_KEYS.iter().map(|(_, key)| *key).collect();
            return Err(format!(
                "trip \"{}\": unknown type \"{}\" (use {})",
                trip.name,
                trip.travel_type.as_deref().unwrap_or_default(),
                known.join(" or ")
            ));
        }
    }
    Ok(Config { settings, projects: file.projects, trips: file.trips, time_entries: file.time_entries })
}

fn day_index(name: &str) -> Option<usize> {
    let name = name.trim().to_lowercase();
    (name.len() >= 3).then(|| DAY_NAMES.iter().position(|full| full.starts_with(&name))).flatten()
}

fn replace_file(path: &Path, text: &str) -> Result<(), String> {
    super::replace_file(path, text).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

fn number(value: f64) -> toml_edit::Item {
    if value.fract() == 0.0 && value.abs() < 1e15 { toml_edit::value(value as i64) } else { toml_edit::value(value) }
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
    };
    let text = with_settings(&text, settings).map_err(|e| format!("Error in {}: {e}", path.display()))?;
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    replace_file(path, &text)
}

fn with_settings(text: &str, settings: &Settings) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let days: toml_edit::Array = (0..7).zip(settings.work_days).filter(|(_, on)| *on).map(|(i, _)| day_key(i)).collect();
    // Set in place, so a key keeps its position and the comment above it.
    let set = |doc: &mut toml_edit::DocumentMut, key: &str, item: toml_edit::Item, comment: &str| match doc
        .get_mut(key)
        .and_then(|i| i.as_value_mut())
    {
        Some(value) => {
            let decor = value.decor().clone();
            *value = item.into_value().expect("a value");
            *value.decor_mut() = decor;
        }
        None => {
            doc.insert(key, item);
            if let Some(mut k) = doc.key_mut(key) {
                k.leaf_decor_mut().set_prefix(comment);
            }
        }
    };
    set(&mut doc, "work_days", toml_edit::value(days), "# The days you work (mon, tue, wed, thu, fri, sat, sun).\n");
    // Rounded, so 0.8 * 100 doesn't come out as 80.00000000000001.
    let pct = (settings.billable_goal * 10000.0).round() / 100.0;
    set(&mut doc, "billable_goal", number(pct), "# Percentage of your hours that should be billable.\n");
    let comment = "# At startup, remind you to register today's trip while none is (true or false).\n";
    set(&mut doc, "travel_reminder", toml_edit::value(settings.travel_reminder), comment);
    // Only written once something is hidden, so a new file doesn't start with it.
    if !settings.hidden_projects.is_empty() || doc.contains_key("hidden_projects") {
        let hidden: toml_edit::Array = settings.hidden_projects.iter().map(String::as_str).collect();
        let comment = "# Projects the forms don't offer (Settings, in the menu under your name).\n";
        set(&mut doc, "hidden_projects", toml_edit::value(hidden), comment);
    }
    if !settings.hidden_tasks.is_empty() || doc.contains_key("hidden_tasks") {
        let comment = "# Per project, the tasks the time form doesn't offer (Settings, in the menu under your name).";
        set_task_table(&mut doc, "hidden_tasks", &settings.hidden_tasks, comment)?;
    }
    // Once written, even empty, so clearing the default doesn't bring it back.
    if settings.excluded_tasks != default_excluded_tasks() || doc.contains_key("excluded_tasks") {
        let comment = "# Per project, the tasks that don't count towards the billable target, like leave: their hours are\n\
                       # left out of both your hours and your contract hours (Settings, in the menu under your name).";
        set_task_table(&mut doc, "excluded_tasks", &settings.excluded_tasks, comment)?;
    }
    let text = doc.to_string();
    parse_config(&text)?;
    Ok(text)
}

/// Keys are changed in place and emptied ones dropped.
fn set_task_table(doc: &mut toml_edit::DocumentMut, key: &str, names: &TaskNames, comment: &str) -> Result<(), String> {
    let table = doc
        .entry(key)
        .or_insert_with(|| {
            let mut table = toml_edit::Table::new();
            table.decor_mut().set_prefix(format!(
                "
{comment}
"
            ));
            toml_edit::Item::Table(table)
        })
        .as_table_like_mut()
        .ok_or_else(|| format!("{key} is not a table"))?;
    let stale: Vec<String> = table
        .iter()
        .map(|(key, _)| key.to_owned())
        .filter(|key| !names.iter().any(|(p, tasks)| same_name(p, key) && !tasks.is_empty()))
        .collect();
    for key in stale {
        table.remove(&key);
    }
    for (project, tasks) in names {
        set_names(table, project, tasks);
    }
    Ok(())
}

pub fn save_project_tags(path: &Path, changes: &[(String, Vec<String>)]) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
    };
    let text = with_project_tags(&text, changes).map_err(|e| format!("Error in {}: {e}", path.display()))?;
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    replace_file(path, &text)
}

fn with_project_tags(text: &str, changes: &[(String, Vec<String>)]) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let projects = doc.entry("projects").or_insert_with(toml_edit::table).as_table_mut().ok_or("[projects] is not a table")?;
    for (project, tags) in changes {
        set_names(projects, project, tags);
    }
    let text = doc.to_string();
    parse_config(&text)?;
    Ok(text)
}

/// An absent key is only added when there are any.
fn set_names(table: &mut dyn toml_edit::TableLike, key: &str, names: &[String]) {
    let names: toml_edit::Array = names.iter().map(String::as_str).collect();
    let existing = table.iter().map(|(k, _)| k.to_owned()).find(|k| same_name(k, key));
    match existing.and_then(|k| table.get_mut(&k)).and_then(|item| item.as_value_mut()) {
        // In place, so the key keeps its position and the comment above it.
        Some(value) => {
            let decor = value.decor().clone();
            *value = toml_edit::Value::Array(names);
            *value.decor_mut() = decor;
        }
        None if !names.is_empty() => {
            table.insert(key, toml_edit::value(names));
        }
        None => {}
    }
}

fn preset_toml<P: Preset>(preset: &P) -> Result<String, String> {
    toml::to_string(&std::collections::BTreeMap::from([(P::KEY, [preset])])).map_err(|e| e.to_string())
}

pub fn append_preset<P: Preset>(path: &Path, preset: &P) -> Result<(), String> {
    let block = preset_toml(preset)?;
    let mut text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
    };
    let had_any = text.parse::<toml_edit::DocumentMut>().is_ok_and(|doc| doc.contains_key(P::KEY));
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !had_any {
        text.push_str(&format!("\n{}", P::COMMENT));
    }
    text.push('\n');
    text.push_str(&block);
    parse_config(&text).map_err(|e| format!("Error in {}: {e}", path.display()))?;
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    replace_file(path, &text)
}

pub fn remove_preset<P: Preset>(path: &Path, preset: &P) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let text = without(&text, preset).map_err(|e| format!("{}: {e}", path.display()))?;
    replace_file(path, &text)
}

fn without<P: Preset>(text: &str, preset: &P) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let not_found = || "preset not found (was it changed in the meantime?)".to_owned();
    let presets = doc.get_mut(P::KEY).and_then(toml_edit::Item::as_array_of_tables_mut).ok_or_else(not_found)?;
    let index = presets.iter().position(|t| preset.is(t)).ok_or_else(not_found)?;
    // The comment above the first preset explains the section; hand it to the next one.
    let prefix = presets.get(index).and_then(|t| t.decor().prefix()).and_then(|p| p.as_str()).map(str::to_owned);
    presets.remove(index);
    if let (Some(prefix), Some(next)) = (prefix, presets.get_mut(index))
        && index == 0
    {
        let own = next.decor().prefix().and_then(|p| p.as_str()).unwrap_or("").to_owned();
        next.decor_mut().set_prefix(format!("{prefix}{own}"));
    }
    if presets.is_empty() {
        doc.remove(P::KEY);
    }
    Ok(doc.to_string())
}

/// Each preset is found by `Preset::is`, so its block keeps its own comments.
pub fn save_presets(path: &Path, trips: &[TripPreset], times: &[TimePreset]) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let text = with_presets(&text, trips, times).map_err(|e| format!("{}: {e}", path.display()))?;
    replace_file(path, &text)
}

fn with_presets(text: &str, trips: &[TripPreset], times: &[TimePreset]) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    rearrange(&mut doc, trips)?;
    rearrange(&mut doc, times)?;
    let text = doc.to_string();
    parse_config(&text)?;
    Ok(text)
}

fn rearrange<P: Preset>(doc: &mut toml_edit::DocumentMut, kept: &[P]) -> Result<(), String> {
    let not_found = || "preset not found (was it changed in the meantime?)".to_owned();
    let Some(item) = doc.get_mut(P::KEY) else {
        return if kept.is_empty() { Ok(()) } else { Err(not_found()) };
    };
    let old = item.as_array_of_tables().ok_or_else(not_found)?;
    let section = old.get(0).and_then(|t| t.decor().prefix()).and_then(|p| p.as_str()).unwrap_or("").to_owned();
    let mut positions: Vec<usize> = old.iter().filter_map(toml_edit::Table::position).collect();
    positions.sort_unstable();
    // Each table is used once, so identical presets stay apart.
    let mut slots: Vec<Option<(usize, toml_edit::Table)>> = old.iter().cloned().enumerate().map(Some).collect();
    let mut new = toml_edit::ArrayOfTables::new();
    for (i, preset) in kept.iter().enumerate() {
        let slot = slots.iter_mut().find(|s| s.as_ref().is_some_and(|(_, t)| preset.is(t))).ok_or_else(not_found)?;
        let (was, mut table) = slot.take().expect("found a filled slot");
        // The section comment goes to the new first preset; the old first one gets a blank line.
        if was == 0 && i > 0 {
            table.decor_mut().set_prefix("\n");
        } else if i == 0 && was > 0 {
            let own = table.decor().prefix().and_then(|p| p.as_str()).unwrap_or("").trim_start_matches('\n').to_owned();
            table.decor_mut().set_prefix(format!("{section}{own}"));
        }
        // Tables print in position order, so the new order takes over the old positions.
        if let Some(&position) = positions.get(i) {
            table.set_position(position);
        }
        new.push(table);
    }
    if new.is_empty() {
        doc.remove(P::KEY);
    } else {
        *item = toml_edit::Item::ArrayOfTables(new);
    }
    Ok(())
}

const TRIPS_COMMENT: &str = "# Preset trips, in the dropdown of \"+ Add trip\" on the Travel tab. Fields: name,\n\
     # project, from, to, distance (km, one way; optional), return_trip, type (\"business\" or\n\
     # \"home-work\"), billable, notes (optional).\n";

const TIME_ENTRIES_COMMENT: &str = "# Preset time entries, in the dropdown of \"+ Add time\" on the Week tab. Fields: project,\n\
     # task (optional; the project's first task), notes (optional), tags (optional list of names).\n";

pub fn load(path: &Path) -> Result<Option<Config>, String> {
    let legacy = path.with_file_name("projects.toml");
    if !path.exists() && legacy.exists() {
        // Earlier versions used projects.toml. The settings go on top, where top-level keys must be.
        let old = std::fs::read_to_string(&legacy).map_err(|e| format!("Could not read {}: {e}", legacy.display()))?;
        let migrated = format!("{}\n{old}", settings_toml());
        std::fs::write(path, migrated).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        let _ = std::fs::remove_file(&legacy);
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
    };
    parse_config(&text).map(Some).map_err(|e| format!("Error in {}: {e}", path.display()))
}

pub fn resolve_project_tags(
    config: &HashMap<String, Vec<String>>,
    projects: &[Project],
    tags: &[Tag],
) -> (HashMap<i64, Vec<Tag>>, Option<String>) {
    let mut resolved = HashMap::new();
    let mut unknown = Vec::new();
    for (project_name, tag_names) in config {
        let Some(project) = projects.iter().find(|p| same_name(&p.name, project_name)) else {
            unknown.push(format!("project \"{project_name}\""));
            continue;
        };
        let mut project_tags = Vec::new();
        for name in tag_names {
            match tags.iter().find(|t| same_name(&t.name, name)) {
                Some(tag) => project_tags.push(tag.clone()),
                None => unknown.push(format!("tag \"{name}\"")),
            }
        }
        resolved.insert(project.id, project_tags);
    }
    let warning = (!unknown.is_empty()).then(|| format!("config.toml: unknown {}", unknown.join(", ")));
    (resolved, warning)
}

fn settings_toml() -> String {
    with_settings("", &Settings::default()).expect("the defaults are valid")
}

pub fn write_template(path: &Path, projects: &[Project], tags: &[Tag]) -> std::io::Result<()> {
    let mut text = format!(
        "# Reload the app (R) after editing. In the menu under your name, Settings edits the\n\
         # top-level keys and Project tags the [projects] table.\n\n{}\n\
         # Tags each project may use; the time entry form shows them as checkboxes.\n\
         # While no project has any, every project offers all tags (but the z_ ones).\n\
         # Names are matched case-insensitively.\n#\n# Available tags:\n",
        settings_toml()
    );
    for tag in tags.iter().filter(|t| !t.name.starts_with("z_")) {
        text.push_str(&format!("#   {}\n", tag.name));
    }
    text.push_str("\n[projects]\n");
    for project in projects.iter().filter(|p| p.active) {
        text.push_str(&format!("# {:?} = []\n", project.name));
    }
    text.push_str(&format!(
        "\n{TRIPS_COMMENT}# \"Save as preset\" in the trip form adds one here. Example:\n#\n\
         # [[trips]]\n# name = \"Commute\"\n# project = \"Intern\"\n# from = \"Home street 1, Town\"\n\
         # to = \"Office lane 2, City\"\n# distance = 23\n# return_trip = true\n# type = \"home-work\"\n"
    ));
    text.push_str(&format!(
        "\n{TIME_ENTRIES_COMMENT}# \"Save as preset\" in the time form adds one here. Example:\n#\n\
         # [[time_entries]]\n# project = \"Intern\"\n# task = \"Meeting\"\n# notes = \"Standup\"\n"
    ));
    std::fs::create_dir_all(path.parent().expect("has parent"))?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("rechimped-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn file_round_trip() {
        let dir = TempDir::new("config");
        let path = dir.0.join("config.toml");
        assert!(load(&path).unwrap().is_none(), "no file yet");

        std::fs::write(dir.0.join("projects.toml"), "[projects]\nIntern = [\"x\"]\n").unwrap();
        let config = load(&path).unwrap().unwrap();
        assert!(!dir.0.join("projects.toml").exists());
        assert_eq!(config.settings, Settings::default());
        assert_eq!(config.projects["Intern"], ["x"]);

        let preset = TripPreset {
            name: "Commute".into(),
            project: "Intern".into(),
            from: "A".into(),
            to: "B".into(),
            distance: Some(12.0),
            return_trip: true,
            travel_type: travel_type_key(MileageType::HomeWork),
            billable: false,
            notes: None,
        };
        append_preset(&path, &preset).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# Percentage of your hours") && text.contains("# Preset trips"), "comments stay");
        assert_eq!(load(&path).unwrap().unwrap().trips, std::slice::from_ref(&preset));

        let standup = TimePreset {
            project: "Intern".into(),
            task: Some("Meeting".into()),
            notes: Some("Standup".into()),
            tags: vec!["daily".into()],
        };
        let bare = TimePreset { notes: None, tags: Vec::new(), ..standup.clone() };
        append_preset(&path, &standup).unwrap();
        append_preset(&path, &bare).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("# Preset time entries").count(), 1, "section comment once");
        let config = load(&path).unwrap().unwrap();
        assert_eq!((config.trips.len(), &config.time_entries[..]), (1, &[standup.clone(), bare.clone()][..]));

        remove_preset(&path, &TripPreset { name: "commute".into(), ..preset.clone() }).unwrap();
        remove_preset(&path, &standup).unwrap();
        let config = load(&path).unwrap().unwrap();
        assert!(config.trips.is_empty());
        assert_eq!(config.time_entries, [bare]);
        assert!(remove_preset(&path, &preset).is_err());
        assert!(remove_preset(&path, &standup).is_err());

        std::fs::write(&path, "billable_goal = [").unwrap();
        assert!(load(&path).is_err());
        assert!(append_preset(&path, &preset_named("X")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "billable_goal = [");
    }

    fn preset_named(name: &str) -> TripPreset {
        TripPreset {
            name: name.into(),
            project: "p".into(),
            from: "a".into(),
            to: "b".into(),
            distance: None,
            return_trip: false,
            travel_type: None,
            billable: false,
            notes: None,
        }
    }

    #[test]
    fn template() {
        let dir = TempDir::new("template");
        let path = dir.0.join("sub").join("config.toml");
        write_template(&path, &[], &[]).unwrap();
        let config = load(&path).unwrap().unwrap();
        assert_eq!(config.settings, Settings::default());
        assert!(config.projects.is_empty() && config.trips.is_empty() && config.time_entries.is_empty());
    }

    #[test]
    fn config_file() {
        let config = parse_config(
            "# comment\nbillable_goal = 80\n[projects]\n\"Tournament Software\" = [\"wbso_tournament\"]\nIntern = []\n",
        )
        .unwrap();
        assert_eq!(config.settings.billable_goal, 0.8);
        assert_eq!(config.projects["Tournament Software"], vec!["wbso_tournament".to_owned()]);
        assert!(config.projects["Intern"].is_empty());

        let empty = parse_config("").unwrap();
        assert!(empty.projects.is_empty());
        assert_eq!(empty.settings, Settings::default());
        assert!(parse_config("billable_goal = 150").is_err());
    }

    #[test]
    fn project_tags_are_written_in_place() {
        let text = "billable_goal = 80\n\n[projects]\n# keep me\nintern = [\"old\"]\nOther = [\"x\"]\n\n[[trips]]\nname = \"T\"\nproject = \"P\"\nfrom = \"A\"\nto = \"B\"\n";
        let changes = [
            ("Intern".to_owned(), vec!["a".to_owned(), "b".to_owned()]),
            ("New".to_owned(), vec!["c".to_owned()]),
            ("Untagged".to_owned(), Vec::new()),
        ];
        let saved = with_project_tags(text, &changes).unwrap();
        assert!(
            saved.contains("# keep me\nintern = [\"a\", \"b\"]\n"),
            "existing key changed in place, its spelling kept:\n{saved}"
        );
        assert!(saved.contains("Other = [\"x\"]"), "projects not changed stay");
        let config = parse_config(&saved).unwrap();
        assert_eq!(config.projects["New"], ["c"]);
        assert!(!config.projects.contains_key("Untagged"), "no key added for a project without tags");
        assert_eq!(config.trips.len(), 1);

        let cleared = with_project_tags(&saved, &[("INTERN".to_owned(), Vec::new())]).unwrap();
        assert!(parse_config(&cleared).unwrap().projects["intern"].is_empty());

        let fresh = with_project_tags("", &[("P".to_owned(), vec!["t".to_owned()])]).unwrap();
        assert_eq!(parse_config(&fresh).unwrap().projects["P"], ["t"]);
    }

    #[test]
    fn settings() {
        let config = parse_config("work_days = [\"Mon\", \"tues\", \"thursday\"]\nbillable_goal = 70\n").unwrap();
        let s = config.settings;
        assert_eq!(s.work_days, [true, true, false, true, false, false, false]);
        assert_eq!(s.billable_goal, 0.7);
        assert_eq!(parse_config("work_days = []").unwrap().settings.work_days, [false; 7]);
        assert!(parse_config("work_days = [\"mo\"]").is_err(), "too short to tell");
        assert!(parse_config("work_days = [\"funday\"]").is_err());
    }

    #[test]
    fn save_settings_keeps_the_rest() {
        let settings = Settings {
            work_days: [false, true, true, false, true, true, false],
            billable_goal: 0.7,
            hidden_projects: vec!["Old \"one\"".into()],
            hidden_tasks: BTreeMap::from([("Intern".into(), vec!["Verlof".into()])]),
            excluded_tasks: BTreeMap::from([("Intern".into(), vec!["Verlof".into()])]),
            travel_reminder: true,
        };
        let text = "# top\n# goal\nbillable_goal = 75 # inline\n\n[projects]\n# keep me\nIntern = []\n";
        let saved = with_settings(text, &settings).unwrap();
        assert_eq!(parse_config(&saved).unwrap().settings, settings);
        assert!(saved.contains("# goal\nbillable_goal = 70 # inline\n"), "whole numbers without .0: {saved}");
        assert!(saved.contains("# keep me\nIntern = []") && saved.contains("# The days you work"), "{saved}");
        assert!(saved.contains("work_days = [\"tue\", \"wed\", \"fri\", \"sat\"]"), "{saved}");
        assert!(saved.contains("\ntravel_reminder = true\n"), "{saved}");
        assert!(saved.find("work_days").unwrap() < saved.find("[projects]").unwrap(), "top-level keys before the tables");
        assert!(saved.find("hidden_projects").unwrap() < saved.find("[projects]").unwrap(), "{saved}");
        let none_hidden = Settings { hidden_projects: Vec::new(), ..settings.clone() };
        assert!(with_settings(&saved, &none_hidden).unwrap().contains("\nhidden_projects = []"), "an existing key stays");
        assert!(!with_settings("", &none_hidden).unwrap().contains("hidden_projects"), "no key while nothing is hidden");
        assert_eq!(with_settings(&saved, &settings).unwrap(), saved);
        let fraction = Settings { billable_goal: 0.825, ..settings.clone() };
        assert_eq!(parse_config(&with_settings(&saved, &fraction).unwrap()).unwrap().settings, fraction);
        assert!(with_settings("billable_goal = [", &settings).is_err());

        let dir = TempDir::new("settings");
        let path = dir.0.join("config.toml");
        save_settings(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap().unwrap().settings, settings);
        save_settings(&path, &Settings { billable_goal: 0.5, ..settings.clone() }).unwrap();
        assert_eq!(load(&path).unwrap().unwrap().settings.billable_goal, 0.5, "replaces an existing file");
        assert!(!path.with_extension("toml.tmp").exists(), "no temporary file left behind");
    }

    #[test]
    fn save_hidden_tasks_in_place() {
        let hidden = |pairs: &[(&str, &[&str])]| Settings {
            hidden_tasks: pairs.iter().map(|(p, t)| ((*p).to_owned(), t.iter().map(|t| (*t).to_owned()).collect())).collect(),
            ..Settings::default()
        };
        let text = "[hidden_tasks]\n# mine\nintern = [\"Verlof\"]\nGone = [\"x\"]\n\n[projects]\nIntern = []\n";
        let saved = with_settings(text, &hidden(&[("Intern", &["Verlof", "Ziek"]), ("New", &["T"])])).unwrap();
        assert!(saved.contains("# mine\nintern = [\"Verlof\", \"Ziek\"]"), "in place, case-insensitively: {saved}");
        assert!(saved.contains("New = [\"T\"]") && !saved.contains("Gone"), "added and dropped: {saved}");
        assert!(saved.contains("[projects]\nIntern = []"), "{saved}");

        let none = with_settings(&saved, &hidden(&[("Intern", &[])])).unwrap();
        assert!(none.contains("[hidden_tasks]") && !none.contains("intern ="), "empty lists go: {none}");
        assert!(!with_settings("", &Settings::default()).unwrap().contains("hidden_tasks"), "no table while nothing is hidden");
        let fresh = with_settings("billable_goal = 80\n", &hidden(&[("P", &["T"])])).unwrap();
        assert!(fresh.contains("# Per project, the tasks") && fresh.contains("[hidden_tasks]\nP = [\"T\"]"), "{fresh}");
        assert!(with_settings("hidden_tasks = { P = [\"A\"] }", &hidden(&[("P", &["B"])])).unwrap().contains("P = [\"B\"]"));
    }

    #[test]
    fn excluded_tasks() {
        assert_eq!(parse_config("").unwrap().settings.excluded_tasks, default_excluded_tasks(), "leave by default");
        assert!(
            parse_config(
                "[excluded_tasks]
"
            )
            .unwrap()
            .settings
            .excluded_tasks
            .is_empty(),
            "an empty table excludes nothing"
        );
        assert!(!with_settings("", &Settings::default()).unwrap().contains("excluded_tasks"), "not written while the default");
        let none = Settings { excluded_tasks: BTreeMap::new(), ..Settings::default() };
        let saved = with_settings(
            "billable_goal = 80
",
            &none,
        )
        .unwrap();
        assert!(
            saved.contains(
                "# Per project, the tasks that don't count towards the billable target, like leave: their hours are\n# left out"
            ),
            "{saved}"
        );
        assert!(saved.contains("[excluded_tasks]"), "{saved}");
        assert_eq!(parse_config(&saved).unwrap().settings, none, "cleared stays cleared");
        assert_eq!(parse_config(&with_settings(&saved, &Settings::default()).unwrap()).unwrap().settings, Settings::default());
    }

    #[test]
    fn trip_presets() {
        let config = parse_config(
            "[projects]\nIntern = []\n\n[[trips]]\nname = \"Commute\"\nproject = \"Intern\"\nfrom = \"A\"\nto = \"B\"\n\
             distance = 23.5\nreturn_trip = true\ntype = \"Home-Work\"\n\n\
             [[trips]]\nname = \"Client\"\nproject = \"X\"\nfrom = \"A\"\nto = \"C\"\n",
        )
        .unwrap();
        let [commute, client] = &config.trips[..] else { panic!("two trips") };
        assert_eq!(commute.distance, Some(23.5));
        assert!(commute.return_trip && !commute.billable);
        assert_eq!(commute.trip_type(), Some(MileageType::HomeWork));
        assert_eq!(client.trip_type(), Some(MileageType::Business));
        assert_eq!(client.distance, None);
        assert!(parse_config("[[trips]]\nname = \"x\"\nproject = \"p\"\nfrom = \"a\"\nto = \"b\"\ntype = \"bike\"\n").is_err());

        assert_eq!(travel_type_key(MileageType::HomeWork).as_deref(), Some("home-work"));
        assert!(crate::domain::TRAVEL_TYPES.iter().all(|(t, _)| travel_type_key(*t).is_some()));
        let block = preset_toml(commute).unwrap();
        let text = format!("billable_goal = 70\n[projects]\nIntern = []\n\n{block}");
        assert_eq!(parse_config(&text).unwrap().trips, vec![TripPreset { name: "Commute".into(), ..commute.clone() }]);

        assert_eq!(preset_name("Kerkstraat 1, Utrecht", "Stationsplein 5, Amersfoort", ""), "Kerkstraat 1 → Stationsplein 5");
        assert_eq!(preset_name("a", "b", " Office "), "Office");
    }

    #[test]
    fn remove_presets() {
        let trip = |name: &str| format!("[[trips]]\nname = \"{name}\"\nproject = \"p\"\nfrom = \"a\"\nto = \"b\"\n");
        let text = format!(
            "# top\nbillable_goal = 70\n\n[projects]\n# keep me\nIntern = []\n\n# about trips\n{}\n# second\n{}\n{}",
            trip("Commute"),
            trip("Client"),
            trip("Gym")
        );
        let names = |t: &str| parse_config(t).unwrap().trips.into_iter().map(|t| t.name).collect::<Vec<_>>();

        let first_gone = without(&text, &preset_named("commute")).unwrap();
        assert_eq!(names(&first_gone), ["Client", "Gym"]);
        assert!(first_gone.contains("# top") && first_gone.contains("# keep me"));
        assert!(first_gone.contains("# about trips"), "section comment moves to the next preset");
        assert!(first_gone.contains("# second"), "and the next preset keeps its own");

        let middle_gone = without(&text, &preset_named("Client")).unwrap();
        assert_eq!(names(&middle_gone), ["Commute", "Gym"]);
        assert!(middle_gone.contains("# about trips"));

        let all_gone = ["Commute", "Client", "Gym"].iter().fold(text.clone(), |t, n| without(&t, &preset_named(n)).unwrap());
        assert!(names(&all_gone).is_empty());
        assert!(all_gone.contains("Intern = []"));

        assert!(without(&text, &preset_named("Nope")).is_err());
        assert!(without("billable_goal = 70\n", &preset_named("Commute")).is_err());
    }

    #[test]
    fn rearrange_presets() {
        let trip = |name: &str| format!("[[trips]]\nname = \"{name}\"\nproject = \"p\"\nfrom = \"a\"\nto = \"b\"\n");
        let text = format!(
            "billable_goal = 70\n\n# about trips\n{}\n# second\n{}\n{}\n[projects]\nIntern = []\n\n# about time\n[[time_entries]]\nproject = \"Intern\"\nnotes = \"a\"\n\n[[time_entries]]\nproject = \"Intern\"\nnotes = \"b\"\n",
            trip("Commute"),
            trip("Client"),
            trip("Gym")
        );
        let config = parse_config(&text).unwrap();
        let [commute, client, gym] = [0, 1, 2].map(|i| config.trips[i].clone());
        let times = config.time_entries.clone();

        let done = with_presets(&text, &[gym, commute], &[times[1].clone(), times[0].clone()]).unwrap();
        let config = parse_config(&done).unwrap();
        assert_eq!(
            config.trips.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["Gym", "Commute"],
            "reordered, Client gone"
        );
        assert_eq!(config.time_entries.iter().map(|t| t.notes.as_deref().unwrap()).collect::<Vec<_>>(), ["b", "a"]);
        let at = |needle: &str| done.find(needle).unwrap_or_else(|| panic!("{needle} missing:\n{done}"));
        assert!(
            at("# about trips") < at("name = \"Gym\"") && at("name = \"Gym\"") < at("name = \"Commute\""),
            "section comment stays on top:\n{done}"
        );
        assert!(!done.contains("# second"), "Client's own comment went with it");
        assert!(at("# about time") < at("notes = \"b\""));
        assert!(done.contains("Intern = []"));

        let none = with_presets(&text, &[], &[]).unwrap();
        assert!(parse_config(&none).unwrap().trips.is_empty() && !none.contains("[[time_entries]]"));
        assert!(with_presets(&text, &[preset_named("Nope")], &[]).is_err());
        assert_eq!(with_presets(&text, &[client], &times).unwrap().matches("[[trips]]").count(), 1);
    }
}
