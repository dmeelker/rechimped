//! `config.toml`: the work settings (work days, billable goal), the tags each project may use (shown as checkboxes in the
//! time form), preset trips and preset time entries. Earlier versions called it `projects.toml`; that file is renamed
//! on first load.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use timechimp::{MileageType, Project, Tag};

use crate::domain::same_name;


/// Share of hours that should be billable, unless `billable_goal` in `config.toml` says otherwise.
pub const DEFAULT_BILLABLE_GOAL: f64 = 0.8;

/// How the days of the week are written in `work_days`, Monday first.
pub const DAY_NAMES: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// The key `work_days` uses for day `i` (Monday = 0): `mon`, `tue`, …
fn day_key(i: usize) -> &'static str {
    &DAY_NAMES[i][..3]
}

/// What the Settings dialog edits: the top-level keys of `config.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The days worked, Monday first.
    pub work_days: [bool; 7],
    /// Fraction, e.g. `0.8`.
    pub billable_goal: f64,
    /// Names of the projects the forms don't offer (matched case-insensitively); their entries
    /// still show everywhere, and editing one still offers its project.
    pub hidden_projects: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            work_days: [true, true, true, true, true, false, false],
            billable_goal: DEFAULT_BILLABLE_GOAL,
            hidden_projects: Vec::new(),
        }
    }
}

/// Where `config.toml` lives (in `store::config_dir`).
pub fn path() -> Result<PathBuf, String> {
    Ok(super::config_dir().ok_or("no config directory (APPDATA, XDG_CONFIG_HOME and HOME are unset)")?.join("config.toml"))
}

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    /// Day names (`DAY_KEYS`; full names and any case work too).
    work_days: Option<Vec<String>>,
    /// Percentage, e.g. `80`.
    billable_goal: Option<f64>,
    /// Project names.
    #[serde(default)]
    hidden_projects: Vec<String>,
    /// Project name → tag names.
    #[serde(default)]
    projects: HashMap<String, Vec<String>>,
    #[serde(default)]
    trips: Vec<TripPreset>,
    #[serde(default)]
    time_entries: Vec<TimePreset>,
}

/// A kind of preset: an array of tables in `config.toml`, appended to and removed from as text so
/// the rest of the file (comments included) stays as it is.
pub trait Preset: serde::Serialize {
    /// The `[[KEY]]` the presets are listed under.
    const KEY: &'static str;
    /// The comment written above the first one.
    const COMMENT: &'static str;
    /// Whether `table` (one of the `[[KEY]]` entries) is this preset.
    fn is(&self, table: &toml_edit::Table) -> bool;
}

fn str_field<'a>(table: &'a toml_edit::Table, key: &str) -> Option<&'a str> {
    table.get(key).and_then(|v| v.as_str())
}

/// A `[[trips]]` entry in `config.toml`: a trip made often (a commute, a regular client visit),
/// offered in the dropdown of "+ Add trip", which opens the trip form filled in.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct TripPreset {
    pub name: String,
    /// Project name, matched case-insensitively.
    pub project: String,
    pub from: String,
    pub to: String,
    /// Kilometres one way; left out, the form's Calculate fills it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    #[serde(default)]
    pub return_trip: bool,
    /// A key from `TRIP_TYPE_KEYS` ("business", "home-work"); defaults to business.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub mileage_type: Option<String>,
    #[serde(default)]
    pub billable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Preset for TripPreset {
    const KEY: &'static str = "trips";
    const COMMENT: &'static str = TRIPS_COMMENT;
    /// Trips are known by name.
    fn is(&self, table: &toml_edit::Table) -> bool {
        str_field(table, "name").is_some_and(|n| same_name(n, &self.name))
    }
}

/// A `[[time_entries]]` entry in `config.toml`: work done often (a standup, a regular project),
/// offered in the dropdown of "+ Add time", which opens the time form filled in.
#[derive(serde::Deserialize, serde::Serialize, Clone, Debug, PartialEq)]
pub struct TimePreset {
    /// Project name, matched case-insensitively.
    pub project: String,
    /// Task name (case-insensitive); left out, the project's first task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The entry's description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Tag names (case-insensitive).
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
    /// The trip type; None when `type` names no known type. Matching ignores case and anything
    /// but letters and digits, so `Home work` and `homework` work too.
    pub fn trip_type(&self) -> Option<MileageType> {
        let Some(name) = &self.mileage_type else { return Some(MileageType::Business) };
        let key = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        TRIP_TYPE_KEYS.iter().find(|(_, k)| key(k) == key(name)).map(|(t, _)| *t)
    }
}

