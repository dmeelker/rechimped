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
use gpuikit::input::InputState;
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
    pub day_view: bool,
    pub week_start: NaiveDate,
    pub entries: Vec<TimeEntry>,
    pub selected_entry: Option<usize>,
    pub mileage: Vec<Mileage>,
    pub selected_mileage: Option<usize>,
    pub report_month: NaiveDate,
    pub report: Report,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub project_tasks: HashMap<i64, Vec<ProjectTask>>,
    pub tags: Vec<Tag>,
    pub form: Option<Form>,
    pub confirm: Option<Entity<DialogState>>,
    pub show_detail: bool,
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
            day_view: true,
            week_start: logic::monday_of(today),
            entries: Vec::new(),
            selected_entry: None,
            mileage: Vec::new(),
            selected_mileage: None,
            report_month: logic::first_of_month(today),
            report: Report::default(),
            projects: Vec::new(),
            tasks: Vec::new(),
            project_tasks: HashMap::new(),
            tags: Vec::new(),
            form: None,
            confirm: None,
            show_detail: false,
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
        let future = async move {
            let base = logic::load_base(&client).await?;
            let week = logic::load_week(&client, base.me.id, week_start).await?;
            let report =
                if want_report { Some(logic::load_report(&client, base.me.id, report_month).await?) } else { None };
            timechimp::Result::Ok((base, week, report))
        };
        self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
            Ok((base, week, report)) => {
                this.project_tasks = base.project_tasks;
                this.me = Some(base.me);
                this.projects = base.projects;
                this.tasks = base.tasks;
                this.tags = base.tags;
                this.report = report.unwrap_or_default();
                if generation == this.week_generation {
                    this.apply_week(week, None);
                }
                this.error = None;
            }
            Err(e) if resuming => {
                this.to_login(Some(format!("Session expired or invalid, please log in again ({e})")), window, cx)
            }
            Err(e) => this.set_error(format!("Error: {e}"), window, cx),
        });
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
        self.selected_entry = keep_selection(self.selected_entry, week.entries.len());
        self.selected_mileage = keep_selection(self.selected_mileage, week.mileage.len());
        self.entries = week.entries;
        self.mileage = week.mileage;
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

    /// The ‹/› buttons: a day in the day view, a week otherwise.
    pub fn step(&mut self, direction: i64, window: &mut Window, cx: &mut Context<Self>) {
        let by_day = self.tab == Tab::Week && self.day_view;
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

    pub fn toggle_day_view(&mut self, cx: &mut Context<Self>) {
        self.day_view = !self.day_view;
        self.fix_selection();
        cx.notify();
    }

    /// Indices into `entries` of the rows currently shown on the Week tab.
    pub fn visible_entries(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| !self.day_view || self.entries[i].date.date() == self.day).collect()
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

    pub fn click_mileage(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_mileage = Some(index);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // ---- Keyboard --------------------------------------------------------------------------

    pub fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Main) || self.form.is_some() {
            return;
        }
        if self.show_detail {
            self.show_detail = false;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let ks = &event.keystroke;
        let (shift, ctrl) = (ks.modifiers.shift, ks.modifiers.control || ks.modifiers.platform);
        let key = ks.key.as_str();
        let week_tab = self.tab == Tab::Week;
        let by_day = week_tab && self.day_view;
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
            // ←/→ move a day in the day view, a week otherwise; Shift+←/→ and [ ] always a week.
            ("left" | "h", _) if by_day && !shift => self.shift_day(-1, window, cx),
            ("right" | "l", _) if by_day && !shift => self.shift_day(1, window, cx),
            ("left" | "h" | "[", _) => self.shift_day(-7, window, cx),
            ("right" | "l" | "]", _) => self.shift_day(7, window, cx),
            ("v", _) if week_tab => self.toggle_day_view(cx),
            ("down" | "j", _) => self.move_selection(1),
            ("up" | "k", _) => self.move_selection(-1),
            ("enter", _) if week_tab && self.selected_entry().is_some() => self.show_detail = true,
            ("a" | "n", _) => self.add(window, cx),
            ("e", _) if week_tab => self.edit_selected(window, cx),
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
            self.open_mileage_form(window, cx);
        } else {
            self.open_time_form(None, false, window, cx);
        }
    }

    pub fn edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.selected_entry().cloned() {
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

        let mut tags = vec![(0, "(none)".to_owned())];
        tags.extend(self.tags.iter().map(|t| (t.id, t.name.clone())));
        if let Some(e) = entry {
            for (id, name) in e.tag_ids.iter().zip(&e.tag_names) {
                if !tags.iter().any(|(t, _)| t == id) {
                    tags.push((*id, name.clone()));
                }
            }
        }

        let editing = entry.filter(|_| !copy);
        let date = editing.map(|e| e.date.date()).unwrap_or(self.day);
        let (start, end) = editing
            .and_then(|e| e.start_end.as_deref())
            .and_then(|s| s.split_once('-'))
            .map(|(s, e)| (s.to_owned(), e.to_owned()))
            .unwrap_or_else(|| (self.next_start_time(date).unwrap_or_default(), String::new()));
        let hours = editing.map(|e| format!("{}", e.hours)).unwrap_or_default();
        let notes = entry.and_then(|e| e.notes.clone()).unwrap_or_default();
        let extra_tags = entry.map(|e| e.tag_ids.len().saturating_sub(1)).unwrap_or(0);

        let project = forms::choice(cx, "form-project", "Project", projects, project_id);
        let form = TimeForm {
            kind: match editing {
                None => TimeKind::New,
                Some(e) => TimeKind::Edit {
                    id: e.id,
                    extra_tag_ids: e.tag_ids.iter().skip(1).copied().collect(),
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
            tag: forms::choice(cx, "form-tag", "Tag", tags, template.and_then(|e| e.tag_ids.first().copied())),
            extra_tags: if copy { 0 } else { extra_tags },
            project: project.clone(),
        };
        // Editing: jump to the times; new: to the end time when the start is already filled in.
        let focus = if editing.is_some() || start.is_empty() { &form.start } else { &form.end };
        window.focus(&focus.read(cx).focus_handle(cx), cx);
        let sub = cx.subscribe_in(&project, window, |this, _, _: &SelectChanged, window, cx| {
            this.on_project_changed(window, cx)
        });
        self.form = Some(Form { body: FormBody::Time(form), error: None, _subscriptions: vec![sub] });
        cx.notify();
    }

    /// The task list follows the project.
    fn on_project_changed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(Form { body: FormBody::Time(form), .. }) = &self.form else { return };
        let project_id = chosen(&form.project, cx);
        let options = self.task_options(project_id, None);
        let task = forms::choice(cx, "form-task", "Task", options, None);
        if let Some(Form { body: FormBody::Time(form), .. }) = &mut self.form {
            form.task = task;
        }
        cx.notify();
    }

    fn open_mileage_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.mileage.last();
        let saved = self.saved_addresses();
        let types = MILEAGE_TYPES.iter().map(|(id, name)| (*id, name.to_string())).collect();
        let (last_project, last_type, last_billable) =
            (last.map(|m| m.project_id), last.map(|m| m.mileage_type as i64), last.is_some_and(|m| m.billable));
        let form = MileageForm {
            date: forms::text_input(cx, self.day.to_string()),
            project: forms::choice(cx, "form-project", "Project", self.project_options(), last_project),
            from: forms::text_input(cx, saved.first().cloned().unwrap_or_default()),
            to: forms::text_input(cx, saved.get(1).cloned().unwrap_or_default()),
            distance: forms::text_input(cx, ""),
            return_trip: forms::toggle(cx, "form-return", "Return trip", false),
            mileage_type: forms::choice(cx, "form-type", "Type", types, last_type),
            billable: forms::toggle(cx, "form-billable", "Billable", last_billable),
            notes: forms::text_input(cx, ""),
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
                SaveRequest::Mileage { trip, return_trip } => {
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
                        mileage_vehicle_id: None,
                    },
                    return_trip: f.return_trip.read(cx).is_on(),
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
                    tag_ids: chosen(&f.tag, cx).filter(|id| *id != 0).into_iter().collect(),
                    timezone: Some(self.timezone()),
                };
                if let TimeKind::Edit { extra_tag_ids, pause, timezone, .. } = &f.kind {
                    let extra: Vec<i64> = extra_tag_ids.iter().copied().filter(|id| !entry.tag_ids.contains(id)).collect();
                    entry.tag_ids.extend(extra);
                    entry.pause = *pause;
                    entry.timezone = timezone.clone().or(entry.timezone);
                }

                let (start, end) = (logic::parse_time(&text(&f.start, cx))?, logic::parse_time(&text(&f.end, cx))?);
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
    Mileage { trip: NewMileage, return_trip: bool },
}

fn keep_selection(selected: Option<usize>, len: usize) -> Option<usize> {
    match (len, selected) {
        (0, _) => None,
        (n, Some(i)) => Some(i.min(n - 1)),
        (_, None) => Some(0),
    }
}
