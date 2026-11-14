mod actions;
mod auth;
mod chart;
mod date_field;
mod delete;
mod loading;
mod nav;
mod reminder;
mod review;
mod save;
mod settings;
mod time_form;
mod trip_form;

pub use actions::*;
pub use chart::BillabilityChart;

use std::future::Future;
use std::sync::Arc;

use gpui::{Context, Entity, FocusHandle, SharedString, Subscription, Window};
use gpuikit::elements::dialog::DialogState;
use gpuikit::elements::toast::ToastExt;
use gpuikit::input::InputState;
use timechimp_api::{Authenticator, Client, MfaChallenge};

use crate::forms::{Form, TravelForm, SettingsForm, TimeForm};
use crate::backend::Api;
use crate::domain;
use crate::net;
use crate::session::Session;
use crate::store::token;
use crate::password::PasswordField;
use crate::scroll::ScrollArea;

/// Writes (saves, deletes) are left to finish on logout and block another save; loads are neither.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskKind {
    Load,
    Write,
}

struct Task {
    id: u64,
    epoch: u64,
    kind: TaskKind,
    /// For display only.
    label: &'static str,
    abort: tokio::task::AbortHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Week,
    Travel,
    Review,
    Report,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Week, Tab::Travel, Tab::Review, Tab::Report];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Week => "Time",
            Tab::Travel => "Travel",
            Tab::Report => "Report",
            Tab::Review => "Review",
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    User,
    AddTrip,
    AddTime,
}

#[derive(Default)]
pub enum Overlay {
    #[default]
    None,
    Detail,
    About,
    Chart(BillabilityChart),
    Form(Box<Form>),
    Settings(Box<SettingsForm>),
    Confirm { dialog: Entity<DialogState>, _closed: Subscription },
    /// No trip registered today yet (`app/reminder.rs`).
    TravelReminder,
}

pub struct TimeChimpApp {
    /// `--mock`: the backend every session uses, so logging in needs no credentials.
    fixed_api: Option<Api>,
    pub focus: FocusHandle,
    pub screen: Screen,
    pub session: Session,
    pub tab: Tab,
    overlay: Overlay,
    menu: Option<Menu>,
    pub error: Option<String>,
    in_flight: Vec<Task>,
    tasks_started: u64,
    forms_opened: u64,
    settings_page: crate::forms::SettingsPage,
    /// The session epoch the travel reminder was last considered for: once per session.
    travel_reminder_checked: Option<u64>,
    pub fx: crate::fx::Fx,
    /// The title bar was pressed; the next mouse move starts a window move (`view::frame`).
    pub title_drag: bool,
    pub review_list: review::ReviewList,
    /// The Time, Travel and Report tables' rows and the presets dropdown.
    pub entry_scroll: ScrollArea,
    pub trip_scroll: ScrollArea,
    pub report_scroll: ScrollArea,
    pub menu_scroll: ScrollArea,
    /// Made on the first render, as they need the app's entity (`view::cached`).
    pub cached_views: Option<crate::view::CachedViews>,
}

impl TimeChimpApp {
    /// With `fixed_api`, every session runs against it and logging in needs no credentials.
    pub fn new(fixed_api: Option<Api>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut app = Self {
            fixed_api: fixed_api.clone(),
            focus: cx.focus_handle(),
            screen: Screen::Main,
            session: Session::new(None, domain::time::today()),
            tab: Tab::Week,
            overlay: Overlay::None,
            menu: None,
            error: None,
            in_flight: Vec::new(),
            tasks_started: 0,
            forms_opened: 0,
            settings_page: crate::forms::SettingsPage::General,
            travel_reminder_checked: None,
            fx: crate::fx::Fx::new(),
            title_drag: false,
            review_list: review::ReviewList::default(),
            entry_scroll: ScrollArea::default(),
            trip_scroll: ScrollArea::default(),
            report_scroll: ScrollArea::default(),
            menu_scroll: ScrollArea::default(),
            cached_views: None,
        };

        let stored = || token::load_token().map(|t| Arc::new(Client::new(Authenticator::from_refresh_token(t))) as Api);
        match fixed_api.or_else(stored) {
            Some(api) => app.open_session(api, window, cx),
            None => app.show_login(None, window, cx),
        }
        app
    }

