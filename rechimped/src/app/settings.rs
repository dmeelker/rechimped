use gpui::{AppContext, Context, ElementId, Focusable, KeyDownEvent, Window};
use gpuikit::elements::checkbox::checkbox;
use gpuikit::elements::dialog::{DialogClosed, DialogState, dialog};

use crate::domain::plain_number;
use crate::forms::{self, Column, PresetsForm, ProjectTagsForm, ProjectVisibility, SettingsForm, SettingsPage, TaskVisibility};
use crate::store::config;
use crate::validate;

use super::{Overlay, TimeChimpApp};

impl TimeChimpApp {
    /// Opens on the page it was last on.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = self.settings_page;
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
        let trips = self.session.trip_presets.iter().map(|t| (t.clone(), forms::text_input(cx, t.name.clone()))).collect();
        let presets = PresetsForm::new(trips, self.session.time_presets.clone());
        let form = SettingsForm {
            page,
            focus: cx.focus_handle(),
            work_days,
            billable_goal,
            tasks: TaskVisibility::new(&self.session, &projects.hidden),
            tags: ProjectTagsForm::new(&self.session, &projects.hidden),
            projects,
            presets,
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
        self.settings_page = page;
        let Some(form) = self.settings_form_mut() else { return };
        form.page = page;
        form.tasks.order(&form.projects.hidden);
        form.tags.order(&form.projects.hidden);
        let focus = match page {
            SettingsPage::General => form.billable_goal.read(cx).focus_handle(cx),
            SettingsPage::Presets => match form.presets.trips.first() {
                Some((_, name)) => name.read(cx).focus_handle(cx),
                None => form.focus.clone(),
            },
            SettingsPage::Projects | SettingsPage::Tasks | SettingsPage::ProjectTags => form.focus.clone(),
        };
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Applies a change to the open Settings window from a click, and gives the window the focus
    /// back so its keys keep working after a button took it.
    pub fn edit_settings(&mut self, window: &mut Window, cx: &mut Context<Self>, change: impl FnOnce(&mut SettingsForm)) {
        let Some(form) = self.settings_form_mut() else { return };
        change(form);
        let focus = form.focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Ctrl+Tab and Ctrl+1… switch pages from anywhere in the window; the list keys and Enter
    /// only work on the window itself, since in a field or on a control they are that one's.
    pub fn on_settings_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        if m.alt || m.platform || m.function {
            return;
        }
        let Some(form) = self.settings_form_mut() else { return };
        let on_window = form.focus.is_focused(window);
        let Some(command) = settings_key(&keystroke.key, m.control, m.shift) else {
            // Letters jump to the first row of the active list starting with what was typed.
            let typed = keystroke.key_char.as_deref().filter(|c| c.chars().all(char::is_alphanumeric));
            if let Some(typed) = typed.filter(|_| on_window && !m.control) {
                match form.page {
                    SettingsPage::General | SettingsPage::Presets => return,
                    SettingsPage::Projects => form.projects.type_ahead(typed),
                    SettingsPage::Tasks => form.tasks.type_ahead(typed),
                    SettingsPage::ProjectTags => form.tags.type_ahead(typed),
                }
                cx.stop_propagation();
                cx.notify();
            }
            return;
        };
        let page = |index: usize| SettingsPage::ALL[index % SettingsPage::ALL.len()];
        let current = SettingsPage::ALL.iter().position(|p| *p == form.page).unwrap_or(0);
        match command {
            SettingsKey::Page(delta) => {
                let len = SettingsPage::ALL.len() as isize;
                return self.show_settings_page(page((current as isize + delta).rem_euclid(len) as usize), window, cx);
            }
            SettingsKey::PageTo(index) if index < SettingsPage::ALL.len() => return self.show_settings_page(page(index), window, cx),
            SettingsKey::PageTo(_) => return,
            _ if !on_window => return,
            SettingsKey::Save => return self.save_settings(window, cx),
            SettingsKey::Move(delta) => match form.page {
                SettingsPage::General | SettingsPage::Presets => return,
                SettingsPage::Projects => form.projects.move_cursor(delta),
                SettingsPage::Tasks => form.tasks.move_cursor(delta),
                SettingsPage::ProjectTags => form.tags.move_cursor(delta),
            },
            SettingsKey::List(column) => match form.page {
                SettingsPage::Tasks => form.tasks.nav.column = column,
                SettingsPage::ProjectTags => form.tags.nav.column = column,
                SettingsPage::General | SettingsPage::Projects | SettingsPage::Presets => return,
            },
            SettingsKey::Toggle => match form.page {
                SettingsPage::General | SettingsPage::Presets => return,
                SettingsPage::Projects => form.projects.toggle_at(None),
                SettingsPage::Tasks => form.tasks.toggle_at(None),
                SettingsPage::ProjectTags => form.tags.toggle_at(None),
            },
        }
        cx.stop_propagation();
        cx.notify();
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

    /// Saves every page at once; project tags and presets only when changed, then rereads the
    /// config for them.
    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.settings_form() else { return };
        if !form.is_dirty(cx) {
            return self.close_overlay(window, cx);
        }
        let settings = match validate::settings(&form.values(cx)) {
            Ok(settings) => settings,
            Err(e) => return self.settings_failed(e, Some(SettingsPage::General), window, cx),
        };
        let trips = form.presets.trip_names(cx);
        let names: Vec<String> = trips.iter().map(|(_, name)| name.clone()).collect();
        if let Err(e) = validate::trip_names(&names) {
            return self.settings_failed(e, Some(SettingsPage::Presets), window, cx);
        }
        let presets_changed = form.presets.is_changed(cx);
        let times = form.presets.times.clone();
        let tag_changes = form.tags.changes();
        let saved = config::path().and_then(|path| {
            config::save_settings(&path, &settings)?;
            if !tag_changes.is_empty() {
                config::save_project_tags(&path, &tag_changes)?;
            }
            if presets_changed {
                config::save_presets(&path, &trips, &times)?;
            }
            Ok(())
        });
        match saved {
            Ok(()) => {
                self.session.settings = settings;
                self.close_overlay(window, cx);
                self.set_info("Settings saved", window, cx);
                if !tag_changes.is_empty() || presets_changed {
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SettingsKey {
    Page(isize),
    PageTo(usize),
    Move(isize),
    List(Column),
    Toggle,
    Save,
}

fn settings_key(key: &str, ctrl: bool, shift: bool) -> Option<SettingsKey> {
    use SettingsKey::*;
    Some(match (key, ctrl) {
        ("tab", true) => Page(if shift { -1 } else { 1 }),
        ("pageup", true) => Page(-1),
        ("pagedown", true) => Page(1),
        (_, true) => match key.parse::<usize>() {
            Ok(n @ 1..=9) => PageTo(n - 1),
            _ => return None,
        },
        ("up", _) => Move(-1),
        ("down", _) => Move(1),
        ("pageup", _) => Move(-10),
        ("pagedown", _) => Move(10),
        ("home", _) => Move(isize::MIN),
        ("end", _) => Move(isize::MAX),
        ("left", _) => List(crate::forms::Column::Projects),
        ("right", _) => List(crate::forms::Column::Items),
        ("space", _) => Toggle,
        ("enter", _) if !shift => Save,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::SettingsKey::*;
    use super::*;

    #[test]
    fn keymap() {
        assert_eq!(settings_key("tab", true, false), Some(Page(1)));
        assert_eq!(settings_key("tab", true, true), Some(Page(-1)));
        assert_eq!(settings_key("3", true, false), Some(PageTo(2)));
        assert_eq!(settings_key("0", true, false), None);
        assert_eq!(settings_key("tab", false, false), None, "plain Tab moves the focus");
        assert_eq!(settings_key("down", false, false), Some(Move(1)));
        assert_eq!(settings_key("j", false, false), None, "letters are for type-ahead");
        assert_eq!(settings_key("right", false, false), Some(List(forms::Column::Items)));
        assert_eq!(settings_key("space", false, false), Some(Toggle));
        assert_eq!(settings_key("enter", false, false), Some(Save));
        assert_eq!(settings_key("a", true, false), None);
    }
}
