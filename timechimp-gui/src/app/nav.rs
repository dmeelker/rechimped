//! Moving around: tabs, days, weeks, months, the selection, and the browse-mode keyboard.

use chrono::{Duration, Months, NaiveDate};
use gpui::{Context, KeyDownEvent, Window};

use crate::domain;

use super::{Overlay, Screen, Tab, TimeChimpApp};

impl TimeChimpApp {
    pub fn switch_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = Tab::ALL[index];
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        self.fx.sweep += 1;
        self.tabs.update(cx, |t, cx| t.set_selected(tab.id().into(), cx));
        if tab == Tab::Report && self.session.report().is_none() {
            self.load_report(window, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Moves the selected day; loads data only when the week changes.
    pub fn shift_day(&mut self, days: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.select_day(self.session.day() + Duration::days(days), window, cx);
    }

    /// The ‹/› buttons and ←/→: a shown day on the Week tab (skipping the days off it hides), a
    /// week on the Mileage tab.
    pub fn step(&mut self, direction: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Week {
            let work_days = self.session.settings.work_days;
            let day = domain::week::step_day(self.session.day(), direction, work_days, domain::time::today());
            self.select_day(day, window, cx);
        } else {
            self.shift_day(7 * direction, window, cx);
        }
    }

    pub fn select_day(&mut self, day: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.select_day(day) {
            self.load_week(None, window, cx);
        }
        cx.notify();
    }

    pub fn go_today(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Report {
            self.session.set_report_month(domain::time::today());
            self.load_report(window, cx);
        } else {
            self.select_day(domain::time::today(), window, cx);
        }
    }

    pub fn shift_month(&mut self, months: i32, window: &mut Window, cx: &mut Context<Self>) {
        let delta = Months::new(months.unsigned_abs());
        let month = self.session.report_month();
        self.session.set_report_month(if months < 0 { month - delta } else { month + delta });
        self.load_report(window, cx);
        cx.notify();
    }

    /// Moves the selection by `delta` rows within what's visible on the current tab.
    fn move_selection(&mut self, delta: isize) {
        if self.tab == Tab::Mileage {
            self.session.move_mileage_selection(delta);
        } else {
            self.session.move_entry_selection(delta);
        }
    }

    /// Clicking a row: select it; a double click opens it for editing.
    pub fn click_entry(&mut self, id: i64, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.session.select_entry(id);
        window.focus(&self.focus, cx);
        if clicks >= 2 {
            self.edit_selected(window, cx);
        }
        cx.notify();
    }

    pub fn click_mileage(&mut self, id: i64, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.session.select_mileage(id);
        window.focus(&self.focus, cx);
        if clicks >= 2 {
            self.edit_selected(window, cx);
        }
        cx.notify();
    }

    pub fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Main) {
            return;
        }
        // Any key closes a menu or a read-only popup; forms and dialogs handle their own keys.
        match self.overlay {
            Overlay::None if self.menu.is_none() => {}
            Overlay::None | Overlay::Detail | Overlay::Chart(_) => {
                self.menu = None;
                self.overlay = Overlay::None;
                cx.stop_propagation();
                cx.notify();
                return;
            }
            Overlay::Form(_) | Overlay::Settings(_) | Overlay::Confirm { .. } => return,
        }
        let ks = &event.keystroke;
        let (shift, ctrl) = (ks.modifiers.shift, ks.modifiers.control || ks.modifiers.platform);
        let key = ks.key.as_str();
        let week_tab = self.tab == Tab::Week;
        match (key, ctrl) {
            ("q", true) => cx.quit(),
            (_, true) => return,
            ("1" | "2" | "3" | "4", _) => self.switch_tab(key.parse::<usize>().expect("digit") - 1, window, cx),
            ("r", _) => self.reload(window, cx),
            ("l", _) if shift => self.logout(window, cx),
            ("left" | "h", _) if self.tab == Tab::Report => self.shift_month(-1, window, cx),
            ("right" | "l", _) if self.tab == Tab::Report => self.shift_month(1, window, cx),
            ("t", _) if self.tab != Tab::Projects => self.go_today(window, cx),
            (_, _) if !matches!(self.tab, Tab::Week | Tab::Mileage) => return,
            // ←/→ move a day on the Week tab, a week on Mileage; Shift+←/→ and [ ] always a week.
            ("left" | "h", _) if week_tab && !shift => self.step(-1, window, cx),
            ("right" | "l", _) if week_tab && !shift => self.step(1, window, cx),
            ("left" | "h" | "[", _) => self.shift_day(-7, window, cx),
            ("right" | "l" | "]", _) => self.shift_day(7, window, cx),
            ("down" | "j", _) => self.move_selection(1),
            ("up" | "k", _) => self.move_selection(-1),
            ("enter", _) if week_tab && self.session.selected_entry().is_some() => {
                self.open_overlay(Overlay::Detail, cx);
            }
            ("a" | "n", _) => self.add(window, cx),
            ("e", _) => self.edit_selected(window, cx),
            ("enter", _) if self.tab == Tab::Mileage => self.edit_selected(window, cx),
            ("c", _) if week_tab => self.copy_selected(window, cx),
            ("d" | "delete", _) => self.ask_delete(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