    fn run<T: Send + 'static>(
        &mut self,
        label: &'static str,
        future: impl Future<Output = T> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.spawn_task(TaskKind::Load, label, future, window, cx, done);
    }

    /// Like `run`, for a request that changes data on the server.
    fn run_write<T: Send + 'static>(
        &mut self,
        label: &'static str,
        future: impl Future<Output = T> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.spawn_task(TaskKind::Write, label, future, window, cx, done);
    }

    fn spawn_task<T: Send + 'static>(
        &mut self,
        kind: TaskKind,
        label: &'static str,
        future: impl Future<Output = T> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let epoch = self.session.epoch;
        self.tasks_started += 1;
        let id = self.tasks_started;
        let handle = net::runtime().spawn(future);
        self.in_flight.push(Task { id, epoch, kind, label, abort: handle.abort_handle() });
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = handle.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.in_flight.retain(|t| t.id != id);
                match result {
                    _ if this.session.epoch != epoch => {}
                    Ok(value) => done(this, value, window, cx),
                    Err(e) => this.set_error(format!("Background task failed: {e}"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves and deletes are left to finish, so what the server receives doesn't depend on the timing.
    fn cancel_loads(&mut self) {
        for task in self.in_flight.iter().filter(|t| t.kind == TaskKind::Load) {
            task.abort.abort();
        }
    }

    /// What covers the main screen; it changes only through `open_overlay`, `close_overlay` and `reset_overlay`.
    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    /// The open dropdown; it changes only through `toggle_menu`, `close_menu`, and when an overlay opens or is reset.
    pub fn menu(&self) -> Option<Menu> {
        self.menu
    }

    pub fn overlay_open(&self) -> bool {
        !matches!(self.overlay, Overlay::None)
    }

    fn open_overlay(&mut self, overlay: Overlay, cx: &mut Context<Self>) -> bool {
        self.menu = None;
        cx.notify();
        if self.overlay_open() {
            return false;
        }
        self.overlay = overlay;
        true
    }

    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Drops any overlay and menu without handing focus back to the main view, for when the screen itself
    /// changes (back to login) and focus goes elsewhere.
    fn reset_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        self.menu = None;
        cx.notify();
    }

    pub fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
        // Each opening starts at the top.
        self.menu_scroll = ScrollArea::default();
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    pub fn form(&self) -> Option<&Form> {
        match &self.overlay {
            Overlay::Form(form) => Some(form.as_ref()),
            _ => None,
        }
    }

    fn form_mut(&mut self) -> Option<&mut Form> {
        match &mut self.overlay {
            Overlay::Form(form) => Some(form.as_mut()),
            _ => None,
        }
    }

    pub fn settings_form(&self) -> Option<&SettingsForm> {
        match &self.overlay {
            Overlay::Settings(form) => Some(form),
            _ => None,
        }
    }

    fn settings_form_mut(&mut self) -> Option<&mut SettingsForm> {
        match &mut self.overlay {
            Overlay::Settings(form) => Some(form),
            _ => None,
        }
    }

    pub fn chart(&self) -> Option<&BillabilityChart> {
        match &self.overlay {
            Overlay::Chart(chart) => Some(chart),
            _ => None,
        }
    }

    fn time_form(&self) -> Option<&TimeForm> {
        self.form()?.time()
    }

    fn time_form_mut(&mut self) -> Option<&mut TimeForm> {
        self.form_mut()?.time_mut()
    }

    fn travel_form(&self) -> Option<&TravelForm> {
        self.form()?.trip()
    }

    fn travel_form_mut(&mut self) -> Option<&mut TravelForm> {
        self.form_mut()?.trip_mut()
    }

    /// A save or delete is in flight.
    pub fn saving(&self) -> bool {
        self.in_flight.iter().any(|t| t.epoch == self.session.epoch && t.kind == TaskKind::Write)
    }

    fn new_form_id(&mut self) -> u64 {
        self.forms_opened += 1;
        self.forms_opened
    }

    pub fn busy(&self) -> Option<&'static str> {
        self.in_flight.iter().rev().find(|t| t.epoch == self.session.epoch).map(|t| t.label)
    }

    fn set_info(&mut self, message: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        cx.toast(message).success().show(window, cx);
    }

    /// Submitted and later entries are locked: true, after a toast, when the selected entry is one.
    fn refuse_locked(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let locked = match self.tab {
            Tab::Week => self.session.selected_entry().and_then(|e| domain::lock_status(e.status, e.status_extern)),
            Tab::Review => self.session.selected_review().and_then(|r| domain::lock_status(r.status, r.client_status)),
            Tab::Travel | Tab::Report => None,
        };
        if let Some(status) = locked {
            cx.toast(format!("{} entries can't be changed.", status.label())).info().show(window, cx);
        }
        locked.is_some()
    }

    /// config.toml failed to load: true, after an error toast, so nothing writes the defaults over it.
    fn refuse_config_write(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Err(e) = self.session.config_writable() else { return false };
        self.set_error(e, window, cx);
        true
    }

    fn set_error(&mut self, message: impl Into<String>, window: &mut Window, cx: &mut Context<Self>) {
        let message = message.into();
        cx.toast(message.clone()).destructive().show(window, cx);
        self.error = Some(message);
    }

    /// Only an auth error logs out (network or server trouble may recover; R retries); the stored
    /// token goes too, as it would fail the same way on the next start.
    fn api_error(&mut self, error: timechimp_api::Error, window: &mut Window, cx: &mut Context<Self>) {
        if error.is_auth() {
            token::delete_token();
            self.show_login(Some(format!("Session expired or invalid, please log in again ({error})")), window, cx);
        } else {
            self.set_error(format!("Error: {error}"), window, cx);
        }
    }

    fn form_api_error(&mut self, form_id: Option<u64>, error: timechimp_api::Error, window: &mut Window, cx: &mut Context<Self>) {
        if error.is_auth() {
            self.api_error(error, window, cx);
        } else if form_id.is_some() && self.form().map(|f| f.id) == form_id {
            self.form_error(error.to_string(), cx);
        } else {
            self.set_error(format!("Error: {error}"), window, cx);
        }
    }
}