/// How each trip type is written in `config.toml`. Part of the file format, so kept apart from the
/// labels the forms show (`domain::MILEAGE_TYPES`): rewording those mustn't break existing files.
const TRIP_TYPE_KEYS: [(MileageType, &str); 2] = [(MileageType::Business, "business"), (MileageType::HomeWork, "home-work")];

/// How a trip type is written in `config.toml`.
pub fn mileage_type_key(mileage_type: MileageType) -> Option<String> {
    TRIP_TYPE_KEYS.iter().find(|(t, _)| *t == mileage_type).map(|(_, key)| (*key).to_owned())
}

/// A short name for a new preset: its notes, else the start of each address ("A → B").
pub fn preset_name(from: &str, to: &str, notes: &str) -> String {
    if !notes.trim().is_empty() {
        return notes.trim().to_owned();
    }
    let short = |a: &str| a.split(',').next().unwrap_or(a).trim().to_owned();
    format!("{} → {}", short(from), short(to))
}

pub struct Config {
    pub settings: Settings,
    /// Project name → tag names.
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
    let settings = Settings { work_days, billable_goal, hidden_projects: file.hidden_projects };
    for trip in &file.trips {
        if trip.trip_type().is_none() {
            let known: Vec<&str> = TRIP_TYPE_KEYS.iter().map(|(_, key)| *key).collect();
            return Err(format!(
                "trip \"{}\": unknown type \"{}\" (use {})",
                trip.name,
                trip.mileage_type.as_deref().unwrap_or_default(),
                known.join(" or ")
            ));
        }
    }
    Ok(Config { settings, projects: file.projects, trips: file.trips, time_entries: file.time_entries })
}

/// Monday = 0 for `mon`, `Monday`, `TUES`, …: the start (three letters or more) of a day's name.
fn day_index(name: &str) -> Option<usize> {
    let name = name.trim().to_lowercase();
    (name.len() >= 3).then(|| DAY_NAMES.iter().position(|full| full.starts_with(&name))).flatten()
}

/// Writes `text` to `path` via a temporary file next to it and a rename, so a crash or full disk
/// halfway leaves the old file (often edited by hand) intact rather than truncated.
fn replace_file(path: &Path, text: &str) -> Result<(), String> {
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            format!("Could not write {}: {e}", path.display())
        })
}

/// A whole number as an integer (`40`, not `40.0`), as people write it.
fn number(value: f64) -> toml_edit::Item {
    if value.fract() == 0.0 && value.abs() < 1e15 { toml_edit::value(value as i64) } else { toml_edit::value(value) }
}

/// Writes `settings` to the top-level keys of `config.toml`, leaving the rest of the file
/// (comments included) as it is. Creates the file when missing.
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
    let days: toml_edit::Array =
        (0..7).zip(settings.work_days).filter(|(_, on)| *on).map(|(i, _)| day_key(i)).collect();
    // Set in place, so a key keeps its position and the comment above it.
    let set = |doc: &mut toml_edit::DocumentMut, key: &str, item: toml_edit::Item, comment: &str| match doc.get_mut(key).and_then(|i| i.as_value_mut()) {
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
    // Only written once something is hidden, so a new file doesn't start with it.
    if !settings.hidden_projects.is_empty() || doc.contains_key("hidden_projects") {
        let hidden: toml_edit::Array = settings.hidden_projects.iter().map(String::as_str).collect();
        let comment = "# Projects the forms don't offer (Settings, in the menu under your name).\n";
        set(&mut doc, "hidden_projects", toml_edit::value(hidden), comment);
    }
    let text = doc.to_string();
    parse_config(&text)?;
    Ok(text)
}

