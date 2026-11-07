use std::collections::{BTreeSet, HashMap};

use chrono::{Datelike, NaiveDate};
use gpui::{App, AppContext, ElementId, Entity, Focusable, SharedString, Subscription, Window};
use gpuikit::date::Date;
use gpuikit::elements::calendar::Calendar;
use gpuikit::elements::checkbox::{Checkbox, checkbox};
use gpuikit::elements::select::{SelectState, select};
use gpuikit::elements::switch::{Switch, switch};
use gpuikit::input::InputState;
use timechimp::{Mileage, MileageType};

use crate::domain::same_name;
use crate::session::Session;
use crate::validate::{MileageValues, SettingsValues, TimeKind, TimeValues};

pub struct TimeForm {
    pub kind: TimeKind,
    pub date: DateField,
    pub project: Entity<SelectState<i64>>,
    pub task: Entity<SelectState<i64>>,
    pub start: Entity<InputState>,
    pub end: Entity<InputState>,
    pub hours: Entity<InputState>,
    pub notes: Entity<InputState>,
    pub tags: Vec<TagCheckbox>,
    /// Tags the selected project has no checkbox for; kept so saving doesn't drop them.
    pub kept_tag_ids: Vec<i64>,
    pub recent: Option<RecentPicker>,
    /// Replaced together with `project`, since selects can't change options in place.
    pub project_sub: Subscription,
}

impl TimeForm {
    pub fn values(&self, cx: &App) -> TimeValues {
        TimeValues {
            kind: self.kind.clone(),
            date: text(&self.date.input, cx),
            project_id: chosen(&self.project, cx),
            project_task_id: chosen(&self.task, cx),
            start: text(&self.start, cx),
            end: text(&self.end, cx),
            hours: text(&self.hours, cx),
            notes: text(&self.notes, cx),
            tag_ids: self
                .kept_tag_ids
                .iter()
                .copied()
                .chain(self.tags.iter().filter(|t| t.checkbox.read(cx).is_checked()).map(|t| t.tag_id))
                .collect(),
        }
    }
}

pub struct RecentPicker {
    pub filter: Entity<InputState>,
    pub selected: usize,
}

pub struct TagCheckbox {
    pub tag_id: i64,
    pub checkbox: Entity<Checkbox>,
}

pub struct MileageForm {
    pub editing: Option<Box<Mileage>>,
    pub date: DateField,
    pub project: Entity<SelectState<i64>>,
    pub from: Entity<InputState>,
    pub to: Entity<InputState>,
    pub distance: Entity<InputState>,
    pub return_trip: Entity<Switch>,
    pub mileage_type: Entity<SelectState<MileageType>>,
    pub billable: Entity<Switch>,
    pub notes: Entity<InputState>,
    pub addresses: Vec<String>,
    pub address_menu: Option<AddressMenu>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddressField {
    From,
    To,
}

pub struct AddressMenu {
    pub field: AddressField,
    pub filtered: bool,
    /// None after typing, so Enter keeps the typed text.
    pub highlighted: Option<usize>,
}

impl MileageForm {
    pub fn address_input(&self, field: AddressField) -> &Entity<InputState> {
        match field {
            AddressField::From => &self.from,
            AddressField::To => &self.to,
        }
    }

    pub fn focused_address(&self, window: &Window, cx: &App) -> Option<AddressField> {
        [AddressField::From, AddressField::To]
            .into_iter()
            .find(|f| self.address_input(*f).read(cx).focus_handle(cx).is_focused(window))
    }

    pub fn address_matches(&self, cx: &App) -> Vec<&str> {
        let Some(menu) = &self.address_menu else { return Vec::new() };
        let query = if menu.filtered { text(self.address_input(menu.field), cx) } else { String::new() };
        crate::domain::matching_addresses(&self.addresses, &query)
    }
    pub fn values(&self, cx: &App) -> MileageValues {
        MileageValues {
            editing: self.editing.clone(),
            date: text(&self.date.input, cx),
            project_id: chosen(&self.project, cx),
            from: text(&self.from, cx),
            to: text(&self.to, cx),
            distance: text(&self.distance, cx),
            return_trip: self.return_trip.read(cx).is_on(),
            mileage_type: chosen(&self.mileage_type, cx),
            billable: self.billable.read(cx).is_on(),
            notes: text(&self.notes, cx),
        }
    }
}

pub struct DateField {
    pub input: Entity<InputState>,
    pub picker: Option<DatePicker>,
}

impl DateField {
    pub fn new(cx: &mut App, date: NaiveDate) -> Self {
        Self { input: text_input(cx, date.to_string()), picker: None }
    }

    pub fn parsed(&self, cx: &App) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(&text(&self.input, cx), "%Y-%m-%d").ok()
    }
}

pub struct DatePicker {
    pub calendar: Entity<Calendar>,
    pub _subscriptions: Vec<Subscription>,
}

pub fn to_calendar_date(date: NaiveDate) -> Date {
    Date::new(date.year(), date.month(), date.day()).expect("a NaiveDate is a valid date")
}

pub fn from_calendar_date(date: Date) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(date.year(), date.month(), date.day())
}

pub enum FormBody {
    Time(TimeForm),
    Mileage(MileageForm),
}

pub struct Form {
    /// Lets a request's result tell whether the form it came from is still the one open.
    pub id: u64,
    pub body: FormBody,
    pub error: Option<String>,
    pub _subscriptions: Vec<Subscription>,
}

impl Form {
    pub fn date(&self) -> &DateField {
        match &self.body {
            FormBody::Time(f) => &f.date,
            FormBody::Mileage(f) => &f.date,
        }
    }

