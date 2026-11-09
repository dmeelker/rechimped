use std::collections::{BTreeMap, BTreeSet, HashMap};

use gpui::{App, Entity, FocusHandle};
use gpuikit::elements::checkbox::Checkbox;
use gpuikit::input::InputState;

use super::text;
use crate::domain::same_name;
use crate::session::Session;
use crate::validate::SettingsValues;

/// The categories of the Settings window, listed on its left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsPage {
    General,
    Projects,
    Tasks,
    ProjectTags,
}

impl SettingsPage {
    pub const ALL: [Self; 4] = [Self::General, Self::Projects, Self::Tasks, Self::ProjectTags];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Projects => "Projects",
            Self::Tasks => "Tasks",
            Self::ProjectTags => "Project tags",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::General => "Your work week and billable target.",
            Self::Projects => "The projects the forms offer. Entries of hidden projects still show everywhere.",
            Self::Tasks => "The tasks the time form offers, per project. Entries of hidden tasks still show everywhere.",
            Self::ProjectTags => "The tags the time form offers as checkboxes, per project.",
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
    pub error: Option<String>,
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
}

pub struct ProjectVisibility {
    pub projects: Vec<(i64, String, String)>,
    pub hidden: BTreeSet<i64>,
    /// Hidden names matching no active project (gone or inactive), kept so saving doesn't drop them.
    others: Vec<String>,
}

impl ProjectVisibility {
    pub fn new(session: &Session) -> Self {
        let by_name = |a: &String, b: &String| a.to_lowercase().cmp(&b.to_lowercase());
        let mut projects: Vec<_> = session
            .projects()
            .iter()
            .filter(|p| p.active)
            .map(|p| (p.id, p.name.clone(), p.customer_name.clone()))
            .collect();
        projects.sort_by(|a, b| by_name(&a.1, &b.1).then_with(|| by_name(&a.2, &b.2)));
        let hidden = projects.iter().filter(|(id, ..)| session.is_hidden(*id)).map(|(id, ..)| *id).collect();
        let others = session
            .settings
            .hidden_projects
            .iter()
            .filter(|name| !projects.iter().any(|(_, p, _)| same_name(p, name)))
            .cloned()
            .collect();
        Self { projects, hidden, others }
    }

    pub fn toggle(&mut self, project_id: i64) {
        if !self.hidden.remove(&project_id) {
            self.hidden.insert(project_id);
        }
    }

    pub fn set_all(&mut self, shown: bool) {
        self.hidden = if shown { BTreeSet::new() } else { self.projects.iter().map(|(id, ..)| *id).collect() };
    }

    pub fn hidden_names(&self) -> Vec<String> {
        let listed = self.projects.iter().filter(|(id, ..)| self.hidden.contains(id)).map(|(_, name, _)| name.clone());
        listed.chain(self.others.iter().cloned()).collect()
    }
}

/// Id, name, customer and the (id, name) of its tasks.
pub type ProjectWithTasks = (i64, String, String, Vec<(i64, String)>);

pub struct TaskVisibility {
    /// Active projects with active tasks, by name, each with those tasks by name.
    pub projects: Vec<ProjectWithTasks>,
    pub current: usize,
    pub hidden: HashMap<i64, BTreeSet<i64>>,
    /// Hidden names matching no listed project or task, kept so saving doesn't drop them.
    others: BTreeMap<String, Vec<String>>,
}

impl TaskVisibility {
    pub fn new(session: &Session) -> Self {
        let by_name = |a: &String, b: &String| a.to_lowercase().cmp(&b.to_lowercase());
        let mut projects: Vec<_> = session
            .projects()
            .iter()
            .filter(|p| p.active)
            .filter_map(|p| {
                let mut tasks: Vec<_> =
                    session.project_tasks(p.id).iter().filter(|t| t.active).map(|t| (t.id, t.name.clone())).collect();
                tasks.sort_by(|a, b| by_name(&a.1, &b.1));
                (!tasks.is_empty()).then(|| (p.id, p.name.clone(), p.customer_name.clone(), tasks))
            })
            .collect();
        projects.sort_by(|a, b| by_name(&a.1, &b.1).then_with(|| by_name(&a.2, &b.2)));
        let hidden = projects
            .iter()
            .map(|(id, _, _, tasks)| (*id, tasks.iter().filter(|(_, t)| session.is_task_hidden(*id, t)).map(|(t, _)| *t).collect()))
            .collect();
        let mut others = BTreeMap::new();
        for (project, names) in &session.settings.hidden_tasks {
            let listed = projects.iter().find(|(_, p, ..)| same_name(p, project)).map(|(.., tasks)| tasks);
            let unlisted: Vec<String> = names
                .iter()
                .filter(|name| !listed.is_some_and(|tasks| tasks.iter().any(|(_, t)| same_name(t, name))))
                .cloned()
                .collect();
            if !unlisted.is_empty() {
                others.insert(project.clone(), unlisted);
            }
        }
        Self { projects, current: 0, hidden, others }
    }

