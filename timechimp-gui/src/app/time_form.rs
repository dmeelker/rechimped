//! The time form: opening it (new, edit, copy, preset), the Recent picker, and following the project.

use gpui::{Context, Entity, Focusable, Subscription, Window};
use gpuikit::elements::select::SelectChanged;
use gpuikit::input::InputStateEvent;
use timechimp::TimeEntry;

use crate::forms::{self, Form, FormBody, TimeForm, chosen, text};
use crate::validate::TimeKind;
use crate::domain;

use super::{Tab, TimeChimpApp};

impl TimeChimpApp {
    pub fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.me.is_none() {
            return;
        }
        if self.tab == Tab::Mileage {
            self.open_mileage_form(None, None, window, cx);
        } else {
            self.open_time_form(None, false, window, cx);
        }
    }

    pub fn edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Mileage {
            if let Some(trip) = self.session.selected_mileage().cloned() {
                self.open_mileage_form(Some(Box::new(trip)), None, window, cx);
            }
        } else if let Some(entry) = self.session.selected_entry().cloned() {
            self.open_time_form(Some(&entry), false, window, cx);
        }
    }

    /// Copies the selected entry as a new one on the selected day.
    pub fn copy_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.session.selected_entry().cloned() {
            self.open_time_form(Some(&entry), true, window, cx);
        }
    }

    /// Form for a new time entry, for editing `entry`, or (with `copy`) for a copy of it.
    fn open_time_form(&mut self, entry: Option<&TimeEntry>, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        let projects = self.session.project_options(entry.map(|e| (e.project_id, &*e.project_name, &*e.customer_name)));
        // New entries default to the project/task/tag of the most recently modified entry.
        let template = entry.or_else(|| self.session.entries.iter().max_by_key(|e| e.modified));
        let project_id = template.map(|e| e.project_id).or(projects.first().map(|(id, _)| *id));
        let tasks = self.session.task_options(project_id, entry);

        let tags = self.tag_checkboxes(project_id, template.map_or(&[], |e| &e.tag_ids), cx);
        let kept_tag_ids = entry.map(|e| self.session.kept_tags(project_id, &e.tag_ids)).unwrap_or_default();

        let editing = entry.filter(|_| !copy);
        let date = editing.map(|e| e.date).unwrap_or(self.session.day);
        let (start, end) = editing
            .and_then(|e| e.start_end.as_deref())
            .and_then(|s| s.split_once('-'))
            .map(|(s, e)| (s.to_owned(), e.to_owned()))
            .unwrap_or_else(|| (self.session.next_start_time(date).unwrap_or_default(), String::new()));
        let hours = editing.map(|e| format!("{}", e.hours)).unwrap_or_default();
        let notes = entry.and_then(|e| e.notes.clone()).unwrap_or_default();

        let project = forms::choice(cx, "form-project", "Project", projects, project_id);
        let project_sub = self.subscribe_project(&project, window, cx);
        let recent = (entry.is_none() && !self.session.recent.is_empty())
            .then(|| forms::RecentPicker { filter: forms::text_input(cx, ""), selected: 0 });
        let form = TimeForm {
            kind: match editing {
                None => TimeKind::New,
                Some(e) => TimeKind::Edit {
                    id: e.id,
                    pause: e.pause,
                    timezone: e.timezone.clone(),
                    task: (e.project_task_id, e.task_id),
                },
            },
            date: forms::DateField::new(cx, date),
            task: forms::choice(cx, "form-task", "Task", tasks, template.map(|e| e.project_task_id)),
            start: forms::text_input(cx, start.clone()),
            end: forms::text_input(cx, end),
            hours: forms::text_input(cx, hours),
            notes: forms::text_input(cx, notes),
            tags,
            kept_tag_ids,
            project,
            recent,
            project_sub,
        };
        // A new entry starts in the Recent filter; editing jumps to the times, a copy to the end
        // time when the start is already filled in.
        let focus = match &form.recent {
            Some(picker) => &picker.filter,
            None if editing.is_some() || start.is_empty() => &form.start,
            None => &form.end,
        };
        window.focus(&focus.read(cx).focus_handle(cx), cx);
        let mut subscriptions = Vec::new();
        // `+15` / `+1h` / `+1h30` in End becomes start + that duration when leaving the field.
        subscriptions.push(cx.subscribe_in(&form.end, window, |this, end, event: &InputStateEvent, _, cx| {
            let Some(form) = this.time_form() else { return };
            if !matches!(event, InputStateEvent::Blur) {
                return;
            }
            if let Some(expanded) = domain::time::expand_end(&text(&form.start, cx), &text(end, cx)) {
                forms::set_text(end, expanded, cx);
            }
        }));
        if let Some(picker) = &form.recent {
            // A new filter text highlights the first match again.
            subscriptions.push(cx.subscribe_in(&picker.filter, window, |this, _, event: &InputStateEvent, _, cx| {
                if matches!(event, InputStateEvent::TextChanged) {
                    if let Some(picker) = this.recent_picker_mut() {
                        picker.selected = 0;
                    }
                    cx.notify();
                }
            }));
        }
        self.form = Some(Form { body: FormBody::Time(form), error: None, _subscriptions: subscriptions });
        cx.notify();
    }

    fn subscribe_project(
        &self,
        project: &Entity<gpuikit::elements::select::SelectState<i64>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(project, window, |this, _, _: &SelectChanged, window, cx| this.on_project_changed(window, cx))
    }

    fn recent_picker_mut(&mut self) -> Option<&mut forms::RecentPicker> {
        self.time_form_mut()?.recent.as_mut()
    }

    /// The recent items matching the filter of the open time form, best first, at most `RECENT_ROWS`.
    /// None while the filter is empty: the list only appears once you type.
    pub fn recent_matches(&self, cx: &gpui::App) -> Vec<&domain::week::RecentItem> {
        let Some(picker) = self.time_form().and_then(|f| f.recent.as_ref()) else { return Vec::new() };
        let query = text(&picker.filter, cx);
        if query.is_empty() {
            return Vec::new();
        }
        self.session.recent.iter().filter(|item| item.matches(&query)).take(RECENT_ROWS).collect()
    }

    /// ↑/↓ in the Recent filter.
    pub fn move_recent(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.recent_matches(cx).len();
        if let Some(picker) = self.recent_picker_mut() {
            picker.selected = picker.selected.saturating_add_signed(delta).min(count.saturating_sub(1));
        }
        cx.notify();
    }

    /// Enter in the Recent filter: fills in the highlighted item, or (without a match) moves on.
    pub fn pick_highlighted_recent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.time_form().and_then(|f| f.recent.as_ref()).map(|p| p.selected) else { return };
        match self.recent_matches(cx).get(selected).map(|item| item.entry.clone()) {
            Some(entry) => self.pick_recent(entry, window, cx),
            None => window.focus_next(cx),
        }
    }

    /// Fills the open time form with the project, task, notes and tags of `entry` (a recent item);
    /// date and times stay. Focus moves on to the times.
    pub fn pick_recent(&mut self, entry: TimeEntry, window: &mut Window, cx: &mut Context<Self>) {
        let fill = TimeFill {
            project_id: entry.project_id,
            project_task_id: Some(entry.project_task_id),
            notes: entry.notes.clone().unwrap_or_default(),
            tag_ids: entry.tag_ids.clone(),
        };
        self.fill_time_form(fill, Some(&entry), window, cx);
    }

    /// Fills the open time form with `fill` (`entry`: where it comes from, so its project and
    /// task are offered even when no longer active); date and times stay. Focus moves on to the
    /// times.
    fn fill_time_form(&mut self, fill: TimeFill, entry: Option<&TimeEntry>, window: &mut Window, cx: &mut Context<Self>) {
        let projects = self.session.project_options(entry.map(|e| (e.project_id, &*e.project_name, &*e.customer_name)));
        let project = forms::choice(cx, "form-project", "Project", projects, Some(fill.project_id));
        let tasks = self.session.task_options(Some(fill.project_id), entry);
        let task = forms::choice(cx, "form-task", "Task", tasks, fill.project_task_id);
        let tags = self.tag_checkboxes(Some(fill.project_id), &fill.tag_ids, cx);
        let kept_tag_ids = self.session.kept_tags(Some(fill.project_id), &fill.tag_ids);
        let project_sub = self.subscribe_project(&project, window, cx);
        let Some(Form { body: FormBody::Time(form), error, .. }) = &mut self.form else { return };
        forms::set_text(&form.notes, fill.notes, cx);
        form.project = project;
        form.task = task;
        form.tags = tags;
        form.kept_tag_ids = kept_tag_ids;
        form.project_sub = project_sub;
        *error = None;
        let next = if text(&form.start, cx).is_empty() { &form.start } else { &form.end };
        window.focus(&next.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// The task list and the tag checkboxes follow the project.
    fn on_project_changed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.time_form() else { return };
        let project_id = chosen(&form.project, cx);
        let options = self.session.task_options(project_id, None);
        let task = forms::choice(cx, "form-task", "Task", options, None);
        // Pre-check the tags the project's most recently modified entry had, and the kept tags
        // this project has a checkbox for; the rest stays kept.
        let last = self.session.entries.iter().filter(|e| Some(e.project_id) == project_id).max_by_key(|e| e.modified);
        let mut checked: Vec<i64> = last.map_or(&[][..], |e| &e.tag_ids).to_vec();
        checked.extend(&form.kept_tag_ids);
        let kept_tag_ids = self.session.kept_tags(project_id, &form.kept_tag_ids);
        let tags = self.tag_checkboxes(project_id, &checked, cx);
        if let Some(form) = self.time_form_mut() {
            form.task = task;
            form.tags = tags;
            form.kept_tag_ids = kept_tag_ids;
        }
        cx.notify();
    }

    /// Checkboxes for the tags configured for `project_id`, checked where `checked` has the tag.
    fn tag_checkboxes(&self, project_id: Option<i64>, checked: &[i64], cx: &mut Context<Self>) -> Vec<forms::TagCheckbox> {
        let tags = project_id.and_then(|id| self.session.project_tags.get(&id)).cloned().unwrap_or_default();
        tags.into_iter().map(|t| forms::tag_checkbox(cx, t.id, t.name, checked.contains(&t.id))).collect()
    }

    /// A time preset in the "+ Add time" dropdown: a new time entry on the selected day, with the
    /// project, task, notes and tags of preset `index`.
    pub fn open_time_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.me.is_none() || self.form.is_some() {
            return;
        }
        self.menu = None;
        let Some(preset) = self.session.time_presets.get(index).cloned() else { return };
        let resolved = match self.session.resolve_time_preset(&preset) {
            Ok(resolved) => resolved,
            Err(unknown) => return self.set_error(format!("Preset: unknown {unknown}"), window, cx),
        };
        self.open_time_form(None, false, window, cx);
        let fill = TimeFill {
            project_id: resolved.project_id,
            project_task_id: resolved.project_task_id,
            notes: preset.notes.unwrap_or_default(),
            tag_ids: resolved.tag_ids,
        };
        self.fill_time_form(fill, None, window, cx);
    }
}

/// What `fill_time_form` puts in the open time form.
struct TimeFill {
    project_id: i64,
    project_task_id: Option<i64>,
    notes: String,
    tag_ids: Vec<i64>,
}

/// Rows the Recent picker shows at most.
const RECENT_ROWS: usize = 6;
