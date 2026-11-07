mod actions;
mod auth;
mod chart;
mod date_field;
mod delete;
mod loading;
mod nav;
mod save;
mod settings;
mod time_form;
mod trip_form;

pub use actions::*;
pub use chart::BillabilityChart;

use std::future::Future;

use gpui::{AppContext, Context, Entity, FocusHandle, SharedString, Subscription, Window};
use gpuikit::elements::dialog::DialogState;
use gpuikit::elements::tabs::{TabChanged, Tabs, tab, tabs};
use gpuikit::elements::toast::ToastExt;
use gpuikit::theme::ControlSize;
use gpuikit::traits::control_sized::ControlSized;
use gpuikit::input::InputState;
use timechimp::{Authenticator, Client, MfaChallenge};

use crate::forms::{Form, MileageForm, ProjectTagsForm, SettingsForm, TimeForm};
use crate::domain;
use crate::net;
use crate::session::Session;
use crate::store::token;
use crate::password::PasswordField;

const SAVING: &str = "Saving…";
const DELETING: &str = "Deleting…";

struct Task {
    id: u64,
    epoch: u64,
    label: &'static str,
    abort: tokio::task::AbortHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Week,
    Mileage,
    Report,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Week, Tab::Mileage, Tab::Report];

    pub fn id(self) -> &'static str {
        match self {
            Tab::Week => "week",
            Tab::Mileage => "mileage",
            Tab::Report => "report",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Week => "Time",
            Tab::Mileage => "Mileage",
            Tab::Report => "Report",
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
    Chart(BillabilityChart),
    Form(Box<Form>),
    Settings(SettingsForm),
    ProjectTags(ProjectTagsForm),
    Confirm { dialog: Entity<DialogState>, _closed: Subscription },
}

pub struct TimeChimpApp {
    pub focus: FocusHandle,
    pub screen: Screen,
    pub session: Session,
    pub tab: Tab,
    pub tabs: Entity<Tabs>,
    pub overlay: Overlay,
    pub menu: Option<Menu>,
    pub error: Option<String>,
    in_flight: Vec<Task>,
    tasks_started: u64,
    forms_opened: u64,
    pub fx: crate::fx::Fx,
    _subscriptions: Vec<Subscription>,
}

impl TimeChimpApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tabs = cx.new(|_| {
            Tab::ALL.iter().fold(tabs("main-tabs"), |t, tb| t.tab(tab(tb.id(), tb.label())))
                .selected(Tab::Week.id())
                .control_size(ControlSize::Large)
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
            overlay: Overlay::None,
            menu: None,
            error: None,
            in_flight: Vec::new(),
            tasks_started: 0,
            forms_opened: 0,
            fx: crate::fx::Fx::new(),
            _subscriptions: vec![tab_sub],
        };

        match token::load_token() {
            Some(token) => {
                app.session = Session::new(Some(Client::new(Authenticator::from_refresh_token(token))), domain::time::today());
                app.reload(window, cx);
                window.focus(&app.focus, cx);
            }
            None => app.show_login(None, window, cx),
        }
        app
    }

    /// Results for an older session (logged out meanwhile) are dropped, so none of it lands on screen.
    fn run<T: Send + 'static>(
        &mut self,
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
        self.in_flight.push(Task { id, epoch, label, abort: handle.abort_handle() });
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
        for task in self.in_flight.iter().filter(|t| t.label != SAVING && t.label != DELETING) {
            task.abort.abort();
        }
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

    pub fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
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

    pub fn project_tags_form(&self) -> Option<&ProjectTagsForm> {
        match &self.overlay {
            Overlay::ProjectTags(form) => Some(form),
            _ => None,
        }
    }

    fn project_tags_form_mut(&mut self) -> Option<&mut ProjectTagsForm> {
        match &mut self.overlay {
            Overlay::ProjectTags(form) => Some(form),
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

    fn mileage_form(&self) -> Option<&MileageForm> {
        self.form()?.trip()
    }

    fn mileage_form_mut(&mut self) -> Option<&mut MileageForm> {
        self.form_mut()?.trip_mut()
    }

    pub fn saving(&self) -> bool {
        self.in_flight.iter().any(|t| t.epoch == self.session.epoch && t.label == SAVING)
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

    fn set_error(&mut self, message: impl Into<String>, window: &mut Window, cx: &mut Context<Self>) {
        let message = message.into();
        cx.toast(message.clone()).destructive().show(window, cx);
        self.error = Some(message);
    }

    /// Only an auth error logs out (network or server trouble may recover; R retries); the stored
    /// token goes too, as it would fail the same way on the next start.
    fn api_error(&mut self, error: timechimp::Error, window: &mut Window, cx: &mut Context<Self>) {
        if error.is_auth() {
            token::delete_token();
            self.show_login(Some(format!("Session expired or invalid, please log in again ({error})")), window, cx);
        } else {
            self.set_error(format!("Error: {error}"), window, cx);
        }
    }

    fn form_api_error(&mut self, form_id: Option<u64>, error: timechimp::Error, window: &mut Window, cx: &mut Context<Self>) {
        if error.is_auth() {
            self.api_error(error, window, cx);
        } else if form_id.is_some() && self.form().map(|f| f.id) == form_id {
            self.form_error(error.to_string(), cx);
        } else {
            self.set_error(format!("Error: {error}"), window, cx);
        }
    }
}