    pub fn date_mut(&mut self) -> &mut DateField {
        match &mut self.body {
            FormBody::Time(f) => &mut f.date,
            FormBody::Mileage(f) => &mut f.date,
        }
    }

    pub fn time(&self) -> Option<&TimeForm> {
        match &self.body {
            FormBody::Time(f) => Some(f),
            FormBody::Mileage(_) => None,
        }
    }

    pub fn time_mut(&mut self) -> Option<&mut TimeForm> {
        match &mut self.body {
            FormBody::Time(f) => Some(f),
            FormBody::Mileage(_) => None,
        }
    }

    pub fn trip(&self) -> Option<&MileageForm> {
        match &self.body {
            FormBody::Mileage(f) => Some(f),
            FormBody::Time(_) => None,
        }
    }

    pub fn trip_mut(&mut self) -> Option<&mut MileageForm> {
        match &mut self.body {
            FormBody::Mileage(f) => Some(f),
            FormBody::Time(_) => None,
        }
    }

    pub fn title(&self) -> &'static str {
        match &self.body {
            FormBody::Time(TimeForm { kind: TimeKind::New, .. }) => "New time entry",
            FormBody::Time(TimeForm { kind: TimeKind::Edit { .. }, .. }) => "Edit time entry",
            FormBody::Mileage(MileageForm { editing: None, .. }) => "New trip",
            FormBody::Mileage(MileageForm { editing: Some(_), .. }) => "Edit trip",
        }
    }
}

pub fn text_input(cx: &mut App, value: impl Into<String>) -> Entity<InputState> {
    let value = value.into();
    let input = cx.new(|cx| {
        let mut state = InputState::new_singleline(cx);
        state.set_content(value, cx);
        state
    });
    // gpuikit inputs aren't tab stops by default; the flag lives in the shared focus map.
    let _ = input.read(cx).focus_handle(cx).tab_stop(true);
    input
}

pub fn choice<T: Clone + PartialEq + 'static>(
    cx: &mut App,
    id: &'static str,
    label: &'static str,
    options: Vec<(T, String)>,
    selected: Option<T>,
) -> Entity<SelectState<T>> {
    let selected =
        selected.filter(|s| options.iter().any(|(id, _)| id == s)).or_else(|| options.first().map(|(id, _)| id.clone()));
    let options: Vec<(T, SharedString)> = options.into_iter().map(|(id, l)| (id, l.into())).collect();
    cx.new(|_| {
        let mut builder = select(id, label, options).full_width(true).placeholder("(none available)");
        if let Some(s) = selected {
            builder = builder.selected(s);
        }
        SelectState::new(builder)
    })
}

pub struct SettingsForm {
    pub work_days: Vec<Entity<Checkbox>>,
    pub billable_goal: Entity<InputState>,
    pub projects: ProjectVisibility,
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

pub fn toggle(cx: &mut App, id: &'static str, label: &'static str, on: bool) -> Entity<Switch> {
    cx.new(|_| switch(id, on).label(label))
}

pub fn text(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).content().trim().to_owned()
}

pub fn chosen<T: Clone + PartialEq + 'static>(select: &Entity<SelectState<T>>, cx: &App) -> Option<T> {
    select.read(cx).selected.clone()
}

pub fn set_text(input: &Entity<InputState>, value: impl Into<String>, cx: &mut App) {
    let value = value.into();
    input.update(cx, |s, cx| s.set_content(value, cx));
}

pub fn tag_checkbox(cx: &mut App, tag_id: i64, label: String, checked: bool) -> TagCheckbox {
    let id = ElementId::NamedInteger("form-tag".into(), tag_id as u64);
    TagCheckbox { tag_id, checkbox: cx.new(|_| checkbox(id, checked).label(label)) }
}

pub struct ProjectTagsForm {
    pub projects: Vec<(i64, String, String)>,
    pub tags: Vec<(i64, String)>,
    pub current: usize,
    pub chosen: HashMap<i64, BTreeSet<i64>>,
    /// So only the projects changed are written.
    original: HashMap<i64, BTreeSet<i64>>,
    pub error: Option<String>,
}

impl ProjectTagsForm {
    pub fn new(session: &Session) -> Self {
        let original: HashMap<i64, BTreeSet<i64>> = session
            .project_tags
            .iter()
            .map(|(id, tags)| (*id, tags.iter().map(|t| t.id).collect()))
            .collect();
        let in_use: BTreeSet<i64> = original.values().flatten().copied().collect();
        let by_name = |a: &String, b: &String| a.to_lowercase().cmp(&b.to_lowercase());
        let mut projects: Vec<_> = session
            .projects()
            .iter()
            .filter(|p| p.active || original.get(&p.id).is_some_and(|t| !t.is_empty()))
            .map(|p| (p.id, p.name.clone(), p.customer_name.clone()))
            .collect();
        let chosen = projects
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
        Self { projects, tags, current: 0, original, chosen, error: None }
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

    pub fn changes(&self) -> Vec<(String, Vec<String>)> {
        let none = BTreeSet::new();
        self.projects
            .iter()
            .filter(|(id, ..)| self.chosen.get(id).unwrap_or(&none) != self.original.get(id).unwrap_or(&none))
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
        let form = ProjectTagsForm::new(&s);
        assert_eq!(form.count(6), 2);
        assert_eq!(form.changes(), [
            ("Other".to_owned(), vec!["A".to_owned(), "b".to_owned()]),
            ("P".to_owned(), vec!["A".to_owned(), "b".to_owned()]),
        ]);
    }
}
