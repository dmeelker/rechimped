use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use gpui::{App, Entity, FocusHandle, ScrollHandle, Subscription};
use gpuikit::elements::checkbox::Checkbox;
use gpuikit::elements::dialog::DialogState;
use gpuikit::input::InputState;
use timechimp_api::Project;

use super::{TypeAhead, text};
use crate::domain::same_name;
use crate::session::Session;
use crate::store::config::{TimePreset, TripPreset};
use crate::validate::SettingsValues;

/// The categories of the Settings window, listed on its left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsPage {
    General,
    Projects,
    Tasks,
    ProjectTags,
    Presets,
}

impl SettingsPage {
    pub const ALL: [Self; 5] = [Self::General, Self::Projects, Self::Tasks, Self::ProjectTags, Self::Presets];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Projects => "Projects",
            Self::Tasks => "Tasks",
            Self::ProjectTags => "Project tags",
            Self::Presets => "Presets",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::General => "Your work week and billable target.",
            Self::Projects => "The projects the forms offer. Entries of hidden projects still show everywhere.",
            Self::Tasks => "The tasks the time form offers, per project. Entries of hidden tasks still show everywhere.",
            Self::ProjectTags => "The tags the time form offers as checkboxes, per project.",
            Self::Presets => "The presets in the dropdowns of \"+ Add trip\" and \"+ Add time\", in this order.",
        }
    }
}

pub struct SettingsForm {
    pub page: SettingsPage,
    /// The window's own, so its keys work on pages without a text field.
    pub focus: FocusHandle,
    pub work_days: Vec<Entity<Checkbox>>,
    pub billable_goal: Entity<InputState>,
    pub projects: ProjectVisibility,
    pub tasks: TaskVisibility,
    pub tags: ProjectTagsForm,
    pub presets: PresetsForm,
    pub error: Option<String>,
    /// What the window opened with, to tell whether anything changed.
    pub initial: SettingsValues,
    /// The "Discard changes?" dialog, while open.
    pub discard: Option<(Entity<DialogState>, Subscription)>,
}

impl SettingsForm {
    pub fn values(&self, cx: &App) -> SettingsValues {
        let mut work_days = [false; 7];
        for (day, checkbox) in work_days.iter_mut().zip(&self.work_days) {
            *day = checkbox.read(cx).is_checked();
        }
        SettingsValues {
            work_days,
            billable_goal: text(&self.billable_goal, cx),
            hidden_projects: self.projects.hidden_names(),
            hidden_tasks: self.tasks.hidden_names(),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.values(cx) != self.initial || !self.tags.changes().is_empty() || self.presets.is_changed()
    }
}

/// A project as the settings lists show it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRow {
    pub id: i64,
    pub name: String,
    pub customer: String,
    pub active: bool,
}

impl ProjectRow {
    fn of(project: &Project) -> Self {
        Self { id: project.id, name: project.name.clone(), customer: project.customer_name.clone(), active: project.active }
    }
}

fn by_name(a: &ProjectRow, b: &ProjectRow) -> Ordering {
    let lower = |s: &String| s.to_lowercase();
    lower(&a.name).cmp(&lower(&b.name)).then_with(|| lower(&a.customer).cmp(&lower(&b.customer)))
}

/// Shown projects first, then the hidden ones, each by name; `current` stays on its project.
fn order_projects<T>(items: &mut [T], row: impl Fn(&T) -> &ProjectRow, hidden: &BTreeSet<i64>, current: &mut usize) {
    let keep = items.get(*current).map(|item| row(item).id);
    items.sort_by(|a, b| {
        let (a, b) = (row(a), row(b));
        hidden.contains(&a.id).cmp(&hidden.contains(&b.id)).then_with(|| by_name(a, b))
    });
    *current = keep.and_then(|id| items.iter().position(|item| row(item).id == id)).unwrap_or(0);
}

