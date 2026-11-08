use gpui::{AppContext, Context, ElementId, Focusable, Window};
use gpuikit::elements::checkbox::checkbox;

use crate::domain::plain_number;
use crate::forms::{self, ProjectTagsForm, ProjectVisibility, SettingsForm};
use crate::store::config;
use crate::validate;

use super::{Overlay, TimeChimpApp};

impl TimeChimpApp {
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay_open() {
            return self.close_menu(cx);
        }
        let current = &self.session.settings;
        let work_days = (0..7u8)
            .zip(current.work_days)
            .map(|(i, on)| {
                let label = chrono::Weekday::try_from(i).expect("0..7 are weekdays").to_string();
                cx.new(|_| checkbox(ElementId::NamedInteger("settings-day".into(), u64::from(i)), on).label(label))
            })
            .collect();
        let billable_goal = forms::text_input(cx, plain_number(current.billable_goal * 100.0));
        let focus = billable_goal.read(cx).focus_handle(cx);
        let projects = ProjectVisibility::new(&self.session);
        self.open_overlay(Overlay::Settings(SettingsForm { work_days, billable_goal, projects, error: None }), cx);
        window.focus(&focus, cx);
    }

    pub fn toggle_project_hidden(&mut self, project_id: i64, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.projects.toggle(project_id);
            cx.notify();
        }
    }

    pub fn show_all_projects(&mut self, shown: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.projects.set_all(shown);
            cx.notify();
        }
    }

    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.settings_form() else { return };
        let saved = validate::settings(&form.values(cx))
            .and_then(|settings| config::path().and_then(|path| config::save_settings(&path, &settings)).map(|_| settings));
        match saved {
            Ok(settings) => {
                self.session.settings = settings;
                self.close_overlay(window, cx);
                self.set_info("Settings saved", window, cx);
            }
            Err(e) => {
                if let Some(form) = self.settings_form_mut() {
                    form.error = Some(e);
                }
                cx.notify();
            }
        }
    }
}

impl TimeChimpApp {
    pub fn open_about(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay_open() {
            return self.close_menu(cx);
        }
        self.open_overlay(Overlay::About, cx);
    }

    pub fn open_project_tags(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay_open() {
            return self.close_menu(cx);
        }
        let form = ProjectTagsForm::new(&self.session);
        self.open_overlay(Overlay::ProjectTags(form), cx);
    }

    pub fn show_project_tags_of(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(form) = self.project_tags_form_mut() {
            form.current = index;
            cx.notify();
        }
    }

    pub fn toggle_project_tag(&mut self, tag_id: i64, cx: &mut Context<Self>) {
        if let Some(form) = self.project_tags_form_mut() {
            form.toggle(tag_id);
            cx.notify();
        }
    }

    pub fn set_all_project_tags(&mut self, checked: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.project_tags_form_mut() {
            form.set_all(checked);
            cx.notify();
        }
    }

    pub fn save_project_tags(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.project_tags_form() else { return };
        let changes = form.changes();
        if changes.is_empty() {
            return self.close_overlay(window, cx);
        }
        match config::path().and_then(|path| config::save_project_tags(&path, &changes)) {
            Ok(()) => {
                self.close_overlay(window, cx);
                self.set_info("Project tags saved", window, cx);
                self.load_config(window, cx);
            }
            Err(e) => {
                if let Some(form) = self.project_tags_form_mut() {
                    form.error = Some(e);
                }
                cx.notify();
            }
        }
    }
}
