//! The Settings dialog: work days and the billable target, kept in `config.toml`.

use gpui::{AppContext, Context, ElementId, Focusable, Window};
use gpuikit::elements::checkbox::checkbox;

use crate::domain::plain_number;
use crate::forms::{self, SettingsForm};
use crate::store::config;
use crate::validate;

use super::TimeChimpApp;

impl TimeChimpApp {
    /// "Settings" in the user menu: opens the dialog with what `config.toml` holds now.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if self.form.is_some() {
            return;
        }
        let current = self.session.settings;
        let work_days = (0..7u8)
            .zip(current.work_days)
            .map(|(i, on)| {
                // "Mon", "Tue", …; Monday is 0, as in `Settings::work_days`.
                let label = chrono::Weekday::try_from(i).expect("0..7 are weekdays").to_string();
                cx.new(|_| checkbox(ElementId::NamedInteger("settings-day".into(), u64::from(i)), on).label(label))
            })
            .collect();
        let billable_goal = forms::text_input(cx, plain_number(current.billable_goal * 100.0));
        let focus = billable_goal.read(cx).focus_handle(cx);
        self.settings = Some(SettingsForm { work_days, billable_goal, error: None });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Save in the dialog: checks the fields, writes `config.toml` and takes the settings in.
    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.settings else { return };
        let saved = validate::settings(&form.values(cx))
            .and_then(|settings| config::path().and_then(|path| config::save_settings(&path, &settings)).map(|_| settings));
        match saved {
            Ok(settings) => {
                self.session.settings = settings;
                self.close_settings(window, cx);
                self.set_info("Settings saved", window, cx);
            }
            Err(e) => {
                if let Some(form) = &mut self.settings {
                    form.error = Some(e);
                }
                cx.notify();
            }
        }
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
