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
        if tab == Tab::Report && self.session.report().is_none() {
            self.load_report(window, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn shift_day(&mut self, days: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.select_day(self.session.day() + Duration::days(days), window, cx);
    }

    pub fn step(&mut self, direction: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Week {
            let work_days = self.session.settings.work_days;
            let day = domain::week::step_day(self.session.day(), direction, work_days, domain::time::today());
            self.select_day(day, window, cx);
        } else {
            self.shift_day(7 * direction, window, cx);
        }
    }

    pub fn step_period(&mut self, direction: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Report {
            self.shift_month(direction as i32, window, cx);
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

    fn move_selection(&mut self, delta: isize) {
        if self.tab == Tab::Travel {
            self.session.move_travel_selection(delta);
        } else {
            self.session.move_entry_selection(delta);
        }
    }

    pub fn click_entry(&mut self, id: i64, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.session.select_entry(id);
        window.focus(&self.focus, cx);
        if clicks >= 2 {
            self.edit_selected(window, cx);
        }
        cx.notify();
    }

    pub fn click_travel(&mut self, id: i64, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.session.select_travel(id);
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
        match self.overlay {
            Overlay::None if self.menu.is_none() => {}
            Overlay::None | Overlay::Detail | Overlay::About | Overlay::Chart(_) => {
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
            Command::Settings => self.open_settings(crate::forms::SettingsPage::General, window, cx),
            Command::Delete => self.ask_delete(window, cx),
        }
        cx.stop_propagation();
        cx.notify();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Quit,
    SwitchTab(usize),
    Reload,
    Logout,
    ShiftMonth(i32),
    Today,
    Step(i64),
    ShiftDay(i64),
    MoveSelection(isize),
    ShowDetail,
    Add,
    Edit,
    Copy,
    Delete,
    Settings,
}

fn browse_command(key: &str, shift: bool, ctrl: bool, tab: Tab, has_entry: bool) -> Option<Command> {
    let week_tab = tab == Tab::Week;
    Some(match (key, ctrl) {
        ("q", true) => Command::Quit,
        (",", true) => Command::Settings,
        (_, true) => return None,
        ("1" | "2" | "3", _) => Command::SwitchTab(key.parse::<usize>().expect("digit") - 1),
        ("r", _) => Command::Reload,
        ("l", _) if shift => Command::Logout,
        ("left" | "h", _) if tab == Tab::Report => Command::ShiftMonth(-1),
        ("right" | "l", _) if tab == Tab::Report => Command::ShiftMonth(1),
        ("t", _) => Command::Today,
        (_, _) if !matches!(tab, Tab::Week | Tab::Travel) => return None,
        ("left" | "h", _) if week_tab && !shift => Command::Step(-1),
        ("right" | "l", _) if week_tab && !shift => Command::Step(1),
        ("left" | "h" | "[", _) => Command::ShiftDay(-7),
        ("right" | "l" | "]", _) => Command::ShiftDay(7),
        ("down" | "j", _) => Command::MoveSelection(1),
        ("up" | "k", _) => Command::MoveSelection(-1),
        ("enter", _) if week_tab && has_entry => Command::ShowDetail,
        ("a" | "n", _) => Command::Add,
        ("e", _) => Command::Edit,
        ("enter", _) if tab == Tab::Travel => Command::Edit,
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
        assert_eq!(key("right", Tab::Travel), Some(ShiftDay(7)), "a week on Travel");
        assert_eq!(key("l", Tab::Report), Some(ShiftMonth(1)), "a month on Report");
    }

    #[test]
    fn enter_opens_details_or_edits() {
        assert_eq!(key("enter", Tab::Week), Some(ShowDetail));
        assert_eq!(browse_command("enter", false, false, Tab::Week, false), None, "no entry selected");
        assert_eq!(key("enter", Tab::Travel), Some(Edit));
    }

    #[test]
    fn modifiers_and_tab_independent_keys() {
        assert_eq!(browse_command("q", false, true, Tab::Report, false), Some(Quit));
        assert_eq!(browse_command(",", false, true, Tab::Travel, false), Some(Settings));
        assert_eq!(browse_command("r", false, true, Tab::Week, true), None, "Ctrl+R isn't R");
        assert_eq!(browse_command("l", true, false, Tab::Report, false), Some(Logout), "Shift+L before the month keys");
        assert_eq!(key("3", Tab::Week), Some(SwitchTab(2)));
        assert_eq!(key("4", Tab::Week), None, "three tabs");
        assert_eq!(key("c", Tab::Travel), None, "copy is for time entries");
        assert_eq!(key("d", Tab::Report), None);
    }
}
