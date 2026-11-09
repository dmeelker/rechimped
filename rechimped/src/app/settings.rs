use gpui::{AppContext, Context, ElementId, Focusable, Window};
use gpuikit::elements::checkbox::checkbox;

use crate::domain::plain_number;
use crate::forms::{self, ProjectTagsForm, ProjectVisibility, SettingsForm, SettingsPage, TaskVisibility};
use crate::store::config;
use crate::validate;

use super::{Overlay, TimeChimpApp};

impl TimeChimpApp {
    pub fn open_settings(&mut self, page: SettingsPage, window: &mut Window, cx: &mut Context<Self>) {
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
        let form = SettingsForm {
            page,
            focus: cx.focus_handle(),
            work_days,
            billable_goal,
            projects: ProjectVisibility::new(&self.session),
            tasks: TaskVisibility::new(&self.session),
            tags: ProjectTagsForm::new(&self.session),
            error: None,
        };
        self.open_overlay(Overlay::Settings(Box::new(form)), cx);
        self.show_settings_page(page, window, cx);
    }

    /// Focuses the page's first field, or the window itself on pages without one.
    pub fn show_settings_page(&mut self, page: SettingsPage, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.settings_form_mut() else { return };
        form.page = page;
        let focus = match page {
            SettingsPage::General => form.billable_goal.read(cx).focus_handle(cx),
            SettingsPage::Projects | SettingsPage::Tasks | SettingsPage::ProjectTags => form.focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
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

    pub fn show_tasks_of(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.tasks.current = index;
            cx.notify();
        }
    }

    pub fn toggle_task_hidden(&mut self, task_id: i64, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.tasks.toggle(task_id);
            cx.notify();
        }
    }

    pub fn show_all_tasks(&mut self, shown: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.tasks.set_all(shown);
            cx.notify();
        }
    }

    pub fn show_project_tags_of(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.tags.current = index;
            cx.notify();
        }
    }

    pub fn toggle_project_tag(&mut self, tag_id: i64, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.tags.toggle(tag_id);
            cx.notify();
        }
    }

    pub fn set_all_project_tags(&mut self, checked: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.settings_form_mut() {
            form.tags.set_all(checked);
            cx.notify();
        }
    }

    /// Saves every page at once; project tags only when changed, then rereads the config for them.
    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.settings_form() else { return };
        let settings = match validate::settings(&form.values(cx)) {
            Ok(settings) => settings,
            // Only the General page has fields that can be invalid.
            Err(e) => return self.settings_failed(e, Some(SettingsPage::General), window, cx),
        };
        let tag_changes = form.tags.changes();
        let saved = config::path().and_then(|path| {
            config::save_settings(&path, &settings)?;
            if !tag_changes.is_empty() {
                config::save_project_tags(&path, &tag_changes)?;
            }
            Ok(())
        });
        match saved {
            Ok(()) => {
                self.session.settings = settings;
                self.close_overlay(window, cx);
                self.set_info("Settings saved", window, cx);
                if !tag_changes.is_empty() {
                    self.load_config(window, cx);
                }
            }
            Err(e) => self.settings_failed(e, None, window, cx),
        }
    }

    fn settings_failed(&mut self, error: String, page: Option<SettingsPage>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = page {
            self.show_settings_page(page, window, cx);
        }
        if let Some(form) = self.settings_form_mut() {
            form.error = Some(error);
        }
        cx.notify();
    }
}

impl TimeChimpApp {
    pub fn open_about(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay_open() {
            return self.close_menu(cx);
        }
        self.open_overlay(Overlay::About, cx);
    }
}
