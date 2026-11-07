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
        let Some(command) = browse_command(&ks.key, shift, ctrl, self.tab, self.session.selected_entry().is_some()) else {
            return;
        };
        match command {
            Command::Quit => cx.quit(),
            Command::SwitchTab(index) => self.switch_tab(index, window, cx),
            Command::Reload => self.reload(window, cx),
            Command::Logout => self.logout(window, cx),
            Command::ShiftMonth(months) => self.shift_month(months, window, cx),
            Command::Today => self.go_today(window, cx),
            Command::Step(direction) => self.step(direction, window, cx),
            Command::ShiftDay(days) => self.shift_day(days, window, cx),
            Command::MoveSelection(delta) => self.move_selection(delta),
            Command::ShowDetail => {
                self.open_overlay(Overlay::Detail, cx);
            }
            Command::Add => self.add(window, cx),
            Command::Edit => self.edit_selected(window, cx),
            Command::Copy => self.copy_selected(window, cx),
            Command::Delete => self.ask_delete(window, cx),
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// What a key does in browse mode (no form or dialog open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Quit,
    /// Index into `Tab::ALL`.
    SwitchTab(usize),
    Reload,
    Logout,
    ShiftMonth(i32),
    Today,
    /// To the previous (-1) or next (1) shown day.
    Step(i64),
    ShiftDay(i64),
    MoveSelection(isize),
    ShowDetail,
    Add,
    Edit,
    Copy,
    Delete,
}

/// The browse-mode keymap: `key` as gpui names it, on `tab`; `has_entry` says whether a time
/// entry is selected. None: the key does nothing (and goes on to other handlers).
fn browse_command(key: &str, shift: bool, ctrl: bool, tab: Tab, has_entry: bool) -> Option<Command> {
    let week_tab = tab == Tab::Week;
    Some(match (key, ctrl) {
        ("q", true) => Command::Quit,
        (_, true) => return None,
        ("1" | "2" | "3" | "4", _) => Command::SwitchTab(key.parse::<usize>().expect("digit") - 1),
        ("r", _) => Command::Reload,
        ("l", _) if shift => Command::Logout,
        ("left" | "h", _) if tab == Tab::Report => Command::ShiftMonth(-1),
        ("right" | "l", _) if tab == Tab::Report => Command::ShiftMonth(1),
        ("t", _) if tab != Tab::Projects => Command::Today,
        (_, _) if !matches!(tab, Tab::Week | Tab::Mileage) => return None,
        // ←/→ move a day on the Week tab, a week on Mileage; Shift+←/→ and [ ] always a week.
        ("left" | "h", _) if week_tab && !shift => Command::Step(-1),
        ("right" | "l", _) if week_tab && !shift => Command::Step(1),
        ("left" | "h" | "[", _) => Command::ShiftDay(-7),
        ("right" | "l" | "]", _) => Command::ShiftDay(7),
        ("down" | "j", _) => Command::MoveSelection(1),
        ("up" | "k", _) => Command::MoveSelection(-1),
        ("enter", _) if week_tab && has_entry => Command::ShowDetail,
        ("a" | "n", _) => Command::Add,
        ("e", _) => Command::Edit,
        ("enter", _) if tab == Tab::Mileage => Command::Edit,
        ("c", _) if week_tab => Command::Copy,
        ("d" | "delete", _) => Command::Delete,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::Command::*;
    use super::*;

    fn key(key: &str, tab: Tab) -> Option<Command> {
        browse_command(key, false, false, tab, true)
    }

    #[test]
    fn arrows_depend_on_the_tab() {
        assert_eq!(key("left", Tab::Week), Some(Step(-1)), "a shown day on the Week tab");
        assert_eq!(browse_command("left", true, false, Tab::Week, true), Some(ShiftDay(-7)), "Shift: a week");
        assert_eq!(key("]", Tab::Week), Some(ShiftDay(7)));
        assert_eq!(key("right", Tab::Mileage), Some(ShiftDay(7)), "a week on Mileage");
        assert_eq!(key("l", Tab::Report), Some(ShiftMonth(1)), "a month on Report");
        assert_eq!(key("left", Tab::Projects), None);
    }

    #[test]
    fn enter_opens_details_or_edits() {
        assert_eq!(key("enter", Tab::Week), Some(ShowDetail));
        assert_eq!(browse_command("enter", false, false, Tab::Week, false), None, "no entry selected");
        assert_eq!(key("enter", Tab::Mileage), Some(Edit));
    }

    #[test]
    fn modifiers_and_tab_independent_keys() {
        assert_eq!(browse_command("q", false, true, Tab::Projects, false), Some(Quit));
        assert_eq!(browse_command("r", false, true, Tab::Week, true), None, "Ctrl+R isn't R");
        assert_eq!(browse_command("l", true, false, Tab::Report, false), Some(Logout), "Shift+L before the month keys");
        assert_eq!(key("3", Tab::Week), Some(SwitchTab(2)));
        assert_eq!(key("t", Tab::Projects), None);
        assert_eq!(key("c", Tab::Mileage), None, "copy is for time entries");
        assert_eq!(key("d", Tab::Report), None);
    }
}
