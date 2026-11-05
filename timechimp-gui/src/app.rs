//! Application state, keyboard handling and the (async) actions. Rendering lives in `view.rs`.

use std::collections::HashMap;
use std::future::Future;

use chrono::{Duration, Months, NaiveDate};
use gpui::{
    AppContext, Context, Entity, FocusHandle, Focusable, KeyDownEvent, SharedString, Subscription, Window, actions,
};
use gpuikit::elements::dialog::{DialogClosed, DialogState, dialog};
use gpuikit::elements::select::SelectChanged;
use gpuikit::elements::tabs::{TabChanged, Tabs, tab, tabs};
use gpuikit::elements::toast::ToastExt;
use gpuikit::input::{InputState, InputStateEvent};
use timechimp::{
    Authenticator, Client, CurrentUser, LoginOutcome, MfaChallenge, Mileage, NewMileage, NewTimeEntry, Project,
    ProjectTask, Tag, Task, TimeEntry,
};

use crate::forms::{self, Form, FormBody, MileageForm, TimeForm, TimeKind, chosen, text};
use crate::logic::{self, MILEAGE_TYPES, Report};
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
        /// F2 in an address field: cycle through saved addresses.
        CycleAddress,
        /// ↑ in the time form's Recent filter.
        RecentUp,
        /// ↓ in the time form's Recent filter.
        RecentDown,
        /// Enter in the time form's Recent filter: fill in the highlighted item.
        PickRecent,
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

    fn index(self) -> usize {
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
    Login(Login),
    Main,
}

