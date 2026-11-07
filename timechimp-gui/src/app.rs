//! Application state, keyboard handling and the (async) actions. Rendering lives in `view/`.

use std::future::Future;

use futures::FutureExt;

use chrono::{Duration, Months, NaiveDate};
use gpui::{
    AppContext, Context, Entity, FocusHandle, Focusable, KeyDownEvent, SharedString, Subscription, Window, actions,
};
use gpuikit::date::Weekday;
use gpuikit::elements::calendar::{Calendar, CalendarEvent};
use gpuikit::elements::dialog::{DialogClosed, DialogState, dialog};
use gpuikit::elements::select::SelectChanged;
use gpuikit::elements::tabs::{TabChanged, Tabs, tab, tabs};
use gpuikit::elements::toast::ToastExt;
use gpuikit::input::{InputState, InputStateEvent};
use timechimp::{
    Authenticator, Client, LoginOutcome, MfaChallenge, Mileage, NewMileage, TimeEntry,
};

use crate::forms::{self, AddressField, AddressMenu, Form, FormBody, MileageForm, TimeForm, chosen, text};
use crate::validate::{self, SaveRequest, TimeKind};
use crate::domain::report::Report;
use crate::domain::{self, MILEAGE_TYPES};
use crate::net;
use crate::session::{self, Session};
use crate::store::{config, token};
use crate::password::{PasswordEvent, PasswordField};

actions!(
    timechimp,
    [
        /// Enter in a form or the login screen.
        Confirm,
        /// Escape in a form.
        Cancel,
        /// Ctrl+D in the mileage form.
        CalculateDistance,
        /// ↑ in an address field of the trip form: previous suggestion (opens the list).
        AddressUp,
        /// ↓ in an address field of the trip form: next suggestion (opens the list).
        AddressDown,
        /// Enter in an address field: take the highlighted suggestion.
        PickAddress,
        /// Escape in an address field: close the suggestions.
        CloseAddresses,
        /// ← in a form's Date field with the calendar open: the day before.
        PreviousDay,
        /// → in the Date field with the calendar open: the day after.
        NextDay,
        /// ↑ in the Date field with the calendar open: a week earlier.
        PreviousWeek,
        /// ↓ in the Date field: opens the calendar, then a week later.
        NextWeek,
        /// PageUp in the Date field with the calendar open: a month earlier.
        PreviousMonth,
        /// PageDown in the Date field with the calendar open: a month later.
        NextMonth,
        /// Enter or Escape in the Date field: close the calendar.
        CloseCalendar,
        /// ↑ in the time form's Recent filter.
        RecentUp,
        /// ↓ in the time form's Recent filter.
        RecentDown,
        /// Enter in the time form's Recent filter: fill in the highlighted item.
        PickRecent,
        /// F12: showcase mode (aurora behind the app, frame-time HUD).
        ToggleShowcase,
    ]
);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Week,
    Mileage,
    Report,
    Projects,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Week, Tab::Mileage, Tab::Report, Tab::Projects];

    pub fn id(self) -> &'static str {
        match self {
            Tab::Week => "week",
            Tab::Mileage => "mileage",
            Tab::Report => "report",
            Tab::Projects => "projects",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Week => "Week",
            Tab::Mileage => "Mileage",
            Tab::Report => "Report",
            Tab::Projects => "Projects",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).expect("tab in ALL")
    }
}

pub struct Login {
    pub email: Entity<InputState>,
    pub password: Entity<PasswordField>,
    pub code: Entity<InputState>,
    pub mfa: Option<MfaChallenge>,
    pub error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

pub enum Screen {
    Login(Box<Login>),
    Main,
}

/// A dropdown open on the main screen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// Under the user name in the header (Log out).
    User,
    /// Under the ▾ next to "+ Add trip" (the trip presets).
    AddTrip,
    /// Under the ▾ next to "+ Add time" (the time presets).
    AddTime,
}

pub struct TimeChimpApp {
    /// Focus target of the main (browse) view; its key handler drives the keyboard shortcuts.
    pub focus: FocusHandle,
    pub screen: Screen,
    /// The logged-in user's data; replaced as a whole on logout.
    pub session: Session,
    pub tab: Tab,
    pub tabs: Entity<Tabs>,
    pub form: Option<Form>,
    /// The open delete confirmation, with the subscription that clears it when it closes.
    pub confirm: Option<(Entity<DialogState>, Subscription)>,
    pub show_detail: bool,
    /// The open dropdown, if any.
    pub menu: Option<Menu>,
    pub error: Option<String>,
    /// Labels of the tasks `run` has in flight, oldest first, with the session they belong to.
    in_flight: Vec<(u64, &'static str)>,
    /// GPU effects state (`fx.rs`).
    pub fx: crate::fx::Fx,
    _subscriptions: Vec<Subscription>,
}

impl TimeChimpApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tabs = cx.new(|_| {
            Tab::ALL.iter().fold(tabs("main-tabs"), |t, tb| t.tab(tab(tb.id(), tb.label()))).selected(Tab::Week.id())
        });
        let tab_sub = cx.subscribe_in(&tabs, window, |this, _, event: &TabChanged, window, cx| {
            if let Some(t) = Tab::ALL.iter().find(|t| t.id() == event.tab_id.as_ref()) {
                this.switch_tab(t.index(), window, cx);
            }
        });
        let mut app = Self {
            focus: cx.focus_handle(),
            screen: Screen::Main,
            session: Session::new(None, domain::time::today()),
            tab: Tab::Week,
            tabs,
            form: None,
            confirm: None,
            show_detail: false,
            menu: None,
            error: None,
            in_flight: Vec::new(),
            fx: crate::fx::Fx::new(),
            _subscriptions: vec![tab_sub],
        };