/// The trip and time presets, reordered and removed here and written on Save.
pub struct PresetsForm {
    /// In their new order.
    pub trips: Vec<TripPreset>,
    pub times: Vec<TimePreset>,
    original: (Vec<TripPreset>, Vec<TimePreset>),
    pub trips_scroll: ScrollHandle,
    pub times_scroll: ScrollHandle,
}

impl PresetsForm {
    pub fn new(trips: Vec<TripPreset>, times: Vec<TimePreset>) -> Self {
        let original = (trips.clone(), times.clone());
        Self { trips, times, original, trips_scroll: ScrollHandle::default(), times_scroll: ScrollHandle::default() }
    }

    pub fn is_changed(&self) -> bool {
        self.trips != self.original.0 || self.times != self.original.1
    }
}

/// Moves `items[index]` one place up (`delta` -1) or down (1), if there is room.
pub fn shift<T>(items: &mut [T], index: usize, delta: isize) {
    if let Some(other) = index.checked_add_signed(delta).filter(|&o| o < items.len())
        && index < items.len()
    {
        items.swap(index, other);
    }
}

/// Which list of a page the arrow keys move in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Column {
    #[default]
    Projects,
    Items,
}

/// The keyboard cursor of a list page: in the left list it is the selected project, in the right
/// one `item`.
#[derive(Default)]
pub struct ListNav {
    pub column: Column,
    pub item: usize,
    pub projects_scroll: ScrollHandle,
    pub items_scroll: ScrollHandle,
    typed: TypeAhead,
}

fn step(index: usize, delta: isize, len: usize) -> usize {
    index.saturating_add_signed(delta).min(len.saturating_sub(1))
}

impl ListNav {
    fn items_only() -> Self {
        Self { column: Column::Items, ..Self::default() }
    }

    /// Moves the cursor of the active list; moving `current`, the selected project, puts the
    /// right list's cursor back on top.
    fn move_by(&mut self, delta: isize, current: &mut usize, projects: usize, items: usize) {
        match self.column {
            Column::Projects if projects > 0 => {
                *current = step(*current, delta, projects);
                self.item = 0;
                self.projects_scroll.scroll_to_item(*current);
                self.items_scroll.scroll_to_item(0);
            }
            Column::Items if items > 0 => {
                self.item = step(self.item, delta, items);
                self.items_scroll.scroll_to_item(self.item);
            }
            _ => {}
        }
    }

    /// Moves the cursor of the active list to the first row starting with what was typed.
    fn type_ahead<'a>(
        &mut self,
        typed: &str,
        current: &mut usize,
        projects: impl IntoIterator<Item = &'a str>,
        items: impl IntoIterator<Item = &'a str>,
    ) {
        match self.column {
            Column::Projects => {
                if let Some(index) = self.typed.find(typed, projects) {
                    *current = index;
                    self.item = 0;
                    self.projects_scroll.scroll_to_item(index);
                    self.items_scroll.scroll_to_item(0);
                }
            }
            Column::Items => {
                if let Some(index) = self.typed.find(typed, items) {
                    self.item = index;
                    self.items_scroll.scroll_to_item(index);
                }
            }
        }
    }

    fn pick_project(&mut self, current: &mut usize, index: usize) {
        *current = index;
        self.item = 0;
        self.column = Column::Projects;
    }

    fn pick_item(&mut self, index: usize) {
        self.item = index;
        self.column = Column::Items;
    }
}

pub struct ProjectVisibility {
    /// Active projects, by name.
    pub projects: Vec<ProjectRow>,
    pub hidden: BTreeSet<i64>,
    /// Hidden names matching no active project (gone or inactive), kept so saving doesn't drop them.
    others: Vec<String>,
    pub nav: ListNav,
}

impl ProjectVisibility {
    pub fn new(session: &Session) -> Self {
        let mut projects: Vec<_> = session.projects().iter().filter(|p| p.active).map(ProjectRow::of).collect();
        projects.sort_by(by_name);
        let hidden = projects.iter().filter(|p| session.is_hidden(p.id)).map(|p| p.id).collect();
        let others = session
            .settings
            .hidden_projects
            .iter()
            .filter(|name| !projects.iter().any(|p| same_name(&p.name, name)))
            .cloned()
            .collect();
        Self { projects, hidden, others, nav: ListNav::items_only() }
    }

