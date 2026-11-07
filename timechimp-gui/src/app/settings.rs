//! The Settings dialog: work days and the billable target, kept in `config.toml`.

use gpui::{AppContext, Context, ElementId, Focusable, Window};
use gpuikit::elements::checkbox::checkbox;

use crate::forms::{self, SettingsForm};
use crate::store::config;
use crate::validate;

use super::TimeChimpApp;

/// The day checkboxes' labels, Monday first (as `config::DAY_KEYS`).
const DAY_LABELS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// A number as typed: `40`, `32.5`.
fn plain(value: f64) -> String {
    let s = format!("{value:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl TimeChimpApp {
    /// "Settings" in the user menu: opens the dialog with what `config.toml` holds now.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if self.form.is_some() {
            return;
        }
        let current = self.session.settings;
        let work_days = DAY_LABELS
            .iter()
            .zip(current.work_days)
            .enumerate()
            .map(|(i, (label, on))| {
                cx.new(|_| checkbox(ElementId::NamedInteger("settings-day".into(), i as u64), on).label(*label))
            })
            .collect();
        let billable_goal = forms::text_input(cx, plain(current.billable_goal * 100.0));
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

#[cfg(test)]
mod tests {
    #[test]
    fn plain_numbers() {
        assert_eq!(super::plain(40.0), "40");
        assert_eq!(super::plain(32.5), "32.5");
        assert_eq!(super::plain(0.825 * 100.0), "82.5");
    }
}