        // Continue with a stored refresh token, if any; otherwise show the login screen.
        match token::load_token() {
            Some(token) => {
                app.session.client = Some(Client::new(Authenticator::from_refresh_token(token)));
                app.reload(window, cx);
                window.focus(&app.focus, cx);
            }
            None => app.show_login(None, window, cx),
        }
        app
    }

    // ---- Async plumbing -------------------------------------------------------------------

    /// Runs `future` on the tokio runtime, shows `label` meanwhile, then applies the result,
    /// unless the session changed meanwhile (logged out): then the result is dropped.
    fn run<T: Send + 'static>(
        &mut self,
        label: &'static str,
        future: impl Future<Output = T> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let epoch = self.session.epoch;
        self.in_flight.push((epoch, label));
        cx.notify();
        let handle = net::runtime().spawn(future);
        cx.spawn_in(window, async move |this, cx| {
            let result = handle.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(i) = this.in_flight.iter().rposition(|t| *t == (epoch, label)) {
                    this.in_flight.remove(i);
                }
                match result {
                    Ok(_) if this.session.epoch != epoch => {}
                    Ok(value) => done(this, value, window, cx),
                    Err(e) => this.set_error(format!("Background task failed: {e}"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What is being waited for (the most recent task of this session still running), if anything.
    pub fn busy(&self) -> Option<&'static str> {
        self.in_flight.iter().rev().find(|(epoch, _)| *epoch == self.session.epoch).map(|(_, label)| *label)
    }

    fn set_info(&mut self, message: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        cx.toast(message).success().show(window, cx);
    }

    fn set_error(&mut self, message: impl Into<String>, window: &mut Window, cx: &mut Context<Self>) {
        let message = message.into();
        cx.toast(message.clone()).destructive().show(window, cx);
        self.error = Some(message);
    }

    /// A failed request: back to the login screen when the session is no good, otherwise just
    /// the message (the network or the server may recover; R retries).
    /// The stored token is deleted then: it would fail the same way on the next start.
    fn api_error(&mut self, error: timechimp::Error, window: &mut Window, cx: &mut Context<Self>) {
        if error.is_auth() {
            token::delete_token();
            self.show_login(Some(format!("Session expired or invalid, please log in again ({error})")), window, cx);
        } else {
            self.set_error(format!("Error: {error}"), window, cx);
        }
    }

    /// A failed request of the open form: the message in the form, unless the session is no good.
    fn form_api_error(&mut self, error: timechimp::Error, window: &mut Window, cx: &mut Context<Self>) {
        if error.is_auth() {
            self.api_error(error, window, cx);
        } else {
            self.form_error(error.to_string(), cx);
        }
    }

    // ---- Session ---------------------------------------------------------------------------

    fn show_login(&mut self, error: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let email = self
            .session
            .me
            .as_ref()
            .map(|m| m.user_name.clone())
            .or_else(|| std::env::var("TIMECHIMP_EMAIL").ok())
            .unwrap_or_default();
        let email_input = forms::text_input(cx, email.clone());
        let password = cx.new(PasswordField::new);
        let code = forms::text_input(cx, "");
        let sub = cx.subscribe_in(&password, window, |this, _, _: &PasswordEvent, window, cx| this.login(window, cx));
        let focus = if email.is_empty() {
            email_input.read(cx).focus_handle(cx)
        } else {
            password.read(cx).focus_handle(cx)
        };
        window.focus(&focus, cx);

        self.session = Session::new(None, domain::time::today());
        self.form = None;
        self.confirm = None;
        self.show_detail = false;
        self.menu = None;
        self.error = None;
        self.screen = Screen::Login(Box::new(Login {
            email: email_input,
            password,
            code,
            mfa: None,
            error,
            _subscriptions: vec![sub],
        }));
        cx.notify();
    }

    /// Enter on the login screen: move on from the email field, log in, or send the MFA code.
    pub fn login_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Login(login) = &self.screen else { return };
        if login.mfa.is_some() {
            self.submit_mfa_code(window, cx);
        } else if login.email.read(cx).focus_handle(cx).is_focused(window) {
            let focus = login.password.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        } else {
            self.login(window, cx);
        }
    }

    pub fn login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Login(login) = &self.screen else { return };
        if self.busy().is_some() || login.mfa.is_some() {
            return;
        }
        let email = text(&login.email, cx);
        let password = login.password.read(cx).value().to_owned();
        self.run("Logging in…", async move { Authenticator::login(&email, &password).await }, window, cx, |this, result, window, cx| {
            match result {
                Ok(LoginOutcome::LoggedIn(auth)) => this.logged_in(auth, window, cx),
                Ok(LoginOutcome::MfaRequired(challenge)) => {
                    if let Screen::Login(login) = &mut this.screen {
                        login.mfa = Some(challenge);
                        login.error = None;
                        let focus = login.code.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    }
                }
                Err(e) => {
                    if let Screen::Login(login) = &mut this.screen {
                        login.error = Some(e.to_string());
                    }
                }
            }
        });
    }

    fn submit_mfa_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Login(login) = &mut self.screen else { return };
        let Some(challenge) = login.mfa.take() else { return };
        let code = text(&login.code, cx);
        forms::set_text(&login.code, "", cx);
        self.run("Logging in…", async move { challenge.respond(&code).await }, window, cx, |this, result, window, cx| {
            match result {
                Ok(auth) => this.logged_in(auth, window, cx),
                Err(e) => {
                    if let Screen::Login(login) = &mut this.screen {
                        login.error = Some(format!("{e}. Please log in again."));
                        let focus = login.password.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    }
                }
            }
        });
    }

    fn logged_in(&mut self, auth: Authenticator, window: &mut Window, cx: &mut Context<Self>) {
        let save_error = token::save_token(auth.refresh_token()).err();
        self.session.client = Some(Client::new(auth));
        self.screen = Screen::Main;
        window.focus(&self.focus, cx);
        self.reload(window, cx);
        if let Some(e) = save_error {
            self.set_error(format!("Could not save session: {e}"), window, cx);
        }
    }

    pub fn logout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        token::delete_token();
        self.show_login(None, window, cx);
    }

    // ---- Loading ---------------------------------------------------------------------------

    /// Loads everything.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.session.client.clone() else { return };
        let (week_start, report_month, want_report) = (self.session.week_start, self.session.report_month, self.tab == Tab::Report);
        // The report is stale from here on; the Report tab loads it again when shown.
        self.session.report = Report::default();
        self.session.week_generation += 1;
        let generation = self.session.week_generation;
        let today = domain::time::today();
        let future = async move {
            let base = net::load_base(&client).await?;
            let user_id = base.me.id;
            // The history only feeds the Recent picker; failing to load it doesn't fail the reload.
            let (week, history) =
                tokio::join!(net::load_week(&client, user_id, week_start), net::load_history(&client, user_id, today, week_start));
            let report = if want_report { Some(net::load_report(&client, user_id, report_month).await?) } else { None };
            timechimp::Result::Ok((base, week?, report, history))
        };
        self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
            Ok((base, week, report, history)) => {
                // Clear the old error first: loading the config or the history may report one.
                this.error = None;
                let history = history.map_err(|e| this.set_error(format!("Could not load recent entries: {e}"), window, cx));
                this.session.apply_base(base, history.ok(), &week.entries);
                this.load_config(window, cx);
                // A report loaded meanwhile (tab switch, other month) is newer than this one.
                if let Some(report) = report.filter(|r| r.month == Some(this.session.report_month)) {
                    this.session.report = report;
                }
                if generation == this.session.week_generation {
                    this.on_week_loaded(week, None);
                }
            }
            Err(e) => this.api_error(e, window, cx),
        });
    }

    /// Reads `config.toml`, creating a starter file (listing projects and tags) if missing.
    fn load_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match config::path().and_then(|path| Ok((config::load(&path)?, path))) {
            Ok((Some(config), _)) => {
                if let Some(warning) = self.session.apply_config(Some(config)) {
                    self.set_error(warning, window, cx);
                }
            }
            Ok((None, path)) => {
                self.session.apply_config(None);
                match config::write_template(&path, &self.session.projects, &self.session.tags) {
                    Ok(()) => self.set_info(format!("Created {}; add the tags per project there", path.display()), window, cx),
                    Err(e) => self.set_error(format!("Could not create config.toml: {e}"), window, cx),
                }
            }
            Err(e) => {
                self.session.apply_config(None);
                self.set_error(e, window, cx);
            }
        }
    }

    /// Loads the current week; afterwards selects the item with `select_id`, if given.
    fn load_week(&mut self, select_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client.clone(), self.session.me.as_ref()) else { return };
        let (user_id, week_start) = (me.id, self.session.week_start);
        self.session.week_generation += 1;
        let generation = self.session.week_generation;
        self.run(
            "Loading…",
            async move { net::load_week(&client, user_id, week_start).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(week) if generation == this.session.week_generation => {
                    this.on_week_loaded(week, select_id);
                    this.error = None;
                }
                Ok(_) => {}
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    /// A loaded week: into the session, with the sweep animation under the header.
    fn on_week_loaded(&mut self, week: net::Week, select_id: Option<i64>) {
        self.fx.sweep += 1;
        self.session.apply_week(week, select_id);
    }

    fn load_report(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client.clone(), self.session.me.as_ref()) else { return };
        let (user_id, month) = (me.id, self.session.report_month);
        self.run(
            "Loading…",
            async move { net::load_report(&client, user_id, month).await },
            window,
            cx,
            |this, result, window, cx| match result {
                // Only keep it if the user didn't move on to another month meanwhile.
                Ok(report) if report.month == Some(this.session.report_month) => {
                    this.session.report = report;
                    this.error = None;
                }
                Ok(_) => {}
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    // ---- Navigation ------------------------------------------------------------------------

    pub fn switch_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let tab = Tab::ALL[index];
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        self.fx.sweep += 1;
        self.tabs.update(cx, |t, cx| t.set_selected(tab.id().into(), cx));
        if tab == Tab::Report && self.session.report.month != Some(self.session.report_month) {
            self.load_report(window, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Moves the selected day; loads data only when the week changes.
    pub fn shift_day(&mut self, days: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.select_day(self.session.day + Duration::days(days), window, cx);
    }

    /// The ‹/› buttons: a day on the Week tab, a week on the Mileage tab.
    pub fn step(&mut self, direction: i64, window: &mut Window, cx: &mut Context<Self>) {
        let by_day = self.tab == Tab::Week;
        self.shift_day(if by_day { direction } else { 7 * direction }, window, cx);
    }

    pub fn select_day(&mut self, day: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.select_day(day) {
            self.load_week(None, window, cx);
        }
        cx.notify();
    }

    pub fn go_today(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Report {
            self.session.report_month = domain::time::first_of_month(domain::time::today());
            self.load_report(window, cx);
        } else {
            self.select_day(domain::time::today(), window, cx);
        }
    }

    pub fn shift_month(&mut self, months: i32, window: &mut Window, cx: &mut Context<Self>) {
        let delta = Months::new(months.unsigned_abs());
        self.session.report_month = if months < 0 { self.session.report_month - delta } else { self.session.report_month + delta };
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
    pub fn click_entry(&mut self, index: usize, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.session.selected_entry = Some(index);
        if let Some(day) = self.session.entries.get(index).map(|e| e.date) {
            self.session.day = day;
        }
        window.focus(&self.focus, cx);
        if clicks >= 2 {
            self.edit_selected(window, cx);
        }
        cx.notify();
    }

    pub fn click_mileage(&mut self, index: usize, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.session.selected_mileage = Some(index);
        window.focus(&self.focus, cx);
        if clicks >= 2 {
            self.edit_selected(window, cx);
        }
        cx.notify();
    }

    // ---- Keyboard --------------------------------------------------------------------------

    pub fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Main) || self.form.is_some() {
            return;
        }
        if self.menu.is_some() || self.show_detail {
            self.menu = None;
            self.show_detail = false;
            cx.stop_propagation();
            cx.notify();
            return;
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
            ("left" | "h", _) if week_tab && !shift => self.shift_day(-1, window, cx),
            ("right" | "l", _) if week_tab && !shift => self.shift_day(1, window, cx),
            ("left" | "h" | "[", _) => self.shift_day(-7, window, cx),
            ("right" | "l" | "]", _) => self.shift_day(7, window, cx),
            ("down" | "j", _) => self.move_selection(1),
            ("up" | "k", _) => self.move_selection(-1),
            ("enter", _) if week_tab && self.session.selected_entry().is_some() => self.show_detail = true,
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

    // ---- Forms -----------------------------------------------------------------------------

    pub fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.me.is_none() {
            return;
        }
        if self.tab == Tab::Mileage {
            self.open_mileage_form(None, None, window, cx);
        } else {
            self.open_time_form(None, false, window, cx);
        }
    }

    pub fn edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Mileage {
            if let Some(trip) = self.session.selected_mileage().cloned() {
                self.open_mileage_form(Some(Box::new(trip)), None, window, cx);
            }
        } else if let Some(entry) = self.session.selected_entry().cloned() {
            self.open_time_form(Some(&entry), false, window, cx);
        }
    }

    /// Copies the selected entry as a new one on the selected day.
    pub fn copy_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.session.selected_entry().cloned() {
            self.open_time_form(Some(&entry), true, window, cx);
        }
    }

    /// Form for a new time entry, for editing `entry`, or (with `copy`) for a copy of it.
    fn open_time_form(&mut self, entry: Option<&TimeEntry>, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        let projects = self.session.project_options(entry.map(|e| (e.project_id, &*e.project_name, &*e.customer_name)));
        // New entries default to the project/task/tag of the most recently modified entry.
        let template = entry.or_else(|| self.session.entries.iter().max_by_key(|e| e.modified));
        let project_id = template.map(|e| e.project_id).or(projects.first().map(|(id, _)| *id));
        let tasks = self.session.task_options(project_id, entry);

        let tags = self.tag_checkboxes(project_id, template.map_or(&[], |e| &e.tag_ids), cx);
        let kept_tag_ids = entry.map(|e| self.session.kept_tags(project_id, &e.tag_ids)).unwrap_or_default();

        let editing = entry.filter(|_| !copy);
        let date = editing.map(|e| e.date).unwrap_or(self.session.day);
        let (start, end) = editing
            .and_then(|e| e.start_end.as_deref())
            .and_then(|s| s.split_once('-'))
            .map(|(s, e)| (s.to_owned(), e.to_owned()))
            .unwrap_or_else(|| (self.session.next_start_time(date).unwrap_or_default(), String::new()));
        let hours = editing.map(|e| format!("{}", e.hours)).unwrap_or_default();
        let notes = entry.and_then(|e| e.notes.clone()).unwrap_or_default();

        let project = forms::choice(cx, "form-project", "Project", projects, project_id);
        let project_sub = self.subscribe_project(&project, window, cx);
        let recent = (entry.is_none() && !self.session.recent.is_empty())
            .then(|| forms::RecentPicker { filter: forms::text_input(cx, ""), selected: 0 });
        let form = TimeForm {
            kind: match editing {
                None => TimeKind::New,
                Some(e) => TimeKind::Edit {
                    id: e.id,
                    pause: e.pause,
                    timezone: e.timezone.clone(),
                    task: (e.project_task_id, e.task_id),
                },
            },
            date: forms::DateField::new(cx, date),
            task: forms::choice(cx, "form-task", "Task", tasks, template.map(|e| e.project_task_id)),
            start: forms::text_input(cx, start.clone()),
            end: forms::text_input(cx, end),
            hours: forms::text_input(cx, hours),
            notes: forms::text_input(cx, notes),
            tags,
            kept_tag_ids,
            project,
            recent,
            project_sub,
        };
        // A new entry starts in the Recent filter; editing jumps to the times, a copy to the end
        // time when the start is already filled in.
        let focus = match &form.recent {
            Some(picker) => &picker.filter,
            None if editing.is_some() || start.is_empty() => &form.start,
            None => &form.end,
        };
        window.focus(&focus.read(cx).focus_handle(cx), cx);
        let mut subscriptions = Vec::new();
        // `+15` / `+1h` / `+1h30` in End becomes start + that duration when leaving the field.
        subscriptions.push(cx.subscribe_in(&form.end, window, |this, end, event: &InputStateEvent, _, cx| {
            let Some(Form { body: FormBody::Time(form), .. }) = &this.form else { return };
            if !matches!(event, InputStateEvent::Blur) {
                return;
            }
            if let Some(expanded) = domain::time::expand_end(&text(&form.start, cx), &text(end, cx)) {
                forms::set_text(end, expanded, cx);
            }
        }));
        if let Some(picker) = &form.recent {
            // A new filter text highlights the first match again.
            subscriptions.push(cx.subscribe_in(&picker.filter, window, |this, _, event: &InputStateEvent, _, cx| {
                if matches!(event, InputStateEvent::TextChanged) {
                    if let Some(picker) = this.recent_picker_mut() {
                        picker.selected = 0;
                    }
                    cx.notify();
                }
            }));
        }
        self.form = Some(Form { body: FormBody::Time(form), error: None, _subscriptions: subscriptions });
        cx.notify();
    }

    fn subscribe_project(
        &self,
        project: &Entity<gpuikit::elements::select::SelectState<i64>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(project, window, |this, _, _: &SelectChanged, window, cx| this.on_project_changed(window, cx))
    }

    fn recent_picker_mut(&mut self) -> Option<&mut forms::RecentPicker> {
        match &mut self.form {
            Some(Form { body: FormBody::Time(TimeForm { recent: Some(picker), .. }), .. }) => Some(picker),
            _ => None,
        }
    }

    /// The recent items matching the filter of the open time form, best first, at most `RECENT_ROWS`.
    /// None while the filter is empty: the list only appears once you type.
    pub fn recent_matches(&self, cx: &gpui::App) -> Vec<&domain::week::RecentItem> {
        let Some(Form { body: FormBody::Time(TimeForm { recent: Some(picker), .. }), .. }) = &self.form else {
            return Vec::new();
        };
        let query = text(&picker.filter, cx);
        if query.is_empty() {
            return Vec::new();
        }
        self.session.recent.iter().filter(|item| item.matches(&query)).take(RECENT_ROWS).collect()
    }

    /// ↑/↓ in the Recent filter.
    pub fn move_recent(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.recent_matches(cx).len();
        if let Some(picker) = self.recent_picker_mut() {
            picker.selected = picker.selected.saturating_add_signed(delta).min(count.saturating_sub(1));
        }
        cx.notify();
    }

    /// Enter in the Recent filter: fills in the highlighted item, or (without a match) moves on.
    pub fn pick_highlighted_recent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = match &self.form {
            Some(Form { body: FormBody::Time(TimeForm { recent: Some(picker), .. }), .. }) => picker.selected,
            _ => return,
        };
        match self.recent_matches(cx).get(selected).map(|item| item.entry.clone()) {
            Some(entry) => self.pick_recent(entry, window, cx),
            None => window.focus_next(cx),
        }
    }

    /// Fills the open time form with the project, task, notes and tags of `entry` (a recent item);
    /// date and times stay. Focus moves on to the times.
    pub fn pick_recent(&mut self, entry: TimeEntry, window: &mut Window, cx: &mut Context<Self>) {
        let fill = TimeFill {
            project_id: entry.project_id,
            project_task_id: Some(entry.project_task_id),
            notes: entry.notes.clone().unwrap_or_default(),
            tag_ids: entry.tag_ids.clone(),
        };
        self.fill_time_form(fill, Some(&entry), window, cx);
    }

    /// Fills the open time form with `fill` (`entry`: where it comes from, so its project and
    /// task are offered even when no longer active); date and times stay. Focus moves on to the
    /// times.
    fn fill_time_form(&mut self, fill: TimeFill, entry: Option<&TimeEntry>, window: &mut Window, cx: &mut Context<Self>) {
        let projects = self.session.project_options(entry.map(|e| (e.project_id, &*e.project_name, &*e.customer_name)));
        let project = forms::choice(cx, "form-project", "Project", projects, Some(fill.project_id));
        let tasks = self.session.task_options(Some(fill.project_id), entry);
        let task = forms::choice(cx, "form-task", "Task", tasks, fill.project_task_id);
        let tags = self.tag_checkboxes(Some(fill.project_id), &fill.tag_ids, cx);
        let kept_tag_ids = self.session.kept_tags(Some(fill.project_id), &fill.tag_ids);
        let project_sub = self.subscribe_project(&project, window, cx);
        let Some(Form { body: FormBody::Time(form), error, .. }) = &mut self.form else { return };
        forms::set_text(&form.notes, fill.notes, cx);
        form.project = project;
        form.task = task;
        form.tags = tags;
        form.kept_tag_ids = kept_tag_ids;
        form.project_sub = project_sub;
        *error = None;
        let next = if text(&form.start, cx).is_empty() { &form.start } else { &form.end };
        window.focus(&next.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// The task list and the tag checkboxes follow the project.
    fn on_project_changed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(Form { body: FormBody::Time(form), .. }) = &self.form else { return };
        let project_id = chosen(&form.project, cx);
        let options = self.session.task_options(project_id, None);
        let task = forms::choice(cx, "form-task", "Task", options, None);
        // Pre-check the tags the project's most recently modified entry had, and the kept tags
        // this project has a checkbox for; the rest stays kept.
        let last = self.session.entries.iter().filter(|e| Some(e.project_id) == project_id).max_by_key(|e| e.modified);
        let mut checked: Vec<i64> = last.map_or(&[][..], |e| &e.tag_ids).to_vec();
        checked.extend(&form.kept_tag_ids);
        let kept_tag_ids = self.session.kept_tags(project_id, &form.kept_tag_ids);
        let tags = self.tag_checkboxes(project_id, &checked, cx);
        if let Some(Form { body: FormBody::Time(form), .. }) = &mut self.form {
            form.task = task;
            form.tags = tags;
            form.kept_tag_ids = kept_tag_ids;
        }
        cx.notify();
    }

    /// Checkboxes for the tags configured for `project_id`, checked where `checked` has the tag.
    fn tag_checkboxes(&self, project_id: Option<i64>, checked: &[i64], cx: &mut Context<Self>) -> Vec<forms::TagCheckbox> {
        let tags = project_id.and_then(|id| self.session.project_tags.get(&id)).cloned().unwrap_or_default();
        tags.into_iter().map(|t| forms::tag_checkbox(cx, t.id, t.name, checked.contains(&t.id))).collect()
    }

    /// A trip preset in the "+ Add trip" dropdown: the trip form for the selected day, filled in
    /// from preset `index`.
    pub fn open_trip_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.me.is_none() || self.form.is_some() {
            return;
        }
        self.menu = None;
        if let Some(preset) = self.session.trip_presets.get(index).cloned() {
            self.open_mileage_form(None, Some(&preset), window, cx);
        }
    }

    /// A time preset in the "+ Add time" dropdown: a new time entry on the selected day, with the
    /// project, task, notes and tags of preset `index`.
    pub fn open_time_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.me.is_none() || self.form.is_some() {
            return;
        }
        self.menu = None;
        let Some(preset) = self.session.time_presets.get(index).cloned() else { return };
        let resolved = match self.session.resolve_time_preset(&preset) {
            Ok(resolved) => resolved,
            Err(unknown) => return self.set_error(format!("Preset: unknown {unknown}"), window, cx),
        };
        self.open_time_form(None, false, window, cx);
        let fill = TimeFill {
            project_id: resolved.project_id,
            project_task_id: resolved.project_task_id,
            notes: preset.notes.unwrap_or_default(),
            tag_ids: resolved.tag_ids,
        };
        self.fill_time_form(fill, None, window, cx);
    }

    /// "Save as preset" in the time or trip form: appends it to `config.toml`.
    pub fn save_as_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = match &self.form {
            Some(Form { body: FormBody::Mileage(f), .. }) => validate::preset(&f.values(cx), &self.session)
                .and_then(|p| config::path().and_then(|path| config::append_preset(&path, &p)).map(|_| p.name)),
            Some(Form { body: FormBody::Time(f), .. }) => validate::time_preset(&f.values(cx), &self.session).and_then(|p| {
                let label = session::time_preset_label(&p).to_owned();
                config::path().and_then(|path| config::append_preset(&path, &p)).map(|_| label)
            }),
            None => return,
        };
        match saved {
            Ok(name) => {
                if let Some(form) = &mut self.form {
                    form.error = None;
                }
                self.load_config(window, cx);
                self.set_info(format!("Preset \"{name}\" added to config.toml"), window, cx);
            }
            Err(e) => self.form_error(e, cx),
        }
    }

    /// The trip form: for a new trip filled in from `preset`, or empty (project, type and billable
    /// from the last trip); or filled in with `editing`.
    fn open_mileage_form(
        &mut self,
        editing: Option<Box<Mileage>>,
        preset: Option<&config::TripPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let template = editing.as_deref().or(self.session.mileage.last());
        let saved = self.session.saved_addresses();
        let types = MILEAGE_TYPES.iter().map(|(id, name)| (*id, name.to_string())).collect();
        let projects =
            self.session.project_options(editing.as_deref().map(|m| (m.project_id, &*m.project_name, &*m.customer_name)));
        let (mut project, mut mileage_type, mut billable, mut return_trip) = (
            template.map(|m| m.project_id),
            template.map(|m| m.mileage_type),
            template.is_some_and(|m| m.billable),
            false,
        );
        if let Some(p) = preset {
            project = self.session.project_named(&p.project).map(|p| p.id).or(project);
            mileage_type = p.trip_type();
            billable = p.billable;
            return_trip = p.return_trip;
        }
        let (date, from, to, distance, notes) = match (&editing, preset) {
            (None, Some(p)) => (
                self.session.day,
                p.from.clone(),
                p.to.clone(),
                p.distance.map(|d| format!("{d}")).unwrap_or_default(),
                p.notes.clone().unwrap_or_default(),
            ),
            (Some(m), _) => (
                m.date,
                m.from_address.clone().unwrap_or_default(),
                m.to_address.clone().unwrap_or_default(),
                format!("{}", m.distance),
                m.notes.clone().unwrap_or_default(),
            ),
            (None, None) => (
                self.session.day,
                saved.first().cloned().unwrap_or_default(),
                saved.get(1).cloned().unwrap_or_default(),
                String::new(),
                String::new(),
            ),
        };
        let form = MileageForm {
            editing,
            date: forms::DateField::new(cx, date),
            project: forms::choice(cx, "form-project", "Project", projects, project),
            from: forms::text_input(cx, from),
            to: forms::text_input(cx, to),
            distance: forms::text_input(cx, distance),
            return_trip: forms::toggle(cx, "form-return", "Return trip", return_trip),
            mileage_type: forms::choice(cx, "form-type", "Type", types, mileage_type),
            billable: forms::toggle(cx, "form-billable", "Billable", billable),
            notes: forms::text_input(cx, notes),
            addresses: saved,
            address_menu: None,
        };
        // Typing in From/To filters the suggestions; leaving the field closes them.
        let mut subscriptions = Vec::new();
        for field in [AddressField::From, AddressField::To] {
            let input = form.address_input(field);
            subscriptions.push(cx.subscribe_in(input, window, move |this, _, event: &InputStateEvent, _, cx| {
                if matches!(event, InputStateEvent::TextChanged) {
                    this.open_addresses(field, true, cx);
                }
            }));
            let handle = input.read(cx).focus_handle(cx);
            subscriptions.push(cx.on_focus_out(&handle, window, move |this, _, _, cx| this.close_addresses(field, cx)));
        }
        window.focus(&form.from.read(cx).focus_handle(cx), cx);
        self.form = Some(Form { body: FormBody::Mileage(form), error: None, _subscriptions: subscriptions });
        cx.notify();
    }

    pub fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn form_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form {
            form.error = Some(message.into());
        }
        cx.notify();
    }

    fn mileage_form(&self) -> Option<&MileageForm> {
        match &self.form {
            Some(Form { body: FormBody::Mileage(form), .. }) => Some(form),
            _ => None,
        }
    }

    fn mileage_form_mut(&mut self) -> Option<&mut MileageForm> {
        match &mut self.form {
            Some(Form { body: FormBody::Mileage(form), .. }) => Some(form),
            _ => None,
        }
    }

    /// The calendar icon of the Date field: focuses the field and opens the calendar, or closes it.
    pub fn toggle_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let open = form.date().picker.is_some();
        window.focus(&form.date().input.read(cx).focus_handle(cx), cx);
        if open {
            self.close_calendar(cx);
        } else {
            self.open_calendar(window, cx);
        }
    }

    /// Opens a calendar under the Date field on the date typed in (or the selected day).
    ///
    /// The Date field keeps the focus and drives the calendar (`move_calendar`): gpuikit draws a
    /// focused calendar's focus ring as a filled shadow behind its transparent grid, which turns
    /// the whole grid accent-coloured, so a click on it hands the focus straight back.
    pub fn open_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        if form.date().picker.is_some() {
            return;
        }
        let date = form.date().parsed(cx).unwrap_or(self.session.day);
        let input = form.date().input.clone();
        let calendar = cx.new(|cx| {
            Calendar::new("form-calendar", forms::to_calendar_date(date), cx)
                .first_day_of_week(Weekday::Monday)
                .today(forms::to_calendar_date(domain::time::today()))
                .selected(Some(forms::to_calendar_date(date)))
        });
        let calendar_focus = calendar.read(cx).focus_handle(cx);
        let input_focus = input.read(cx).focus_handle(cx);
        let subscriptions = vec![
            // A clicked day fills in the field and closes the calendar.
            cx.subscribe_in(&calendar, window, |this, _, event: &CalendarEvent, window, cx| {
                if let CalendarEvent::Selected(day) = event
                    && let (Some(form), Some(day)) = (&this.form, forms::from_calendar_date(*day))
                {
                    let input = &form.date().input;
                    input.update(cx, |input, cx| input.set_content_silent(day.to_string(), cx));
                    // The click focused the calendar, which is about to go.
                    window.focus(&input.read(cx).focus_handle(cx), cx);
                    this.close_calendar(cx);
                }
            }),
            // A typed date shows in the calendar.
            cx.subscribe_in(&input, window, |this, _, event: &InputStateEvent, _, cx| {
                if let (InputStateEvent::TextChanged, Some(form)) = (event, &this.form)
                    && let (Some(picker), Some(date)) = (&form.date().picker, form.date().parsed(cx))
                {
                    picker.calendar.update(cx, |c, cx| c.set_selected(Some(forms::to_calendar_date(date)), cx));
                }
            }),
            cx.on_focus_in(&calendar_focus, window, {
                let input_focus = input_focus.clone();
                move |_, window, cx| window.focus(&input_focus, cx)
            }),
            // Leaving the field closes it, but not a click on the calendar: by the time this runs,
            // the focus may be on the calendar or already handed back to the field.
            cx.on_focus_out(&input_focus.clone(), window, move |this, _, window, cx| {
                if !calendar_focus.is_focused(window) && !input_focus.is_focused(window) {
                    this.close_calendar(cx);
                }
            }),
        ];
        if let Some(form) = &mut self.form {
            form.date_mut().picker = Some(forms::DatePicker { calendar, _subscriptions: subscriptions });
        }
        cx.notify();
    }

    /// Closes the calendar, if open.
    pub fn close_calendar(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form
            && form.date_mut().picker.take().is_some()
        {
            cx.notify();
        }
    }

    /// The arrows and PageUp/PageDown in the Date field while the calendar is open: move the date
    /// by `days` and `months`, in the field and the calendar. ↓ opens a closed calendar; other keys
    /// then do what they normally do in a text field.
    pub fn move_calendar(&mut self, days: i64, months: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return cx.propagate() };
        let field = form.date();
        let Some(picker) = &field.picker else {
            return if days == 7 { self.open_calendar(window, cx) } else { cx.propagate() };
        };
        let current = field.parsed(cx).unwrap_or(self.session.day);
        let shifted = match months {
            0 => Some(current),
            m if m > 0 => current.checked_add_months(Months::new(m.unsigned_abs())),
            m => current.checked_sub_months(Months::new(m.unsigned_abs())),
        };
        let Some(date) = shifted.and_then(|d| d.checked_add_signed(Duration::days(days))) else { return };
        field.input.update(cx, |input, cx| input.set_content_silent(date.to_string(), cx));
        picker.calendar.update(cx, |c, cx| c.set_selected(Some(forms::to_calendar_date(date)), cx));
        cx.notify();
    }

    /// Enter or Escape in the Date field: closes an open calendar (keeping the date); with none
    /// open the key goes on, so Enter saves and Escape cancels the form.
    pub fn dismiss_calendar(&mut self, cx: &mut Context<Self>) {
        match &self.form {
            Some(form) if form.date().picker.is_some() => self.close_calendar(cx),
            _ => cx.propagate(),
        }
    }

    /// Opens the suggestions under `field`: filtered by its text after typing (nothing
    /// highlighted, so Enter keeps what was typed), or all of them from ↓ or ▾.
    pub fn open_addresses(&mut self, field: AddressField, filtered: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.mileage_form_mut() {
            let highlighted = (!filtered).then_some(0);
            form.address_menu = Some(AddressMenu { field, filtered, highlighted });
            cx.notify();
        }
    }

    /// Closes the suggestions if they are open under `field`.
    pub fn close_addresses(&mut self, field: AddressField, cx: &mut Context<Self>) {
        if let Some(form) = self.mileage_form_mut()
            && form.address_menu.as_ref().is_some_and(|m| m.field == field)
        {
            form.address_menu = None;
            cx.notify();
        }
    }

    /// The ▾ of an address field: focuses it and shows all suggestions, or closes them.
    pub fn toggle_addresses(&mut self, field: AddressField, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form() else { return };
        let open = form.address_menu.as_ref().is_some_and(|m| m.field == field);
        window.focus(&form.address_input(field).read(cx).focus_handle(cx), cx);
        if open {
            self.close_addresses(field, cx);
        } else {
            self.open_addresses(field, false, cx);
        }
    }

    /// ↑/↓ in an address field: moves through the suggestions, opening the full list first.
    pub fn move_address(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form() else { return cx.propagate() };
        let Some(field) = form.focused_address(window, cx) else { return cx.propagate() };
        let count = form.address_matches(cx).len();
        let Some(menu) = self.mileage_form_mut().and_then(|f| f.address_menu.as_mut()).filter(|m| m.field == field) else {
            return self.open_addresses(field, false, cx);
        };
        let last = count.saturating_sub(1);
        menu.highlighted = Some(match menu.highlighted {
            None => 0,
            Some(i) => i.saturating_add_signed(delta).min(last),
        });
        cx.notify();
    }

    /// Enter in an address field: takes the highlighted suggestion; with none, Enter saves the form.
    pub fn pick_highlighted_address(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form() else { return cx.propagate() };
        let picked = form
            .address_menu
            .as_ref()
            .and_then(|m| Some((m.field, form.address_matches(cx).get(m.highlighted?)?.to_string())));
        match picked {
            Some((field, address)) => self.pick_address(field, address, cx),
            None => {
                if let Some(form) = self.mileage_form_mut() {
                    form.address_menu = None;
                }
                cx.propagate();
            }
        }
    }

    /// Puts `address` in `field` and closes the suggestions.
    pub fn pick_address(&mut self, field: AddressField, address: String, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form_mut() else { return };
        form.address_menu = None;
        // Silent, or the TextChanged subscription would reopen the list filtered by the address.
        form.address_input(field).update(cx, |input, cx| input.set_content_silent(address, cx));
        cx.notify();
    }

    /// Escape in an address field: closes the suggestions; with none open, Escape closes the form.
    pub fn dismiss_addresses(&mut self, cx: &mut Context<Self>) {
        match self.mileage_form_mut() {
            Some(form) if form.address_menu.is_some() => {
                form.address_menu = None;
                cx.notify();
            }
            _ => cx.propagate(),
        }
    }

    pub fn calculate_distance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Form { body: FormBody::Mileage(form), .. }), Some(client)) = (&self.form, self.session.client.clone()) else {
            return;
        };
        let (from, to) = (text(&form.from, cx), text(&form.to, cx));
        if from.is_empty() || to.is_empty() {
            return self.form_error("Fill in both addresses first", cx);
        }
        // Ok(Err(message)): an address the lookup doesn't know.
        let future = async move {
            let lookup = |query: String| {
                let client = client.clone();
                async move {
                    let place = client.place_predictions(&query).await?.into_iter().next();
                    timechimp::Result::Ok(place.ok_or(format!("Address not found: {query}")))
                }
            };
            let (from_place, to_place) = tokio::try_join!(lookup(from), lookup(to))?;
            let (from_place, to_place) = match (from_place, to_place) {
                (Ok(from), Ok(to)) => (from, to),
                (Err(message), _) | (_, Err(message)) => return Ok(Err(message)),
            };
            let route = client.route_distance(&from_place.place_id, &to_place.place_id).await?;
            timechimp::Result::Ok(Ok((from_place.description, to_place.description, route)))
        };
        self.run("Calculating distance…", future, window, cx, |this, result, window, cx| match result {
            Ok(Ok((from, to, route))) => {
                if let Some(form) = &mut this.form {
                    form.error = None;
                    if let FormBody::Mileage(f) = &form.body {
                        forms::set_text(&f.from, from, cx);
                        forms::set_text(&f.to, to, cx);
                        forms::set_text(&f.distance, format!("{:.0}", route.kilometres()), cx);
                    }
                }
                this.set_info(
                    format!("Route: {:.1} km, {} min by car", route.kilometres(), route.seconds / 60),
                    window,
                    cx,
                );
            }
            Ok(Err(message)) => this.form_error(message, cx),
            Err(e) => this.form_api_error(e, window, cx),
        });
    }

    pub fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy().is_some() {
            return;
        }
        let Some(client) = self.session.client.clone() else { return };
        let (date, request) = match self.build_save(cx) {
            Ok(r) => r,
            Err(message) => return self.form_error(message, cx),
        };
        let future = async move {
            match request {
                SaveRequest::Time { id: Some(id), entry } => {
                    Ok(Saved::Done(client.update_time_entry(id, &entry).await?.id, "Time entry updated."))
                }
                SaveRequest::Time { id: None, entry } => {
                    Ok(Saved::Done(client.create_time_entry(&entry).await?.id, "Time entry saved."))
                }
                SaveRequest::Mileage { trip, editing: Some(original), .. } => {
                    Ok(Saved::Done(client.update_mileage(&original, &trip).await?.id, "Trip updated."))
                }
                SaveRequest::Mileage { trip, return_trip, editing: None } => {
                    let id = client.create_mileage(&trip).await?.id;
                    if !return_trip {
                        return Ok(Saved::Done(id, "Mileage saved."));
                    }
                    let back = NewMileage { from_address: trip.to_address.clone(), to_address: trip.from_address.clone(), ..trip };
                    match client.create_mileage(&back).await {
                        Ok(_) => Ok(Saved::Done(id, "Mileage saved (return trip included).")),
                        // The outbound trip is in: closing the form keeps Save from adding it twice.
                        Err(error) => Ok(Saved::Partial(id, error)),
                    }
                }
            }
        };
        self.run("Saving…", future, window, cx, move |this, result: timechimp::Result<Saved>, window, cx| {
            let (id, outcome) = match result {
                Ok(Saved::Done(id, message)) => (id, Ok(message)),
                Ok(Saved::Partial(id, e)) => (id, Err(e)),
                Err(e) => return this.form_api_error(e, window, cx),
            };
            this.close_form(window, cx);
            // Jump to the day of the saved item and select it.
            this.session.day = date;
            this.session.week_start = domain::time::monday_of(date);
            this.session.report = Report::default();
            this.load_week(Some(id), window, cx);
            match outcome {
                Ok(message) => this.set_info(message, window, cx),
                Err(e) if e.is_auth() => this.api_error(e, window, cx),
                Err(e) => this.set_error(format!("Outbound trip saved, but the return trip failed: {e}"), window, cx),
            }
        });
    }

    /// Validates the open form and turns it into an API request.
    fn build_save(&self, cx: &Context<Self>) -> Result<(NaiveDate, SaveRequest), String> {
        match &self.form.as_ref().ok_or("No form open")?.body {
            FormBody::Time(f) => validate::time_entry(&f.values(cx), &self.session),
            FormBody::Mileage(f) => validate::trip(&f.values(cx), &self.session),
        }
    }

    // ---- Delete ----------------------------------------------------------------------------

    pub fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = if self.tab == Tab::Week {
            self.session.selected_entry().map(|e| {
                (
                    DeleteTarget::Time(e.id),
                    "Delete time entry?",
                    format!(
                        "{} {} {:.2}h {} / {}",
                        e.date.format("%a %d %b"),
                        e.start_end.as_deref().unwrap_or(""),
                        e.hours,
                        e.project_name,
                        e.task_name
                    ),
                )
            })
        } else {
            self.session.selected_mileage().map(|m| {
                (
                    DeleteTarget::Mileage(m.id),
                    "Delete mileage?",
                    format!(
                        "{} {} km {} → {}",
                        m.date.format("%a %d %b"),
                        m.distance,
                        m.from_address.as_deref().unwrap_or("?"),
                        m.to_address.as_deref().unwrap_or("?")
                    ),
                )
            })
        };
        let Some((target, question, label)) = target else { return };
        self.confirm_delete(target, question, label, window, cx);
    }

    /// The × of a trip preset in the "+ Add trip" dropdown: removes preset `index` from
    /// `config.toml`, after confirmation.
    pub fn ask_delete_trip_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(preset) = self.session.trip_presets.get(index) else { return };
        let label = format!("\"{}\": {} → {}", preset.name, preset.from, preset.to);
        let target = DeleteTarget::TripPreset(preset.clone());
        self.confirm_delete(target, "Delete preset?", label, window, cx);
    }

    /// The × of a time preset in the "+ Add time" dropdown: removes preset `index` from
    /// `config.toml`, after confirmation.
    pub fn ask_delete_time_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(preset) = self.session.time_presets.get(index) else { return };
        let task = preset.task.as_deref().map(|t| format!(" / {t}")).unwrap_or_default();
        let label = format!("\"{}\": {}{task}", session::time_preset_label(preset), preset.project);
        let target = DeleteTarget::TimePreset(preset.clone());
        self.confirm_delete(target, "Delete preset?", label, window, cx);
    }

    fn confirm_delete(
        &mut self,
        target: DeleteTarget,
        question: &'static str,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.weak_entity();
        let state = cx.new(|_| {
            DialogState::new(dialog("confirm-delete").confirm(question, label).confirm_label("Delete").on_confirm(
                move |window, cx| {
                    let target = target.clone();
                    let _ = this.update(cx, |this, cx| this.delete(target, window, cx));
                },
            ))
        });
        let sub = cx.subscribe_in(&state, window, |this, _, _: &DialogClosed, window, cx| {
            this.confirm = None;
            window.focus(&this.focus, cx);
            cx.notify();
        });
        state.update(cx, |d, cx| d.open(window, cx));
        self.confirm = Some((state, sub));
        cx.notify();
    }

    fn delete(&mut self, target: DeleteTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.session.client.clone() else { return };
        let future = match &target {
            &DeleteTarget::Time(id) => async move { client.delete_time_entry(id).await }.boxed(),
            &DeleteTarget::Mileage(id) => async move { client.delete_mileage(id).await }.boxed(),
            // A preset only lives in config.toml; no request needed.
            DeleteTarget::TripPreset(_) | DeleteTarget::TimePreset(_) => {
                let removed = config::path().and_then(|path| match &target {
                    DeleteTarget::TripPreset(p) => config::remove_preset(&path, p),
                    DeleteTarget::TimePreset(p) => config::remove_preset(&path, p),
                    _ => unreachable!(),
                });
                match removed {
                    Ok(_) => {
                        self.load_config(window, cx);
                        self.set_info("Preset deleted from config.toml", window, cx);
                    }
                    Err(e) => self.set_error(e, window, cx),
                }
                return;
            }
        };
        self.run("Deleting…", future, window, cx, |this, result, window, cx| match result {
            Ok(()) => {
                this.session.report = Report::default();
                this.load_week(None, window, cx);
                this.set_info("Deleted.", window, cx);
            }
            Err(e) => this.api_error(e, window, cx),
        });
    }
}

#[derive(Clone)]
enum DeleteTarget {
    Time(i64),
    Mileage(i64),
    /// A `[[trips]]` preset in `config.toml`.
    TripPreset(config::TripPreset),
    /// A `[[time_entries]]` preset in `config.toml`.
    TimePreset(config::TimePreset),
}

/// What a save did: everything, with a message, or (a trip with its return) only the outbound trip.
enum Saved {
    Done(i64, &'static str),
    Partial(i64, timechimp::Error),
}

/// What `fill_time_form` puts in the open time form.
struct TimeFill {
    project_id: i64,
    project_task_id: Option<i64>,
    notes: String,
    tag_ids: Vec<i64>,
}

/// Rows the Recent picker shows at most.
const RECENT_ROWS: usize = 6;