    pub fn toggle(&mut self, project_id: i64) {
        if !self.hidden.remove(&project_id) {
            self.hidden.insert(project_id);
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        self.nav.move_by(delta, &mut 0, 0, self.projects.len());
    }

    pub fn type_ahead(&mut self, typed: &str) {
        self.nav.type_ahead(typed, &mut 0, [], self.projects.iter().map(|p| p.name.as_str()));
    }

    /// Toggles the project under the cursor, after moving it to `index` if given.
    pub fn toggle_at(&mut self, index: Option<usize>) {
        if let Some(index) = index {
            self.nav.pick_item(index);
        }
        if let Some(id) = self.projects.get(self.nav.item).map(|p| p.id) {
            self.toggle(id);
        }
    }

    pub fn set_all(&mut self, shown: bool) {
        self.hidden = if shown { BTreeSet::new() } else { self.projects.iter().map(|p| p.id).collect() };
    }

    pub fn hidden_names(&self) -> Vec<String> {
        let listed = self.projects.iter().filter(|p| self.hidden.contains(&p.id)).map(|p| p.name.clone());
        listed.chain(self.others.iter().cloned()).collect()
    }
}

pub struct TaskVisibility {
    /// Active projects with active tasks, each with those tasks (id, name) by name.
    pub projects: Vec<(ProjectRow, Vec<(i64, String)>)>,
    pub current: usize,
    pub hidden: HashMap<i64, BTreeSet<i64>>,
    /// The projects hidden on the Projects page, listed last and dimmed.
    pub hidden_projects: BTreeSet<i64>,
    /// Hidden names matching no listed project or task, kept so saving doesn't drop them.
    others: BTreeMap<String, Vec<String>>,
    pub nav: ListNav,
}

impl TaskVisibility {
    pub fn new(session: &Session, hidden_projects: &BTreeSet<i64>) -> Self {
        let projects: Vec<_> = session
            .projects()
            .iter()
            .filter(|p| p.active)
            .filter_map(|p| {
                let mut tasks: Vec<_> =
                    session.project_tasks(p.id).iter().filter(|t| t.active).map(|t| (t.id, t.name.clone())).collect();
                tasks.sort_by_key(|(_, name)| name.to_lowercase());
                (!tasks.is_empty()).then(|| (ProjectRow::of(p), tasks))
            })
            .collect();
        let hidden = projects
            .iter()
            .map(|(p, tasks)| (p.id, tasks.iter().filter(|(_, t)| session.is_task_hidden(p.id, t)).map(|(t, _)| *t).collect()))
            .collect();
        let mut others = BTreeMap::new();
        for (project, names) in &session.settings.hidden_tasks {
            let listed = projects.iter().find(|(p, _)| same_name(&p.name, project)).map(|(_, tasks)| tasks);
            let unlisted: Vec<String> = names
                .iter()
                .filter(|name| !listed.is_some_and(|tasks| tasks.iter().any(|(_, t)| same_name(t, name))))
                .cloned()
                .collect();
            if !unlisted.is_empty() {
                others.insert(project.clone(), unlisted);
            }
        }
        let mut form =
            Self { projects, current: 0, hidden, hidden_projects: BTreeSet::new(), others, nav: ListNav::default() };
        form.order(hidden_projects);
        form.current = 0;
        form
    }

    /// Lists the projects hidden on the Projects page last, keeping the selection.
    pub fn order(&mut self, hidden_projects: &BTreeSet<i64>) {
        order_projects(&mut self.projects, |(p, _)| p, hidden_projects, &mut self.current);
        self.hidden_projects = hidden_projects.clone();
    }

    pub fn current_project(&self) -> Option<i64> {
        self.projects.get(self.current).map(|(p, _)| p.id)
    }

    pub fn is_hidden(&self, project_id: i64, task_id: i64) -> bool {
        self.hidden.get(&project_id).is_some_and(|t| t.contains(&task_id))
    }