pub struct TimeChimpApp {
    /// Focus target of the main (browse) view; its key handler drives the keyboard shortcuts.
    pub focus: FocusHandle,
    pub screen: Screen,
    pub client: Option<Client>,
    pub me: Option<CurrentUser>,
    pub tab: Tab,
    pub tabs: Entity<Tabs>,
    /// The selected day; `week_start` is always the Monday of its week.
    pub day: NaiveDate,
    /// Week tab: show one day (true) or the whole week grouped per day (false).
    pub week_start: NaiveDate,
    pub entries: Vec<TimeEntry>,
    pub selected_entry: Option<usize>,
    /// Entries of the last few weeks plus every week loaded since, by id; feeds `recent`.
    history: HashMap<i64, TimeEntry>,
    /// What the time form's Recent picker offers, most recent first.
    pub recent: Vec<logic::RecentItem>,
    pub mileage: Vec<Mileage>,
    pub selected_mileage: Option<usize>,
    pub report_month: NaiveDate,
    pub report: Report,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub project_tasks: HashMap<i64, Vec<ProjectTask>>,
    pub tags: Vec<Tag>,
    /// Tags each project may use, from `config.toml` (project id → tags).
    pub project_tags: HashMap<i64, Vec<Tag>>,
    /// Billable share to aim for, from `config.toml` (fraction).
    pub billable_goal: f64,
    pub form: Option<Form>,
    pub confirm: Option<Entity<DialogState>>,
    pub show_detail: bool,
    /// The dropdown under the user name in the header is open.
    pub user_menu: bool,
    pub error: Option<String>,
    pub busy: Option<&'static str>,
    /// Bumped on every week load, so a slow response for a week we already left is dropped.
    week_generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl TimeChimpApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = logic::today();
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
            client: None,
            me: None,
            tab: Tab::Week,
            tabs,
            day: today,
            week_start: logic::monday_of(today),
            entries: Vec::new(),
            selected_entry: None,
            history: HashMap::new(),
            recent: Vec::new(),
            mileage: Vec::new(),
            selected_mileage: None,
            report_month: logic::first_of_month(today),
            report: Report::default(),
            projects: Vec::new(),
            tasks: Vec::new(),
            project_tasks: HashMap::new(),
            tags: Vec::new(),
            project_tags: HashMap::new(),
            billable_goal: logic::DEFAULT_BILLABLE_GOAL,
            form: None,
            confirm: None,
            show_detail: false,
            user_menu: false,
            error: None,
            busy: None,
            week_generation: 0,
            _subscriptions: vec![tab_sub],
        };

        // Continue with a stored refresh token, if any; otherwise show the login screen.
        match logic::load_token() {
            Some(token) => {
                app.client = Some(Client::new(Authenticator::from_refresh_token(token)));
                app.reload(true, window, cx);
                window.focus(&app.focus, cx);
            }
            None => app.to_login(None, window, cx),
        }
        app
    }

    // ---- Async plumbing -------------------------------------------------------------------

    /// Runs `future` on the tokio runtime, shows `label` meanwhile, then applies the result.
    fn run<T: Send + 'static>(
        &mut self,
        label: &'static str,
        future: impl Future<Output = T> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.busy = Some(label);
        cx.notify();
        let handle = logic::runtime().spawn(future);
        cx.spawn_in(window, async move |this, cx| {
            let result = handle.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(value) => done(this, value, window, cx),
                    Err(e) => this.set_error(format!("Background task failed: {e}"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
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

    // ---- Session ---------------------------------------------------------------------------

    fn to_login(&mut self, error: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let email = self
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

        self.client = None;
        self.me = None;
        self.entries.clear();
        self.history.clear();
        self.recent.clear();
        self.mileage.clear();
        self.projects.clear();
        self.tasks.clear();
        self.report = Report::default();
        self.form = None;
        self.confirm = None;
        self.show_detail = false;
        self.error = None;
        self.screen = Screen::Login(Login {
            email: email_input,
            password,
            code,
            mfa: None,
            error,
            _subscriptions: vec![sub],
        });
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
        if self.busy.is_some() || login.mfa.is_some() {
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
        let save_error = logic::save_token(auth.refresh_token()).err();
        self.client = Some(Client::new(auth));
        self.screen = Screen::Main;
        window.focus(&self.focus, cx);
        self.reload(false, window, cx);
        if let Some(e) = save_error {
            self.set_error(format!("Could not save session: {e}"), window, cx);
        }
    }

    pub fn logout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.user_menu = false;
        logic::delete_token();
        self.to_login(None, window, cx);
    }

    // ---- Loading ---------------------------------------------------------------------------

    /// Loads everything. With `resuming`, a failure means the stored session is no good.
    pub fn reload(&mut self, resuming: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let (week_start, report_month, want_report) = (self.week_start, self.report_month, self.tab == Tab::Report);
        self.week_generation += 1;
        let generation = self.week_generation;
        let today = logic::today();
        let future = async move {
            let base = logic::load_base(&client).await?;
            let user_id = base.me.id;
            // The history only feeds the Recent picker; failing to load it doesn't fail the reload.
            let (week, history) =
                tokio::join!(logic::load_week(&client, user_id, week_start), logic::load_history(&client, user_id, today));
            let report = if want_report { Some(logic::load_report(&client, user_id, report_month).await?) } else { None };
            timechimp::Result::Ok((base, week?, report, history))
        };
        self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
            Ok((base, week, report, history)) => {
                this.project_tasks = base.project_tasks;
                this.me = Some(base.me);
                this.projects = base.projects;
                this.tasks = base.tasks;
                this.tags = base.tags;
                this.load_config(window, cx);
                this.report = report.unwrap_or_default();
                this.error = None;
                match history {
                    Ok(history) => {
                        this.history = history.into_iter().map(|e| (e.id, e)).collect();
                        this.recent = logic::recent_items(this.history.values());
                    }
                    Err(e) => this.set_error(format!("Could not load recent entries: {e}"), window, cx),
                }
                if generation == this.week_generation {
                    this.apply_week(week, None);
                }
            }
            Err(e) if resuming => {
                this.to_login(Some(format!("Session expired or invalid, please log in again ({e})")), window, cx)
            }
            Err(e) => this.set_error(format!("Error: {e}"), window, cx),
        });
    }

    /// Reads `config.toml`, creating a starter file (listing projects and tags) if missing.
    fn load_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match logic::load_config() {
            Ok(Some(config)) => {
                self.billable_goal = config.billable_goal;
                let (resolved, warning) = logic::resolve_project_tags(&config.projects, &self.projects, &self.tags);
                self.project_tags = resolved;
                if let Some(warning) = warning {
                    self.set_error(warning, window, cx);
                }
            }
            Ok(None) => {
                self.project_tags.clear();
                self.billable_goal = logic::DEFAULT_BILLABLE_GOAL;
                match logic::write_config_template(&self.projects, &self.tags) {
                    Ok(path) => self.set_info(format!("Created {}; add the tags per project there", path.display()), window, cx),
                    Err(e) => self.set_error(format!("Could not create config.toml: {e}"), window, cx),
                }
            }
            Err(e) => {
                self.project_tags.clear();
                self.set_error(e, window, cx);
            }
        }
    }

    /// Loads the current week; afterwards selects the item with `select_id`, if given.
    fn load_week(&mut self, select_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.as_ref()) else { return };
        let (user_id, week_start) = (me.id, self.week_start);
        self.week_generation += 1;
        let generation = self.week_generation;
        self.run(
            "Loading…",
            async move { logic::load_week(&client, user_id, week_start).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(week) if generation == this.week_generation => {
                    this.apply_week(week, select_id);
                    this.error = None;
                }
                Ok(_) => {}
                Err(e) => this.set_error(format!("Error: {e}"), window, cx),
            },
        );
    }

    fn apply_week(&mut self, week: logic::Week, select_id: Option<i64>) {
        // The week replaces what the history had for it, so saves and deletes show up in Recent.
        let week_end = self.week_start + Duration::days(7);
        self.history.retain(|_, e| !(self.week_start..week_end).contains(&e.date.date()));
        self.history.extend(week.entries.iter().map(|e| (e.id, e.clone())));
        self.recent = logic::recent_items(self.history.values());

        self.selected_entry = keep_selection(self.selected_entry, week.entries.len());
        self.selected_mileage = keep_selection(self.selected_mileage, week.mileage.len());
        self.entries = week.entries;
        self.mileage = week.mileage;
        // Per day in the order they were registered (outbound before return), so the Mileage tab
        // can group consecutive rows by day and ↑/↓ follow what's on screen.
        self.mileage.sort_by_key(|m| (m.date, m.id));
        if let Some(id) = select_id {
            if let Some(i) = self.entries.iter().position(|e| e.id == id) {
                self.selected_entry = Some(i);
            }
            if let Some(i) = self.mileage.iter().position(|m| m.id == id) {
                self.selected_mileage = Some(i);
            }
        }
        self.fix_selection();
    }

    fn load_report(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.as_ref()) else { return };
        let (user_id, month) = (me.id, self.report_month);
        self.run(
            "Loading…",
            async move { logic::load_report(&client, user_id, month).await },
            window,
            cx,
            |this, result, window, cx| match result {
                // Only keep it if the user didn't move on to another month meanwhile.
                Ok(report) if report.month == Some(this.report_month) => {
                    this.report = report;
                    this.error = None;
                }
                Ok(_) => {}
                Err(e) => this.set_error(format!("Error: {e}"), window, cx),
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
        self.tabs.update(cx, |t, cx| t.set_selected(tab.id().into(), cx));
        if tab == Tab::Report && self.report.month != Some(self.report_month) {
            self.load_report(window, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Moves the selected day; loads data only when the week changes.
    pub fn shift_day(&mut self, days: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.select_day(self.day + Duration::days(days), window, cx);
    }

    /// The ‹/› buttons: a day on the Week tab, a week on the Mileage tab.
    pub fn step(&mut self, direction: i64, window: &mut Window, cx: &mut Context<Self>) {
        let by_day = self.tab == Tab::Week;
        self.shift_day(if by_day { direction } else { 7 * direction }, window, cx);
    }

    pub fn select_day(&mut self, day: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        self.day = day;
        let week_start = logic::monday_of(day);
        if week_start != self.week_start {
            self.week_start = week_start;
            self.load_week(None, window, cx);
        }
        self.fix_selection();
        cx.notify();
    }

    pub fn go_today(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Report {
            self.report_month = logic::first_of_month(logic::today());
            self.load_report(window, cx);
        } else {
            self.select_day(logic::today(), window, cx);
        }
    }

    pub fn shift_month(&mut self, months: i32, window: &mut Window, cx: &mut Context<Self>) {
        let delta = Months::new(months.unsigned_abs());
        self.report_month = if months < 0 { self.report_month - delta } else { self.report_month + delta };
        self.load_report(window, cx);
        cx.notify();
    }

    /// Indices into `entries` of the rows currently shown on the Week tab.
    pub fn visible_entries(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.entries[i].date.date() == self.day).collect()
    }

    /// Moves the selection by `delta` rows within what's visible on the current tab.
    fn move_selection(&mut self, delta: isize) {
        if self.tab == Tab::Mileage {
            let len = self.mileage.len();
            if len > 0 {
                self.selected_mileage = Some(self.selected_mileage.map_or(0, |i| i.saturating_add_signed(delta).min(len - 1)));
            }
            return;
        }
        let visible = self.visible_entries();
        let Some(last) = visible.len().checked_sub(1) else { return };
        let pos = self.selected_entry.and_then(|s| visible.iter().position(|&i| i == s));
        let pos = pos.map_or(0, |p| p.saturating_add_signed(delta).min(last));
        self.selected_entry = Some(visible[pos]);
    }

    /// Keeps the selection on a visible entry (the first one of the day if it isn't).
    fn fix_selection(&mut self) {
        let visible = self.visible_entries();
        if !self.selected_entry.is_some_and(|s| visible.contains(&s)) {
            self.selected_entry = visible.first().copied();
        }
    }

    pub fn selected_entry(&self) -> Option<&TimeEntry> {
        self.entries.get(self.selected_entry?)
    }

    pub fn selected_mileage(&self) -> Option<&Mileage> {
        self.mileage.get(self.selected_mileage?)
    }

    /// Clicking a row: select it; a double click opens it for editing.
    pub fn click_entry(&mut self, index: usize, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_entry = Some(index);
        if let Some(day) = self.entries.get(index).map(|e| e.date.date()) {
            self.day = day;
        }
        window.focus(&self.focus, cx);
        if clicks >= 2 {
            self.edit_selected(window, cx);
        }
        cx.notify();
    }

    pub fn click_mileage(&mut self, index: usize, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_mileage = Some(index);
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
        if self.user_menu || self.show_detail {
            self.user_menu = false;
            self.show_detail = false;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let ks = &event.keystroke;
        let (shift, ctrl) = (ks.modifiers.shift, ks.modifiers.control || ks.modifiers.platform);
        let key = ks.key.as_str();
        let week_tab = self.tab == Tab::Week;
        let by_day = week_tab;
        match (key, ctrl) {
            ("q", true) => cx.quit(),
            (_, true) => return,
            ("1" | "2" | "3" | "4", _) => self.switch_tab(key.parse::<usize>().expect("digit") - 1, window, cx),
            ("r", _) => self.reload(false, window, cx),
            ("l", _) if shift => self.logout(window, cx),
            ("left" | "h", _) if self.tab == Tab::Report => self.shift_month(-1, window, cx),
            ("right" | "l", _) if self.tab == Tab::Report => self.shift_month(1, window, cx),
            ("t", _) if self.tab != Tab::Projects => self.go_today(window, cx),
            (_, _) if !matches!(self.tab, Tab::Week | Tab::Mileage) => return,
            // ←/→ move a day on the Week tab, a week on Mileage; Shift+←/→ and [ ] always a week.
            ("left" | "h", _) if by_day && !shift => self.shift_day(-1, window, cx),
            ("right" | "l", _) if by_day && !shift => self.shift_day(1, window, cx),
            ("left" | "h" | "[", _) => self.shift_day(-7, window, cx),
            ("right" | "l" | "]", _) => self.shift_day(7, window, cx),
            ("down" | "j", _) => self.move_selection(1),
            ("up" | "k", _) => self.move_selection(-1),
            ("enter", _) if week_tab && self.selected_entry().is_some() => self.show_detail = true,
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
        if self.me.is_none() {
            return;
        }
        if self.tab == Tab::Mileage {
            self.open_mileage_form(None, window, cx);
        } else {
            self.open_time_form(None, false, window, cx);
        }
    }

    pub fn edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == Tab::Mileage {
            if let Some(trip) = self.selected_mileage().cloned() {
                self.open_mileage_form(Some(Box::new(trip)), window, cx);
            }
        } else if let Some(entry) = self.selected_entry().cloned() {
            self.open_time_form(Some(&entry), false, window, cx);
        }
    }

    /// Copies the selected entry as a new one on the selected day.
    pub fn copy_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.selected_entry().cloned() {
            self.open_time_form(Some(&entry), true, window, cx);
        }
    }

    fn project_options(&self) -> Vec<(i64, String)> {
        self.projects.iter().filter(|p| p.active).map(|p| (p.id, format!("{} — {}", p.name, p.customer_name))).collect()
    }

    fn task_options(&self, project_id: Option<i64>, entry: Option<&TimeEntry>) -> Vec<(i64, String)> {
        let mut tasks: Vec<(i64, String)> = project_id
            .and_then(|id| self.project_tasks.get(&id))
            .map(|ts| ts.iter().filter(|t| t.active).map(|t| (t.id, t.name.clone())).collect())
            .unwrap_or_default();
        if let Some(e) = entry.filter(|e| Some(e.project_id) == project_id) {
            if !tasks.iter().any(|(id, _)| *id == e.project_task_id) {
                tasks.push((e.project_task_id, e.task_name.clone()));
            }
        }
        tasks
    }

    /// Form for a new time entry, for editing `entry`, or (with `copy`) for a copy of it.
    fn open_time_form(&mut self, entry: Option<&TimeEntry>, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut projects = self.project_options();
        if let Some(e) = entry {
            if !projects.iter().any(|(id, _)| *id == e.project_id) {
                projects.push((e.project_id, format!("{} — {}", e.project_name, e.customer_name)));
            }
        }
        // New entries default to the project/task/tag of the most recently modified entry.
        let template = entry.or_else(|| self.entries.iter().max_by_key(|e| e.modified));
        let project_id = template.map(|e| e.project_id).or(projects.first().map(|(id, _)| *id));
        let tasks = self.task_options(project_id, entry);

        let tags = self.tag_checkboxes(project_id, template, cx);
        let kept_tag_ids = entry.map(|e| self.unconfigured_tags(&e.tag_ids)).unwrap_or_default();

        let editing = entry.filter(|_| !copy);
        let date = editing.map(|e| e.date.date()).unwrap_or(self.day);
        let (start, end) = editing
            .and_then(|e| e.start_end.as_deref())
            .and_then(|s| s.split_once('-'))
            .map(|(s, e)| (s.to_owned(), e.to_owned()))
            .unwrap_or_else(|| (self.next_start_time(date).unwrap_or_default(), String::new()));
        let hours = editing.map(|e| format!("{}", e.hours)).unwrap_or_default();
        let notes = entry.and_then(|e| e.notes.clone()).unwrap_or_default();

        let project = forms::choice(cx, "form-project", "Project", projects, project_id);
        let recent = (entry.is_none() && !self.recent.is_empty())
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
            date: forms::text_input(cx, date.to_string()),
            task: forms::choice(cx, "form-task", "Task", tasks, template.map(|e| e.project_task_id)),
            start: forms::text_input(cx, start.clone()),
            end: forms::text_input(cx, end),
            hours: forms::text_input(cx, hours),
            notes: forms::text_input(cx, notes),
            tags,
            kept_tag_ids,
            project: project.clone(),
            recent,
        };
        // A new entry starts in the Recent filter; editing jumps to the times, a copy to the end
        // time when the start is already filled in.
        let focus = match &form.recent {
            Some(picker) => &picker.filter,
            None if editing.is_some() || start.is_empty() => &form.start,
            None => &form.end,
        };
        window.focus(&focus.read(cx).focus_handle(cx), cx);
        let mut subscriptions = vec![self.subscribe_project(&project, window, cx)];
        // `+15` / `+1h` / `+1h30` in End becomes start + that duration when leaving the field.
        subscriptions.push(cx.subscribe_in(&form.end, window, |this, end, event: &InputStateEvent, _, cx| {
            let Some(Form { body: FormBody::Time(form), .. }) = &this.form else { return };
            if !matches!(event, InputStateEvent::Blur) {
                return;
            }
            if let Some(expanded) = logic::expand_end(&text(&form.start, cx), &text(end, cx)) {
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
    pub fn recent_matches(&self, cx: &gpui::App) -> Vec<&logic::RecentItem> {
        let Some(Form { body: FormBody::Time(TimeForm { recent: Some(picker), .. }), .. }) = &self.form else {
            return Vec::new();
        };
        let query = text(&picker.filter, cx);
        if query.is_empty() {
            return Vec::new();
        }
        self.recent.iter().filter(|item| item.matches(&query)).take(RECENT_ROWS).collect()
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
        let mut projects = self.project_options();
        if !projects.iter().any(|(id, _)| *id == entry.project_id) {
            projects.push((entry.project_id, format!("{} — {}", entry.project_name, entry.customer_name)));
        }
        let project = forms::choice(cx, "form-project", "Project", projects, Some(entry.project_id));
        let tasks = self.task_options(Some(entry.project_id), Some(&entry));
        let task = forms::choice(cx, "form-task", "Task", tasks, Some(entry.project_task_id));
        let tags = self.tag_checkboxes(Some(entry.project_id), Some(&entry), cx);
        let kept_tag_ids = self.unconfigured_tags(&entry.tag_ids);
        let project_sub = self.subscribe_project(&project, window, cx);
        let Some(Form { body: FormBody::Time(form), _subscriptions, error }) = &mut self.form else { return };
        forms::set_text(&form.notes, entry.notes.clone().unwrap_or_default(), cx);
        form.project = project;
        form.task = task;
        form.tags = tags;
        form.kept_tag_ids = kept_tag_ids;
        _subscriptions[0] = project_sub;
        *error = None;
        let next = if text(&form.start, cx).is_empty() { &form.start } else { &form.end };
        window.focus(&next.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// The task list and the tag checkboxes follow the project.
    fn on_project_changed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(Form { body: FormBody::Time(form), .. }) = &self.form else { return };
        let project_id = chosen(&form.project, cx);
        let options = self.task_options(project_id, None);
        let task = forms::choice(cx, "form-task", "Task", options, None);
        // Pre-check the tags the project's most recently modified entry had.
        let last = self.entries.iter().filter(|e| Some(e.project_id) == project_id).max_by_key(|e| e.modified).cloned();
        let tags = self.tag_checkboxes(project_id, last.as_ref(), cx);
        if let Some(Form { body: FormBody::Time(form), .. }) = &mut self.form {
            form.task = task;
            form.tags = tags;
        }
        cx.notify();
    }

    /// Checkboxes for the tags configured for `project_id`, checked where `from` has the tag.
    fn tag_checkboxes(
        &self,
        project_id: Option<i64>,
        from: Option<&TimeEntry>,
        cx: &mut Context<Self>,
    ) -> Vec<forms::TagCheckbox> {
        let tags = project_id.and_then(|id| self.project_tags.get(&id)).cloned().unwrap_or_default();
        tags.into_iter()
            .map(|t| {
                let checked = from.is_some_and(|e| e.tag_ids.contains(&t.id));
                forms::tag_checkbox(cx, t.id, t.name, checked)
            })
            .collect()
    }

    /// The tags in `tag_ids` that no project configures; the form can't show them, so they're kept.
    fn unconfigured_tags(&self, tag_ids: &[i64]) -> Vec<i64> {
        tag_ids.iter().copied().filter(|id| !self.project_tags.values().flatten().any(|t| t.id == *id)).collect()
    }

    /// The trip form: empty for a new trip (project, type and billable from the last trip), or
    /// filled in with `editing`.
    fn open_mileage_form(&mut self, editing: Option<Box<Mileage>>, window: &mut Window, cx: &mut Context<Self>) {
        let template = editing.as_deref().or(self.mileage.last());
        let saved = self.saved_addresses();
        let types = MILEAGE_TYPES.iter().map(|(id, name)| (*id, name.to_string())).collect();
        let mut projects = self.project_options();
        if let Some(m) = &editing
            && !projects.iter().any(|(id, _)| *id == m.project_id)
        {
            projects.push((m.project_id, format!("{} — {}", m.project_name, m.customer_name)));
        }
        let (project, mileage_type, billable) = (
            template.map(|m| m.project_id),
            template.map(|m| m.mileage_type as i64),
            template.is_some_and(|m| m.billable),
        );
        let (date, from, to, distance, notes) = match &editing {
            Some(m) => (
                m.date.date(),
                m.from_address.clone().unwrap_or_default(),
                m.to_address.clone().unwrap_or_default(),
                format!("{}", m.distance),
                m.notes.clone().unwrap_or_default(),
            ),
            None => (
                self.day,
                saved.first().cloned().unwrap_or_default(),
                saved.get(1).cloned().unwrap_or_default(),
                String::new(),
                String::new(),
            ),
        };
        let form = MileageForm {
            editing,
            date: forms::text_input(cx, date.to_string()),
            project: forms::choice(cx, "form-project", "Project", projects, project),
            from: forms::text_input(cx, from),
            to: forms::text_input(cx, to),
            distance: forms::text_input(cx, distance),
            return_trip: forms::toggle(cx, "form-return", "Return trip", false),
            mileage_type: forms::choice(cx, "form-type", "Type", types, mileage_type),
            billable: forms::toggle(cx, "form-billable", "Billable", billable),
            notes: forms::text_input(cx, notes),
        };
        window.focus(&form.from.read(cx).focus_handle(cx), cx);
        self.form = Some(Form { body: FormBody::Mileage(form), error: None, _subscriptions: Vec::new() });
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

    /// F2 in an address field cycles through the user's saved addresses.
    pub fn cycle_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.saved_addresses();
        let Some(Form { body: FormBody::Mileage(form), .. }) = &self.form else { return };
        let field = [&form.from, &form.to].into_iter().find(|f| f.read(cx).focus_handle(cx).is_focused(window));
        if let (Some(field), false) = (field, saved.is_empty()) {
            let current = text(field, cx);
            let next = saved.iter().position(|a| *a == current).map_or(0, |i| (i + 1) % saved.len());
            forms::set_text(field, saved[next].clone(), cx);
        }
    }

    pub fn calculate_distance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Form { body: FormBody::Mileage(form), .. }), Some(client)) = (&self.form, self.client.clone()) else {
            return;
        };
        let (from, to) = (text(&form.from, cx), text(&form.to, cx));
        if from.is_empty() || to.is_empty() {
            return self.form_error("Fill in both addresses first", cx);
        }
        let future = async move {
            let lookup = |query: String| {
                let client = client.clone();
                async move {
                    client
                        .place_predictions(&query)
                        .await
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .next()
                        .ok_or_else(|| format!("Address not found: {query}"))
                }
            };
            let (from_place, to_place) = tokio::try_join!(lookup(from), lookup(to))?;
            let route =
                client.route_distance(&from_place.place_id, &to_place.place_id).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((from_place.description, to_place.description, route))
        };
        self.run("Calculating distance…", future, window, cx, |this, result, window, cx| match result {
            Ok((from, to, route)) => {
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
            Err(e) => this.form_error(e, cx),
        });
    }

    pub fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(client) = self.client.clone() else { return };
        let (date, request) = match self.build_save(cx) {
            Ok(r) => r,
            Err(message) => return self.form_error(message, cx),
        };
        let future = async move {
            let e = |e: timechimp::Error| e.to_string();
            match request {
                SaveRequest::Time { id: Some(id), entry } => {
                    Ok((client.update_time_entry(id, &entry).await.map_err(e)?.id, "Time entry updated."))
                }
                SaveRequest::Time { id: None, entry } => {
                    Ok((client.create_time_entry(&entry).await.map_err(e)?.id, "Time entry saved."))
                }
                SaveRequest::Mileage { trip, editing: Some(original), .. } => {
                    Ok((client.update_mileage(&original, &trip).await.map_err(e)?.id, "Trip updated."))
                }
                SaveRequest::Mileage { trip, return_trip, editing: None } => {
                    let id = client.create_mileage(&trip).await.map_err(e)?.id;
                    if !return_trip {
                        return Ok((id, "Mileage saved."));
                    }
                    let back = NewMileage { from_address: trip.to_address.clone(), to_address: trip.from_address.clone(), ..trip };
                    client.create_mileage(&back).await.map_err(e)?;
                    Ok((id, "Mileage saved (return trip included)."))
                }
            }
        };
        self.run("Saving…", future, window, cx, move |this, result: Result<(i64, &str), String>, window, cx| {
            match result {
                Ok((id, message)) => {
                    this.close_form(window, cx);
                    // Jump to the day of the saved item and select it.
                    this.day = date;
                    this.week_start = logic::monday_of(date);
                    this.report = Report::default();
                    this.load_week(Some(id), window, cx);
                    this.set_info(message, window, cx);
                }
                Err(e) => this.form_error(e, cx),
            }
        });
    }

    /// Validates the open form and turns it into an API request.
    fn build_save(&self, cx: &Context<Self>) -> Result<(NaiveDate, SaveRequest), String> {
        let (Some(form), Some(me)) = (&self.form, &self.me) else { return Err("Not logged in".into()) };
        let (date_input, project) = match &form.body {
            FormBody::Time(f) => (&f.date, &f.project),
            FormBody::Mileage(f) => (&f.date, &f.project),
        };
        let date = NaiveDate::parse_from_str(&text(date_input, cx), "%Y-%m-%d").map_err(|_| "Date must be YYYY-MM-DD")?;
        let project_id = chosen(project, cx).ok_or("Pick a project")?;
        let customer_id = self
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| p.customer_id)
            .or(self.entries.iter().find(|e| e.project_id == project_id).map(|e| e.customer_id))
            .ok_or("Unknown project")?;

        let request = match &form.body {
            FormBody::Mileage(f) => {
                let distance: f64 = text(&f.distance, cx)
                    .replace(',', ".")
                    .parse()
                    .map_err(|_| "Distance must be a number (Ctrl+D calculates it)")?;
                let (from, to) = (text(&f.from, cx), text(&f.to, cx));
                if from.is_empty() || to.is_empty() {
                    return Err("Fill in both addresses".into());
                }
                let notes = text(&f.notes, cx);
                SaveRequest::Mileage {
                    trip: NewMileage {
                        user_id: me.id,
                        customer_id,
                        project_id,
                        date,
                        distance,
                        from_address: from,
                        to_address: to,
                        notes: Some(notes).filter(|n| !n.is_empty()),
                        billable: f.billable.read(cx).is_on(),
                        mileage_type: chosen(&f.mileage_type, cx).unwrap_or(1) as i32,
                        mileage_vehicle_id: f.editing.as_ref().and_then(|m| m.mileage_vehicle_id),
                    },
                    return_trip: f.editing.is_none() && f.return_trip.read(cx).is_on(),
                    editing: f.editing.clone(),
                }
            }
            FormBody::Time(f) => {
                let project_task_id = chosen(&f.task, cx).ok_or("Pick a task")?;
                let task_id = self
                    .project_tasks
                    .get(&project_id)
                    .and_then(|ts| ts.iter().find(|t| t.id == project_task_id))
                    .map(|t| t.task_id)
                    .or(match &f.kind {
                        TimeKind::Edit { task: (pt, t), .. } if *pt == project_task_id => Some(*t),
                        _ => None,
                    })
                    .ok_or("Unknown task")?;
                let notes = text(&f.notes, cx);
                let mut entry = NewTimeEntry {
                    user_id: me.id,
                    customer_id,
                    project_id,
                    project_task_id,
                    task_id,
                    date,
                    hours: 0.0,
                    notes: Some(notes).filter(|n| !n.is_empty()),
                    start: None,
                    end: None,
                    pause: 0.0,
                    tag_ids: f
                        .kept_tag_ids
                        .iter()
                        .copied()
                        .chain(f.tags.iter().filter(|t| t.checkbox.read(cx).is_checked()).map(|t| t.tag_id))
                        .collect(),
                    timezone: Some(self.timezone()),
                };
                if let TimeKind::Edit { pause, timezone, .. } = &f.kind {
                    entry.pause = *pause;
                    entry.timezone = timezone.clone().or(entry.timezone);
                }

                let start = logic::parse_time(&text(&f.start, cx))?;
                let end = logic::parse_end(start, &text(&f.end, cx))?;
                let entry = match (start, end) {
                    (Some(start), Some(end)) => {
                        if end <= start {
                            return Err("End must be after start".into());
                        }
                        entry.with_times(logic::local(date, start)?, logic::local(date, end)?)
                    }
                    (None, None) => {
                        let hours: f64 = text(&f.hours, cx)
                            .replace(',', ".")
                            .parse()
                            .map_err(|_| "Enter start and end, or a number of hours")?;
                        if hours <= 0.0 {
                            return Err("Hours must be positive".into());
                        }
                        NewTimeEntry { hours, ..entry }
                    }
                    _ => return Err("Fill in both start and end, or neither".into()),
                };
                let id = match &f.kind {
                    TimeKind::Edit { id, .. } => Some(*id),
                    TimeKind::New => None,
                };
                SaveRequest::Time { id, entry }
            }
        };
        Ok((date, request))
    }

    // ---- Delete ----------------------------------------------------------------------------

    pub fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = if self.tab == Tab::Week {
            self.selected_entry().map(|e| {
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
            self.selected_mileage().map(|m| {
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
        let this = cx.weak_entity();
        let state = cx.new(|_| {
            DialogState::new(dialog("confirm-delete").confirm(question, label).confirm_label("Delete").on_confirm(
                move |window, cx| {
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
        self._subscriptions.push(sub);
        self.confirm = Some(state);
        cx.notify();
    }

    fn delete(&mut self, target: DeleteTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let future = async move {
            match target {
                DeleteTarget::Time(id) => client.delete_time_entry(id).await,
                DeleteTarget::Mileage(id) => client.delete_mileage(id).await,
            }
        };
        self.run("Deleting…", future, window, cx, |this, result, window, cx| match result {
            Ok(()) => {
                this.report = Report::default();
                this.load_week(None, window, cx);
                this.set_info("Deleted.", window, cx);
            }
            Err(e) => this.set_error(format!("Error: {e}"), window, cx),
        });
    }

    // ---- Helpers ---------------------------------------------------------------------------

    /// End time of the last entry on `date`, so a new entry continues where the day left off.
    fn next_start_time(&self, date: NaiveDate) -> Option<String> {
        self.entries
            .iter()
            .filter(|e| e.date.date() == date)
            .filter_map(|e| e.start_end.as_deref()?.split_once('-').map(|(_, end)| end.to_owned()))
            .max()
    }

    fn timezone(&self) -> String {
        self.entries.iter().find_map(|e| e.timezone.clone()).unwrap_or_else(|| logic::FALLBACK_TIMEZONE.to_owned())
    }

    fn saved_addresses(&self) -> Vec<String> {
        let mut addresses: Vec<String> = self
            .me
            .as_ref()
            .and_then(|m| m.extra.get("mileageAddresses"))
            .and_then(logic::json_strings)
            .unwrap_or_default();
        for m in &self.mileage {
            for a in [&m.from_address, &m.to_address].into_iter().flatten() {
                if !addresses.contains(a) {
                    addresses.push(a.clone());
                }
            }
        }
        addresses
    }

    /// Hours registered per day of the current week, Monday first.
    pub fn hours_per_day(&self) -> [f64; 7] {
        let mut totals = [0.0; 7];
        for e in &self.entries {
            let offset = (e.date.date() - self.week_start).num_days();
            if (0..7).contains(&offset) {
                totals[offset as usize] += e.hours;
            }
        }
        totals
    }

    /// Contracted hours for a day of the week (0 = Monday).
    pub fn target_for(&self, day: usize) -> f64 {
        self.me.as_ref().and_then(|m| m.user_availability.as_ref()).and_then(|a| a.for_day(day)).unwrap_or(0.0)
    }

    pub fn contract_hours(&self) -> f64 {
        self.me.as_ref().and_then(|m| m.contract_hours).unwrap_or(0.0)
    }
}

#[derive(Clone, Copy)]
enum DeleteTarget {
    Time(i64),
    Mileage(i64),
}

enum SaveRequest {
    Time { id: Option<i64>, entry: NewTimeEntry },
    /// `editing` is the trip being replaced; None creates `trip` (and its return with `return_trip`).
    Mileage { trip: NewMileage, return_trip: bool, editing: Option<Box<Mileage>> },
}

/// Rows the Recent picker shows at most.
const RECENT_ROWS: usize = 6;

fn keep_selection(selected: Option<usize>, len: usize) -> Option<usize> {
    match (len, selected) {
        (0, _) => None,
        (n, Some(i)) => Some(i.min(n - 1)),
        (_, None) => Some(0),
    }
}
