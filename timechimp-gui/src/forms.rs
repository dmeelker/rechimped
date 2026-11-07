//! The add/edit dialogs, built from gpuikit inputs, selects and switches.

use chrono::{Datelike, NaiveDate};
use gpui::{App, AppContext, ElementId, Entity, Focusable, SharedString, Subscription, Window};
use gpuikit::date::Date;
use gpuikit::elements::calendar::Calendar;
use gpuikit::elements::checkbox::{Checkbox, checkbox};
use gpuikit::elements::select::{SelectState, select};
use gpuikit::elements::switch::{Switch, switch};
use gpuikit::input::InputState;
use timechimp::{Mileage, MileageType};

use crate::validate::{MileageValues, TimeKind, TimeValues};

pub struct TimeForm {
    pub kind: TimeKind,
    pub date: DateField,
    pub project: Entity<SelectState<i64>>,
    pub task: Entity<SelectState<i64>>,
    pub start: Entity<InputState>,
    pub end: Entity<InputState>,
    pub hours: Entity<InputState>,
    pub notes: Entity<InputState>,
    /// One checkbox per tag configured for the selected project (`config.toml`).
    pub tags: Vec<TagCheckbox>,
    /// Tags on the entry that the selected project has no checkbox for (e.g. old `z_wbso_*` ones);
    /// kept on save (`Session::kept_tags`).
    pub kept_tag_ids: Vec<i64>,
    /// Picker over recent work, on a new (not copied) entry only.
    pub recent: Option<RecentPicker>,
    /// Follows `project` (task list and tags); replaced together with it.
    pub project_sub: Subscription,
}

/// Filter field plus highlighted row of the Recent list; picking a row fills in project, task,
/// notes and tags.
impl TimeForm {
    /// What is filled in, for `validate::time_entry`.
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
    /// Index into the filtered list.
    pub selected: usize,
}

pub struct TagCheckbox {
    pub tag_id: i64,
    pub checkbox: Entity<Checkbox>,
}

pub struct MileageForm {
    /// The trip being edited (None for a new one); its vehicle and statuses are kept.
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
    /// What From and To suggest: the saved addresses, then this week's (`Session::saved_addresses`).
    pub addresses: Vec<String>,
    /// The suggestion list open under From or To, if any.
    pub address_menu: Option<AddressMenu>,
}

/// The two address fields of the trip form, each a text field with a list of suggestions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddressField {
    From,
    To,
}

/// The suggestion list under an address field.
pub struct AddressMenu {
    pub field: AddressField,
    /// Filtered by the field's text (opened by typing) or all addresses (opened with ↓ or ▾).
    pub filtered: bool,
    /// Index into `MileageForm::address_matches`. None after typing, so Enter keeps the typed text.
    pub highlighted: Option<usize>,
}

impl MileageForm {
    pub fn address_input(&self, field: AddressField) -> &Entity<InputState> {
        match field {
            AddressField::From => &self.from,
            AddressField::To => &self.to,
        }
    }

    /// The address field that has focus, if any.
    pub fn focused_address(&self, window: &Window, cx: &App) -> Option<AddressField> {
        [AddressField::From, AddressField::To]
            .into_iter()
            .find(|f| self.address_input(*f).read(cx).focus_handle(cx).is_focused(window))
    }

    /// The suggestions the open list shows (empty while none is open).
    pub fn address_matches(&self, cx: &App) -> Vec<&str> {
        let Some(menu) = &self.address_menu else { return Vec::new() };
        let query = if menu.filtered { text(self.address_input(menu.field), cx) } else { String::new() };
        crate::domain::matching_addresses(&self.addresses, &query)
    }
    /// What is filled in, for `validate::trip` and `validate::preset`.
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

/// The Date field of both forms: a text field (a date can still be typed) whose calendar icon,
/// or the down arrow, opens a month grid under it.
pub struct DateField {
    pub input: Entity<InputState>,
    /// The calendar while it is open.
    pub picker: Option<DatePicker>,
}

impl DateField {
    pub fn new(cx: &mut App, date: NaiveDate) -> Self {
        Self { input: text_input(cx, date.to_string()), picker: None }
    }

    /// The date typed in, if it is one.
    pub fn parsed(&self, cx: &App) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(&text(&self.input, cx), "%Y-%m-%d").ok()
    }
}

/// An open calendar, with its subscriptions (a day picked, focus leaving it).
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

    pub fn title(&self) -> &'static str {
        match &self.body {
            FormBody::Time(TimeForm { kind: TimeKind::New, .. }) => "New time entry",
            FormBody::Time(TimeForm { kind: TimeKind::Edit { .. }, .. }) => "Edit time entry",
            FormBody::Mileage(MileageForm { editing: None, .. }) => "New trip",
            FormBody::Mileage(MileageForm { editing: Some(_), .. }) => "Edit trip",
        }
    }
}

/// A single-line text input that takes part in Tab navigation.
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

/// A dropdown over `(id, label)` options; falls back to the first option.
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