    pub fn hidden_count(&self, project_id: i64) -> usize {
        self.hidden.get(&project_id).map_or(0, BTreeSet::len)
    }

    pub fn toggle(&mut self, task_id: i64) {
        let Some(project) = self.current_project() else { return };
        let hidden = self.hidden.entry(project).or_default();
        if !hidden.remove(&task_id) {
            hidden.insert(task_id);
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let tasks = self.projects.get(self.current).map_or(0, |(_, tasks)| tasks.len());
        self.nav.move_by(delta, &mut self.current, self.projects.len(), tasks);
    }

    pub fn type_ahead(&mut self, typed: &str) {
        let projects = self.projects.iter().map(|(p, _)| p.name.as_str());
        let tasks = self.projects.get(self.current).into_iter().flat_map(|(_, tasks)| tasks.iter().map(|(_, t)| t.as_str()));
        self.nav.type_ahead(typed, &mut self.current, projects, tasks);
    }

    pub fn pick_project(&mut self, index: usize) {
        self.nav.pick_project(&mut self.current, index);
    }

    /// Toggles the task under the cursor, after moving it to `index` if given.
    pub fn toggle_at(&mut self, index: Option<usize>) {
        if let Some(index) = index {
            self.nav.pick_item(index);
        }
        let task = self.projects.get(self.current).and_then(|(_, tasks)| tasks.get(self.nav.item));
        if self.nav.column == Column::Items
            && let Some(&(task, _)) = task
        {
            self.toggle(task);
        }
    }

    pub fn set_all(&mut self, shown: bool) {
        let Some((p, tasks)) = self.projects.get(self.current) else { return };
        let hidden = if shown { BTreeSet::new() } else { tasks.iter().map(|(t, _)| *t).collect() };
        self.hidden.insert(p.id, hidden);
    }

    /// Per project name; projects with nothing hidden are left out.
    pub fn hidden_names(&self) -> BTreeMap<String, Vec<String>> {
        let mut names = BTreeMap::new();
        for (project, tasks) in &self.projects {
            let others =
                self.others.iter().find(|(p, _)| same_name(p, &project.name)).map(|(_, t)| t.as_slice()).unwrap_or_default();
            let hidden: Vec<String> = tasks
                .iter()
                .filter(|(t, _)| self.is_hidden(project.id, *t))
                .map(|(_, name)| name.clone())
                .chain(others.iter().cloned())
                .collect();
            if !hidden.is_empty() {
                names.insert(project.name.clone(), hidden);
            }
        }
        for (project, tasks) in &self.others {
            if !self.projects.iter().any(|(p, _)| same_name(&p.name, project)) {
                names.insert(project.clone(), tasks.clone());
            }
        }
        names
    }
}

pub struct ProjectTagsForm {
    /// Active projects, and inactive ones that still have tags configured.
    pub projects: Vec<ProjectRow>,
    pub tags: Vec<(i64, String)>,
    pub current: usize,
    pub chosen: HashMap<i64, BTreeSet<i64>>,
    /// The projects hidden on the Projects page, listed last and dimmed.
    pub hidden_projects: BTreeSet<i64>,
    /// What the page opened with, so only the projects changed are written.
    original: HashMap<i64, BTreeSet<i64>>,
    /// Whether the config names tags for any project; until then every project offers all tags.
    pub configured: bool,
    pub nav: ListNav,
}

impl ProjectTagsForm {
    pub fn new(session: &Session, hidden_projects: &BTreeSet<i64>) -> Self {
        let in_use: BTreeSet<i64> = session.project_tags.values().flatten().map(|t| t.id).collect();
        let projects: Vec<_> = session
            .projects()
            .iter()
            .filter(|p| p.active || session.project_tags.get(&p.id).is_some_and(|t| !t.is_empty()))
            .map(ProjectRow::of)
            .collect();
        let chosen: HashMap<i64, BTreeSet<i64>> =
            projects.iter().map(|p| (p.id, session.tags_for(Some(p.id)).iter().map(|t| t.id).collect())).collect();
        let mut tags: Vec<_> = session
            .tags()
            .iter()
            .filter(|t| !t.name.starts_with("z_") || in_use.contains(&t.id))
            .map(|t| (t.id, t.name.clone()))
            .collect();
        tags.sort_by_key(|(_, name)| name.to_lowercase());
        let configured = session.tags_configured();
        let mut form = Self {
            projects,
            tags,
            current: 0,
            original: chosen.clone(),
            chosen,
            hidden_projects: BTreeSet::new(),
            configured,
            nav: ListNav::default(),
        };
        form.order(hidden_projects);
        form.current = 0;
        form
    }

