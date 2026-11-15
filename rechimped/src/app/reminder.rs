use gpui::{Context, Window};

use crate::domain;
use crate::store::reminder;

use super::{Overlay, Tab, TimeChimpApp};

impl TimeChimpApp {
    /// After a session's first successful load only, so it comes up at startup (or login) and not
    /// on every reload. "Later" leaves it for the next start; "Not today" stores the day.
    pub(super) fn check_travel_reminder(&mut self, cx: &mut Context<Self>) {
        if self.travel_reminder_checked == Some(self.session.epoch()) {
            return;
        }
        self.travel_reminder_checked = Some(self.session.epoch());
        let today = self.session.today();
        let shows_today = self.session.week().loaded() && self.session.week().week_start() == domain::time::monday_of(today);
        if !self.session.config().settings().travel_reminder || !shows_today {
            return;
        }
        let dismissed = reminder::path().and_then(|path| reminder::dismissed_on(&path));
        if domain::week::travel_reminder_due(
            self.session.week().travel(),
            today,
            self.session.config().settings().work_days,
            dismissed,
        ) {
            self.open_overlay(Overlay::TravelReminder, cx);
        }
    }

    pub fn reminder_go_to_travel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlay(window, cx);
        self.switch_tab(Tab::Travel, window, cx);
        self.select_day(self.session.today(), window, cx);
    }

    pub fn reminder_later(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlay(window, cx);
    }

    pub fn reminder_not_today(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlay(window, cx);
        let saved = reminder::path()
            .ok_or_else(|| "no config directory".to_owned())
            .and_then(|path| reminder::dismiss(&path, self.session.today()).map_err(|e| e.to_string()));
        if let Err(e) = saved {
            self.set_error(format!("Could not save that you didn't travel today: {e}"), window, cx);
        }
    }
}
