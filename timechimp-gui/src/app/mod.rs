//! Application state, keyboard handling and the (async) actions. Rendering lives in `view/`; the
//! behaviour is split over the submodules, each with its own `impl TimeChimpApp` block.

mod actions;
mod auth;
mod date_field;
mod delete;
mod loading;
mod nav;
mod save;
mod settings;
mod time_form;
mod trip_form;

pub use actions::*;

use std::future::Future;

use gpui::{AppContext, Context, Entity, FocusHandle, SharedString, Subscription, Window};
use gpuikit::elements::dialog::DialogState;
use gpuikit::elements::tabs::{TabChanged, Tabs, tab, tabs};
use gpuikit::elements::toast::ToastExt;
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
    /// Under the user name in the header (Settings, Log out).
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
    /// The open Settings dialog.
    pub settings: Option<SettingsForm>,
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
            settings: None,
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

    fn time_form(&self) -> Option<&TimeForm> {
        self.form.as_ref()?.time()
    }

    fn time_form_mut(&mut self) -> Option<&mut TimeForm> {
        self.form.as_mut()?.time_mut()
    }

    fn mileage_form(&self) -> Option<&MileageForm> {
        self.form.as_ref()?.trip()
    }

    fn mileage_form_mut(&mut self) -> Option<&mut MileageForm> {
        self.form.as_mut()?.trip_mut()
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
