use gpui::{AppContext, Context, Focusable, Window};
use timechimp_api::{Authenticator, Client, LoginOutcome};

use crate::forms::{self, text};
use crate::domain;
use crate::session::Session;
use crate::store::token;
use crate::password::{PasswordEvent, PasswordField};

use super::{Login, Overlay, Screen, TimeChimpApp};

impl TimeChimpApp {
    pub(super) fn show_login(&mut self, error: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let email = self
            .session
            .me()
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

        self.cancel_loads();
        self.session = Session::new(None, domain::time::today());
        self.overlay = Overlay::None;
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
        if self.busy().is_some() {
            return;
        }
        let Screen::Login(login) = &mut self.screen else { return };
        let Some(challenge) = login.mfa.clone() else { return };
        let code: String = text(&login.code, cx).chars().filter(|c| !c.is_whitespace()).collect();
        forms::set_text(&login.code, "", cx);
        self.run("Logging in…", async move { challenge.respond(&code).await }, window, cx, |this, result, window, cx| {
            match result {
                Ok(auth) => this.logged_in(auth, window, cx),
                Err(e) => {
                    if let Screen::Login(login) = &mut this.screen {
                        let focus = if e.is_wrong_code() {
                            login.error = Some("Wrong code, please try again.".into());
                            login.code.read(cx).focus_handle(cx)
                        } else {
                            login.mfa = None;
                            login.error = Some(format!("{e}. Please log in again."));
                            login.password.read(cx).focus_handle(cx)
                        };
                        window.focus(&focus, cx);
                    }
                }
            }
        });
    }

    fn logged_in(&mut self, auth: Authenticator, window: &mut Window, cx: &mut Context<Self>) {
        let save_error = token::save_token(auth.refresh_token()).err();
        self.session = Session::new(Some(Client::new(auth)), domain::time::today());
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
}
