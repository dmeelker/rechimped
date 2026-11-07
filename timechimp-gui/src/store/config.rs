//! `config.toml`: the billable goal, the tags each project may use (shown as checkboxes in the
//! time form), preset trips and preset time entries. Earlier versions called it `projects.toml`; that file is renamed
//! on first load.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use timechimp::{MileageType, Project, Tag};


/// Share of hours that should be billable, unless `billable_goal` in `config.toml` says otherwise.
pub const DEFAULT_BILLABLE_GOAL: f64 = 0.75;

/// Where `config.toml` lives (in `store::config_dir`).
pub fn path() -> Result<PathBuf, String> {
    Ok(super::config_dir().ok_or("no config directory (APPDATA, XDG_CONFIG_HOME and HOME are unset)")?.join("config.toml"))
}

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    /// Percentage, e.g. `75`.
    billable_goal: Option<f64>,
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
        str_field(table, "name").is_some_and(|n| n.eq_ignore_ascii_case(&self.name))
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
    /// Fraction, e.g. `0.75`.
    pub billable_goal: f64,
    /// Project name → tag names.
    pub projects: HashMap<String, Vec<String>>,
    pub trips: Vec<TripPreset>,
    pub time_entries: Vec<TimePreset>,
}

fn parse_config(text: &str) -> Result<Config, String> {
    let file: ConfigFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let billable_goal = match file.billable_goal {
        None => DEFAULT_BILLABLE_GOAL,
        Some(pct) if (0.0..=100.0).contains(&pct) => pct / 100.0,
        Some(pct) => return Err(format!("billable_goal must be a percentage between 0 and 100, not {pct}")),
    };
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
    Ok(Config { billable_goal, projects: file.projects, trips: file.trips, time_entries: file.time_entries })
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
    std::fs::write(path, text).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

/// Removes `preset` from `config.toml`, leaving the rest of the file (comments included) as it is.
pub fn remove_preset<P: Preset>(path: &Path, preset: &P) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let text = without(&text, preset).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(path, text).map_err(|e| format!("Could not write {}: {e}", path.display()))
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
        // Move it over with the (then new) goal setting on top, where top-level keys must go.
        let old = std::fs::read_to_string(&legacy).map_err(|e| format!("Could not read {}: {e}", legacy.display()))?;
        let migrated = format!(
            "# Percentage of your hours that should be billable.\nbillable_goal = {}\n\n{old}",
            DEFAULT_BILLABLE_GOAL * 100.0
        );
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
pub fn write_template(path: &Path, projects: &[Project], tags: &[Tag]) -> std::io::Result<()> {
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
        assert_eq!(config.billable_goal, DEFAULT_BILLABLE_GOAL);
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
        assert_eq!(config.billable_goal, DEFAULT_BILLABLE_GOAL);
        assert!(config.projects.is_empty() && config.trips.is_empty() && config.time_entries.is_empty());
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
