use gpui::{AppContext, Context, ElementId, Focusable, Window};
use gpuikit::elements::checkbox::checkbox;
use gpuikit::elements::dialog::{DialogClosed, DialogState, dialog};

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
        let projects = ProjectVisibility::new(&self.session);
        let form = SettingsForm {
            page,
            focus: cx.focus_handle(),
            work_days,
            billable_goal,
            tasks: TaskVisibility::new(&self.session, &projects.hidden),
            tags: ProjectTagsForm::new(&self.session, &projects.hidden),
            projects,
            error: None,
            initial: Default::default(),
            discard: None,
        };
        let form = SettingsForm { initial: form.values(cx), ..form };
        self.open_overlay(Overlay::Settings(Box::new(form)), cx);
        self.show_settings_page(page, window, cx);
    }

    /// Focuses the page's first field, or the window itself on pages without one.
    pub fn show_settings_page(&mut self, page: SettingsPage, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.settings_form_mut() else { return };
        form.page = page;
        form.tasks.order(&form.projects.hidden);
        form.tags.order(&form.projects.hidden);
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

    /// Closes the window, asking first when something changed.
    pub fn cancel_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.settings_form() else { return };
        if !form.is_dirty(cx) {
            return self.close_overlay(window, cx);
        }
        let this = cx.weak_entity();
        let state = cx.new(|_| {
            DialogState::new(
                dialog("settings-discard")
                    .confirm("Discard changes?", "Your changes to the settings haven't been saved.")
                    .confirm_label("Discard")
                    .cancel_label("Keep editing")
                    .on_confirm(move |window, cx| {
                        let _ = this.update(cx, |this, cx| this.close_overlay(window, cx));
                    }),
            )
        });
        // Kept editing: back to the page, which had the focus.
        let sub = cx.subscribe_in(&state, window, |this, _, _: &DialogClosed, window, cx| {
            if let Some(form) = this.settings_form_mut() {
                form.discard = None;
                let page = form.page;
                this.show_settings_page(page, window, cx);
            }
        });
        state.update(cx, |d, cx| d.open(window, cx));
        if let Some(form) = self.settings_form_mut() {
            form.discard = Some((state, sub));
        }
        cx.notify();
    }

    /// Saves every page at once; project tags only when changed, then rereads the config for them.
    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.settings_form() else { return };
        if !form.is_dirty(cx) {
            return self.close_overlay(window, cx);
        }
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
