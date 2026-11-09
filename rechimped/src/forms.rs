use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate};
use gpui::{App, AppContext, ElementId, Entity, Focusable, SharedString, Subscription, Window};
use gpuikit::date::Date;
use gpuikit::elements::calendar::Calendar;
use gpuikit::elements::checkbox::{Checkbox, checkbox};
use gpuikit::elements::select::{SelectState, select};
use gpuikit::elements::switch::{Switch, switch};
use gpuikit::input::InputState;
use timechimp_api::{Mileage, MileageType};

use crate::validate::{TravelValues, TimeKind, TimeValues};

mod settings;

pub use settings::*;

pub struct TimeForm {
    pub kind: TimeKind,
    pub date: DateField,
    pub project: Choice<i64>,
    pub task: Choice<i64>,
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

pub struct TravelForm {
    pub editing: Option<Box<Mileage>>,
    pub date: DateField,
    pub project: Choice<i64>,
    pub from: Entity<InputState>,
    pub to: Entity<InputState>,
    pub distance: Entity<InputState>,
    pub return_trip: Entity<Switch>,
    pub travel_type: Choice<MileageType>,
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

impl TravelForm {
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
    pub fn values(&self, cx: &App) -> TravelValues {
        TravelValues {
            editing: self.editing.clone(),
            date: text(&self.date.input, cx),
            project_id: chosen(&self.project, cx),
            from: text(&self.from, cx),
            to: text(&self.to, cx),
            distance: text(&self.distance, cx),
            return_trip: self.return_trip.read(cx).is_on(),
            travel_type: chosen(&self.travel_type, cx),
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
    Travel(TravelForm),
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
            FormBody::Travel(f) => &f.date,
        }
    }

    pub fn date_mut(&mut self) -> &mut DateField {
        match &mut self.body {
            FormBody::Time(f) => &mut f.date,
            FormBody::Travel(f) => &mut f.date,
        }
    }

    pub fn time(&self) -> Option<&TimeForm> {
        match &self.body {
            FormBody::Time(f) => Some(f),
            FormBody::Travel(_) => None,
        }
    }

    pub fn time_mut(&mut self) -> Option<&mut TimeForm> {
        match &mut self.body {
            FormBody::Time(f) => Some(f),
            FormBody::Travel(_) => None,
        }
    }

    pub fn trip(&self) -> Option<&TravelForm> {
        match &self.body {
            FormBody::Travel(f) => Some(f),
            FormBody::Time(_) => None,
        }
    }

    pub fn trip_mut(&mut self) -> Option<&mut TravelForm> {
        match &mut self.body {
            FormBody::Travel(f) => Some(f),
            FormBody::Time(_) => None,
        }
    }

    pub fn title(&self) -> &'static str {
        match &self.body {
            FormBody::Time(TimeForm { kind: TimeKind::New, .. }) => "New time entry",
            FormBody::Time(TimeForm { kind: TimeKind::Edit { .. }, .. }) => "Edit time entry",
            FormBody::Travel(TravelForm { editing: None, .. }) => "New trip",
            FormBody::Travel(TravelForm { editing: Some(_), .. }) => "Edit trip",
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
) -> Choice<T> {
    let selected =
        selected.filter(|s| options.iter().any(|(id, _)| id == s)).or_else(|| options.first().map(|(id, _)| id.clone()));
    let labels = Rc::new(options.iter().map(|(id, l)| (id.clone(), l.clone())).collect::<Vec<_>>());
    let options: Vec<(T, SharedString)> = options.into_iter().map(|(id, l)| (id, l.into())).collect();
    let state = cx.new(|_| {
        let mut builder = select(id, label, options).full_width(true).placeholder("(none available)");
        if let Some(s) = selected {
            builder = builder.selected(s);
        }
        SelectState::new(builder)
    });
    Choice { state, labels, typed: Rc::default() }
}

/// A dropdown plus what its keyboard type-ahead needs: the option labels (gpuikit keeps them
/// private) and the characters typed so far.
#[derive(Clone)]
pub struct Choice<T: Clone + PartialEq + 'static> {
    pub state: Entity<SelectState<T>>,
    labels: Rc<Vec<(T, String)>>,
    typed: Rc<RefCell<TypeAhead>>,
}

impl<T: Clone + PartialEq + 'static> Choice<T> {
    /// The first option starting with what was typed; see `TypeAhead::find`.
    pub fn type_ahead(&self, typed: &str) -> Option<T> {
        let index = self.typed.borrow_mut().find(typed, self.labels.iter().map(|(_, l)| l.as_str()))?;
        Some(self.labels[index].0.clone())
    }
}

/// Typing pauses longer than this start a new prefix.
const TYPE_AHEAD_RESET: Duration = Duration::from_millis(1000);

/// The characters typed so far for a keyboard type-ahead.
pub struct TypeAhead {
    prefix: String,
    last: Instant,
}

impl Default for TypeAhead {
    fn default() -> Self {
        Self { prefix: String::new(), last: Instant::now() }
    }
}

impl TypeAhead {
    /// Adds `typed` to the prefix typed so far and returns the index of the first label starting
    /// with it, ignoring case. A prefix nothing matches is dropped, so the next key starts over.
    pub fn find<'a>(&mut self, typed: &str, labels: impl IntoIterator<Item = &'a str>) -> Option<usize> {
        if self.last.elapsed() > TYPE_AHEAD_RESET {
            self.prefix.clear();
        }
        self.last = Instant::now();
        self.prefix.push_str(&typed.to_lowercase());
        let found = labels.into_iter().position(|l| l.to_lowercase().starts_with(self.prefix.as_str()));
        if found.is_none() {
            self.prefix.clear();
        }
        found
    }
}

pub fn toggle(cx: &mut App, id: &'static str, label: &'static str, on: bool) -> Entity<Switch> {
    cx.new(|_| switch(id, on).label(label))
}

pub fn text(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).content().trim().to_owned()
}

pub fn chosen<T: Clone + PartialEq + 'static>(select: &Choice<T>, cx: &App) -> Option<T> {
    select.state.read(cx).selected.clone()
}

pub fn set_text(input: &Entity<InputState>, value: impl Into<String>, cx: &mut App) {
    let value = value.into();
    input.update(cx, |s, cx| s.set_content(value, cx));
}

pub fn tag_checkbox(cx: &mut App, tag_id: i64, label: String, checked: bool) -> TagCheckbox {
    let id = ElementId::NamedInteger("form-tag".into(), tag_id as u64);
    TagCheckbox { tag_id, checkbox: cx.new(|_| checkbox(id, checked).label(label)) }
}