    pub fn current_project(&self) -> Option<i64> {
        self.projects.get(self.current).map(|p| p.0)
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

    pub fn set_all(&mut self, shown: bool) {
        let Some((id, .., tasks)) = self.projects.get(self.current) else { return };
        let hidden = if shown { BTreeSet::new() } else { tasks.iter().map(|(t, _)| *t).collect() };
        self.hidden.insert(*id, hidden);
    }

    /// Per project name; projects with nothing hidden are left out.
    pub fn hidden_names(&self) -> BTreeMap<String, Vec<String>> {
        let mut names = BTreeMap::new();
        for (id, project, _, tasks) in &self.projects {
            let others = self.others.iter().find(|(p, _)| same_name(p, project)).map(|(_, t)| t.as_slice()).unwrap_or_default();
            let hidden: Vec<String> = tasks
                .iter()
                .filter(|(t, _)| self.is_hidden(*id, *t))
                .map(|(_, name)| name.clone())
                .chain(others.iter().cloned())
                .collect();
            if !hidden.is_empty() {
                names.insert(project.clone(), hidden);
            }
        }
        for (project, tasks) in &self.others {
            if !self.projects.iter().any(|(_, p, ..)| same_name(p, project)) {
                names.insert(project.clone(), tasks.clone());
            }
        }
        names
    }
}

pub struct ProjectTagsForm {
    pub projects: Vec<(i64, String, String)>,
    pub tags: Vec<(i64, String)>,
    pub current: usize,
    pub chosen: HashMap<i64, BTreeSet<i64>>,
    /// What the page opened with, so only the projects changed are written.
    original: HashMap<i64, BTreeSet<i64>>,
    /// Whether the config names tags for any project; until then every project offers all tags.
    configured: bool,
}

impl ProjectTagsForm {
    pub fn new(session: &Session) -> Self {
        let in_use: BTreeSet<i64> = session.project_tags.values().flatten().map(|t| t.id).collect();
        let by_name = |a: &String, b: &String| a.to_lowercase().cmp(&b.to_lowercase());
        let mut projects: Vec<_> = session
            .projects()
            .iter()
            .filter(|p| p.active || session.project_tags.get(&p.id).is_some_and(|t| !t.is_empty()))
            .map(|p| (p.id, p.name.clone(), p.customer_name.clone()))
            .collect();
        let chosen: HashMap<i64, BTreeSet<i64>> = projects
            .iter()
            .map(|(id, ..)| (*id, session.tags_for(Some(*id)).iter().map(|t| t.id).collect()))
            .collect();
        projects.sort_by(|a, b| by_name(&a.1, &b.1).then_with(|| by_name(&a.2, &b.2)));
        let mut tags: Vec<_> = session
            .tags()
            .iter()
            .filter(|t| !t.name.starts_with("z_") || in_use.contains(&t.id))
            .map(|t| (t.id, t.name.clone()))
            .collect();
        tags.sort_by(|a, b| by_name(&a.1, &b.1));
        Self { projects, tags, current: 0, original: chosen.clone(), chosen, configured: session.tags_configured() }
    }

    pub fn current_project(&self) -> Option<i64> {
        self.projects.get(self.current).map(|p| p.0)
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
        let any = self.projects.iter().any(|(id, ..)| changed(id));
        self.projects
            .iter()
            .filter(|(id, ..)| any && (!self.configured || changed(id)))
            .map(|(id, name, _)| {
                let names = self.tags.iter().filter(|(t, _)| self.is_checked(*id, *t)).map(|(_, n)| n.clone()).collect();
                (name.clone(), names)
            })
            .collect()
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
        assert_eq!(form.projects.iter().map(|p| p.0).collect::<Vec<_>>(), [6, 3], "active ones, by name");
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

        let mut form = TaskVisibility::new(&s);
        assert_eq!(form.projects.iter().map(|p| p.0).collect::<Vec<_>>(), [3], "only projects with active tasks");
        assert_eq!(form.projects[0].3.iter().map(|t| t.0).collect::<Vec<_>>(), [20, 10], "tasks by name");
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

        let mut form = ProjectTagsForm::new(&s);
        assert_eq!(form.projects.iter().map(|p| p.1.as_str()).collect::<Vec<_>>(), ["Other", "P"], "by name, inactive left out");
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
        let mut form = ProjectTagsForm::new(&s);
        assert_eq!(form.count(6), 2, "all tags while none are configured");
        assert!(form.changes().is_empty(), "nothing written unless changed");
        form.toggle(10);
        assert_eq!(form.changes(), [
            ("Other".to_owned(), vec!["A".to_owned()]),
            ("P".to_owned(), vec!["A".to_owned(), "b".to_owned()]),
        ], "the first change writes every project, so the others keep their tags");
    }
}