    /// Lists the projects hidden on the Projects page last, keeping the selection.
    pub fn order(&mut self, hidden_projects: &BTreeSet<i64>) {
        order_projects(&mut self.projects, |p| p, hidden_projects, &mut self.current);
        self.hidden_projects = hidden_projects.clone();
    }

    pub fn current_project(&self) -> Option<i64> {
        self.projects.get(self.current).map(|p| p.id)
    }

    pub fn is_checked(&self, project_id: i64, tag_id: i64) -> bool {
        self.chosen.get(&project_id).is_some_and(|t| t.contains(&tag_id))
    }

    pub fn count(&self, project_id: i64) -> usize {
        self.chosen.get(&project_id).map_or(0, BTreeSet::len)
    }

    pub fn toggle(&mut self, tag_id: i64) {
        let Some(project) = self.current_project() else { return };
        let tags = self.chosen.entry(project).or_default();
        if !tags.remove(&tag_id) {
            tags.insert(tag_id);
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        self.nav.move_by(delta, &mut self.current, self.projects.len(), self.tags.len());
    }

    pub fn type_ahead(&mut self, typed: &str) {
        let projects = self.projects.iter().map(|p| p.name.as_str());
        self.nav.type_ahead(typed, &mut self.current, projects, self.tags.iter().map(|(_, t)| t.as_str()));
    }

    pub fn pick_project(&mut self, index: usize) {
        self.nav.pick_project(&mut self.current, index);
    }

    /// Toggles the tag under the cursor, after moving it to `index` if given.
    pub fn toggle_at(&mut self, index: Option<usize>) {
        if let Some(index) = index {
            self.nav.pick_item(index);
        }
        if self.nav.column == Column::Items
            && let Some(&(tag, _)) = self.tags.get(self.nav.item)
        {
            self.toggle(tag);
        }
    }

    pub fn set_all(&mut self, checked: bool) {
        let Some(project) = self.current_project() else { return };
        let all = if checked { self.tags.iter().map(|(id, _)| *id).collect() } else { BTreeSet::new() };
        self.chosen.insert(project, all);
    }

    /// The projects to write. The first change while nothing is configured writes them all, since
    /// from then on a project without tags in the config offers none.
    pub fn changes(&self) -> Vec<(String, Vec<String>)> {
        let none = BTreeSet::new();
        let changed = |id: &i64| self.chosen.get(id).unwrap_or(&none) != self.original.get(id).unwrap_or(&none);
        let any = self.projects.iter().any(|p| changed(&p.id));
        let mut changes: Vec<_> = self
            .projects
            .iter()
            .filter(|p| any && (!self.configured || changed(&p.id)))
            .map(|p| {
                let names = self.tags.iter().filter(|(t, _)| self.is_checked(p.id, *t)).map(|(_, n)| n.clone()).collect();
                (p.name.clone(), names)
            })
            .collect();
        changes.sort_by_key(|(name, _)| name.to_lowercase());
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn project_visibility() {
        let mut s = Session::new(None, date(4));
        let mut inactive = project(9, "Inactive");
        inactive.active = false;
        s.apply_base(base(vec![project(3, "b"), project(6, "A"), inactive], HashMap::new(), Vec::new()), None, &[]);
        s.settings.hidden_projects = vec!["B".into(), "Inactive".into(), "Gone".into()];

        let mut form = ProjectVisibility::new(&s);
        assert_eq!(form.projects.iter().map(|p| p.id).collect::<Vec<_>>(), [6, 3], "active ones, by name");
        assert_eq!(form.hidden, BTreeSet::from([3]));
        assert_eq!(form.hidden_names(), ["b", "Inactive", "Gone"], "unlisted names are kept");
        form.toggle(3);
        form.toggle(6);
        assert_eq!(form.hidden_names(), ["A", "Inactive", "Gone"]);
        form.set_all(false);
        assert_eq!(form.hidden.len(), 2);
        form.set_all(true);
        assert_eq!(form.hidden_names(), ["Inactive", "Gone"]);
    }

    #[test]
    fn task_visibility() {
        let mut s = Session::new(None, date(4));
        let mut gone = task(40, 3, "Gone");
        gone.active = false;
        let tasks = HashMap::from([(3, vec![task(10, 3, "b"), task(20, 3, "A"), gone]), (6, Vec::new())]);
        s.apply_base(base(vec![project(3, "P"), project(6, "Empty")], tasks, Vec::new()), None, &[]);
        s.settings.hidden_tasks =
            BTreeMap::from([("p".into(), vec!["B".into(), "Gone".into()]), ("Old".into(), vec!["X".into()])]);

        let mut form = TaskVisibility::new(&s, &BTreeSet::new());
        assert_eq!(form.projects.iter().map(|(p, _)| p.id).collect::<Vec<_>>(), [3], "only projects with active tasks");
        assert_eq!(form.projects[0].1.iter().map(|t| t.0).collect::<Vec<_>>(), [20, 10], "tasks by name");
        assert!(form.is_hidden(3, 10) && !form.is_hidden(3, 20));
        let names = |pairs: &[(&str, &[&str])]| {
            pairs.iter().map(|(p, t)| ((*p).to_owned(), t.iter().map(|t| (*t).to_owned()).collect())).collect::<BTreeMap<_, Vec<_>>>()
        };
        assert_eq!(form.hidden_names(), names(&[("P", &["b", "Gone"]), ("Old", &["X"])]), "unlisted names are kept");
        form.toggle(10);
        form.toggle(20);
        assert_eq!(form.hidden_names(), names(&[("P", &["A", "Gone"]), ("Old", &["X"])]));
        form.set_all(false);
        assert_eq!(form.hidden_count(3), 2);
        form.set_all(true);
        assert_eq!(form.hidden_names(), names(&[("P", &["Gone"]), ("Old", &["X"])]));
    }

    #[test]
    fn project_tags_form() {
        let mut s = Session::new(None, date(4));
        let mut old = project(9, "old");
        old.active = false;
        let tags = vec![tag(10, "b"), tag(20, "A"), tag(30, "z_hidden"), tag(40, "z_used")];
        s.apply_base(base(vec![project(3, "P"), project(6, "Other"), old], HashMap::new(), tags), None, &[]);
        s.project_tags = HashMap::from([(3, vec![tag(40, "z_used")])]);

        let mut form = ProjectTagsForm::new(&s, &BTreeSet::new());
        assert_eq!(form.projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Other", "P"], "by name, inactive left out");
        assert_eq!(form.tags.iter().map(|t| t.0).collect::<Vec<_>>(), [20, 10, 40], "by name; z_ only when in use");
        assert!(form.changes().is_empty());

        assert_eq!(form.current_project(), Some(6));
        form.toggle(10);
        form.toggle(20);
        form.toggle(20);
        form.current = 1;
        form.toggle(40);
        assert_eq!(form.changes(), [("Other".to_owned(), vec!["b".to_owned()]), ("P".to_owned(), Vec::new())]);
        assert_eq!((form.count(6), form.count(3)), (1, 0));
        form.set_all(true);
        assert_eq!((form.count(6), form.count(3)), (1, 3), "all offered, for the shown project only");
        form.set_all(false);
        assert_eq!(form.count(3), 0);

        s.project_tags = HashMap::new();
        let mut form = ProjectTagsForm::new(&s, &BTreeSet::new());
        assert_eq!(form.count(6), 2, "all tags while none are configured");
        assert!(form.changes().is_empty(), "nothing written unless changed");
        form.toggle(10);
        assert_eq!(form.changes(), [
            ("Other".to_owned(), vec!["A".to_owned()]),
            ("P".to_owned(), vec!["A".to_owned(), "b".to_owned()]),
        ], "the first change writes every project, so the others keep their tags");
    }

    #[test]
    fn hidden_projects_last() {
        let mut s = Session::new(None, date(4));
        let tasks = HashMap::from([(1, vec![task(10, 1, "t")]), (2, vec![task(20, 2, "t")]), (3, vec![task(30, 3, "t")])]);
        s.apply_base(base(vec![project(1, "A"), project(2, "B"), project(3, "C")], tasks, Vec::new()), None, &[]);
        let ids = |f: &TaskVisibility| f.projects.iter().map(|(p, _)| p.id).collect::<Vec<_>>();

        let mut form = TaskVisibility::new(&s, &BTreeSet::from([1]));
        assert_eq!(ids(&form), [2, 3, 1]);
        assert_eq!(form.current_project(), Some(2), "a shown project selected on open");
        form.current = 1;
        form.order(&BTreeSet::from([2]));
        assert_eq!(ids(&form), [1, 3, 2]);
        assert_eq!(form.current_project(), Some(3), "the selection is kept");

        let form = ProjectTagsForm::new(&s, &BTreeSet::from([1, 2]));
        assert_eq!(form.projects.iter().map(|p| p.id).collect::<Vec<_>>(), [3, 1, 2]);
    }

    #[test]
    fn keyboard_cursor() {
        let mut s = Session::new(None, date(4));
        let tasks = HashMap::from([(1, vec![task(10, 1, "a"), task(11, 1, "b")]), (2, vec![task(20, 2, "c")])]);
        s.apply_base(base(vec![project(1, "A"), project(2, "B")], tasks, Vec::new()), None, &[]);
        let mut form = TaskVisibility::new(&s, &BTreeSet::new());

        form.toggle_at(None);
        assert_eq!(form.hidden_count(1), 0, "Space does nothing in the project list");
        form.nav.column = Column::Items;
        form.move_cursor(5);
        assert_eq!(form.nav.item, 1, "stops at the last task");
        form.toggle_at(None);
        assert!(form.is_hidden(1, 11));
        form.nav.column = Column::Projects;
        form.move_cursor(1);
        assert_eq!((form.current_project(), form.nav.item), (Some(2), 0), "a new project starts at its first task");
        form.move_cursor(isize::MIN);
        assert_eq!(form.current, 0);
        form.toggle_at(Some(0));
        assert!(form.is_hidden(1, 10) && form.nav.column == Column::Items, "a click moves the cursor there");

        form.nav.column = Column::Projects;
        form.type_ahead("b");
        assert_eq!((form.current_project(), form.nav.item), (Some(2), 0), "typing jumps to a project");
        form.nav.column = Column::Items;
        form.pick_project(0);
        form.nav.column = Column::Items;
        form.type_ahead("x");
        form.type_ahead("b");
        assert_eq!(form.nav.item, 1, "a prefix nothing matches starts over");

        let mut projects = ProjectVisibility::new(&s);
        projects.move_cursor(1);
        projects.toggle_at(None);
        assert_eq!(projects.hidden, BTreeSet::from([2]));
    }

    #[test]
    fn shift_moves_within_bounds() {
        let mut v = vec![1, 2, 3];
        shift(&mut v, 0, -1);
        assert_eq!(v, [1, 2, 3]);
        shift(&mut v, 0, 1);
        assert_eq!(v, [2, 1, 3]);
        shift(&mut v, 2, 1);
        assert_eq!(v, [2, 1, 3]);
        shift(&mut v, 2, -1);
        assert_eq!(v, [2, 3, 1]);
        shift(&mut v, 7, -1);
        assert_eq!(v, [2, 3, 1]);
    }
}
