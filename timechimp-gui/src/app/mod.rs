//! Application state, keyboard handling and the (async) actions. Rendering lives in `view/`; the
//! behaviour is split over the submodules, each with its own `impl TimeChimpApp` block.

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

use crate::forms::{Form, MileageForm, SettingsForm, TimeForm};
use crate::domain;
use crate::net;
use crate::session::Session;
use crate::store::token;
use crate::password::PasswordField;

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
            Tab::Week => "Time",
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
    /// Under the user name in the header (Settings, Log out).
    User,
    /// Under the ▾ next to "+ Add trip" (the trip presets).
    AddTrip,
    /// Under the ▾ next to "+ Add time" (the time presets).
    AddTime,
}

/// What covers the main screen. One at a time: opening one while another is open is refused
/// (`open_overlay`, and the form openers check `overlay_open`), and closing goes through
/// `close_overlay`, so the rules live in one place instead of one flag per popup.
#[derive(Default)]
pub enum Overlay {
    #[default]
    None,
    /// The selected entry's details (Enter on the Week tab).
    Detail,
    /// The billability chart (from the Week card).
    Chart(BillabilityChart),
    /// The time or trip form.
    Form(Box<Form>),
    /// The Settings dialog.
    Settings(SettingsForm),
    /// The delete confirmation, with the subscription that clears it when it closes.
    Confirm { dialog: Entity<DialogState>, _closed: Subscription },
}

pub struct TimeChimpApp {
    /// Focus target of the main (browse) view; its key handler drives the keyboard shortcuts.
    pub focus: FocusHandle,
    pub screen: Screen,
    /// The logged-in user's data; replaced as a whole on logout.
    pub session: Session,
    pub tab: Tab,
    pub tabs: Entity<Tabs>,
    /// What covers the main screen (form, dialog, popup), if anything.
    pub overlay: Overlay,
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

    /// Whether a form, dialog or popup covers the main screen.
    pub fn overlay_open(&self) -> bool {
        !matches!(self.overlay, Overlay::None)
    }

    /// Shows `overlay`, unless another one is open (then false and nothing changes). Closes an
    /// open dropdown either way.
    fn open_overlay(&mut self, overlay: Overlay, cx: &mut Context<Self>) -> bool {
        self.menu = None;
        cx.notify();
        if self.overlay_open() {
            return false;
        }
        self.overlay = overlay;
        true
    }

    /// Closes the open form, dialog or popup and gives the focus back to the main view.
    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Opens dropdown `menu`, or closes it when it is the one open.
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
}