/// Writes the tag names of the given projects to `[projects]`, leaving the other projects and the
/// rest of the file (comments included) as they are. A project's existing key (matched
/// case-insensitively) is changed in place; a project not in the file yet is added unless it gets
/// no tags. Creates the file when missing.
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
    let projects = doc
        .entry("projects")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("[projects] is not a table")?;
    for (project, tags) in changes {
        let tags: toml_edit::Array = tags.iter().map(String::as_str).collect();
        let existing = projects.iter().map(|(key, _)| key.to_owned()).find(|key| same_name(key, project));
        match existing.and_then(|key| projects.get_mut(&key)).and_then(|item| item.as_value_mut()) {
            // In place, so the key keeps its position and the comment above it.
            Some(value) => {
                let decor = value.decor().clone();
                *value = toml_edit::Value::Array(tags);
                *value.decor_mut() = decor;
            }
            None if !tags.is_empty() => {
                projects.insert(project, toml_edit::value(tags));
            }
            None => {}
        }
    }
    let text = doc.to_string();
    parse_config(&text)?;
    Ok(text)
}

/// The `[[KEY]]` block for `preset`, as appended to `config.toml`.
fn preset_toml<P: Preset>(preset: &P) -> Result<String, String> {
    toml::to_string(&std::collections::BTreeMap::from([(P::KEY, [preset])])).map_err(|e| e.to_string())
}

/// Adds `preset` to the end of `config.toml` (as text, so the comments stay).
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
    // Make sure the result still parses before touching the file.
    parse_config(&text).map_err(|e| format!("Error in {}: {e}", path.display()))?;
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    replace_file(path, &text)
}

/// Removes `preset` from `config.toml`, leaving the rest of the file (comments included) as it is.
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

const TRIPS_COMMENT: &str = "# Preset trips, in the dropdown of \"+ Add trip\" on the Mileage tab. Fields: name,\n\
     # project, from, to, distance (km, one way; optional), return_trip, type (\"business\" or\n\
     # \"home-work\"), billable, notes (optional).\n";

const TIME_ENTRIES_COMMENT: &str = "# Preset time entries, in the dropdown of \"+ Add time\" on the Week tab. Fields: project,\n\
     # task (optional; the project's first task), notes (optional), tags (optional list of names).\n";

/// Reads `config.toml`. `Ok(None)` if the file doesn't exist.
pub fn load(path: &Path) -> Result<Option<Config>, String> {
    let legacy = path.with_file_name("projects.toml");
    if !path.exists() && legacy.exists() {
        // Move it over with the (then new) settings on top, where top-level keys must go.
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

/// The default settings with their comments, for the top of a new `config.toml`.
fn settings_toml() -> String {
    with_settings("", &Settings::default()).expect("the defaults are valid")
}

/// Writes a starter `config.toml` listing the user's projects and current (non-`z_`) tags.
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

    /// A fresh directory under the system temp dir, removed again on drop.
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

        // An old projects.toml is moved over, with the goal setting on top.
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
            mileage_type: mileage_type_key(MileageType::HomeWork),
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

        // A broken file is reported, not overwritten.
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
            mileage_type: None,
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
        assert!(saved.contains("# keep me\nintern = [\"a\", \"b\"]\n"), "existing key changed in place, its spelling kept:\n{saved}");
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
        };
        // Missing keys are added at the top, with a comment; existing ones change in place.
        let text = "# top\n# goal\nbillable_goal = 75 # inline\n\n[projects]\n# keep me\nIntern = []\n";
        let saved = with_settings(text, &settings).unwrap();
        assert_eq!(parse_config(&saved).unwrap().settings, settings);
        assert!(saved.contains("# goal\nbillable_goal = 70 # inline\n"), "whole numbers without .0: {saved}");
        assert!(saved.contains("# keep me\nIntern = []") && saved.contains("# The days you work"), "{saved}");
        assert!(saved.contains("work_days = [\"tue\", \"wed\", \"fri\", \"sat\"]"), "{saved}");
        assert!(saved.find("work_days").unwrap() < saved.find("[projects]").unwrap(), "top-level keys before the tables");
        assert!(saved.find("hidden_projects").unwrap() < saved.find("[projects]").unwrap(), "{saved}");
        let none_hidden = Settings { hidden_projects: Vec::new(), ..settings.clone() };
        assert!(with_settings(&saved, &none_hidden).unwrap().contains("\nhidden_projects = []"), "an existing key stays");
        assert!(!with_settings("", &none_hidden).unwrap().contains("hidden_projects"), "no key while nothing is hidden");
        // Saving again changes nothing.
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

        // Round trip through the appended block.
        assert_eq!(mileage_type_key(MileageType::HomeWork).as_deref(), Some("home-work"));
        // Every type the trip form offers can be saved in a preset.
        assert!(crate::domain::MILEAGE_TYPES.iter().all(|(t, _)| mileage_type_key(*t).is_some()));
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
}
