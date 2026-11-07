use gpui::{
    Animation, AnimationExt, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::elements::text_field::text_field;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use timechimp::MfaKind;

use crate::app::{Confirm, Screen, TimeChimpApp};

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_login(&self, theme: &T, t: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let Screen::Login(login) = &self.screen else { unreachable!() };
        let accent = theme.accent();
        let label = |text: &'static str| div().text_xs().text_color(theme.fg_muted()).child(text);
        let message = match (self.busy(), &login.error) {
            (Some(busy), _) => div().text_color(accent).child(busy),
            (None, Some(error)) => div().text_color(theme.danger()).child(error.clone()),
            (None, None) => muted(theme, "Tab: switch field · Enter: log in"),
        };
        let mfa_label = login.mfa.as_ref().map(|m| if m.kind() == MfaKind::Sms { "SMS code" } else { "Authenticator code" });

        let card = v_stack()
                .id("login")
                .key_context("Modal")
                .moves_focus_on_tab()
                .on_action(cx.listener(|this, _: &Confirm, window, cx| this.login_confirm(window, cx)))
                .w(px(420.))
                .gap(px(12.))
                .p(px(28.))
                .rounded(px(10.))
                .bg(theme.surface().opacity(0.92))
                .child(
                    h_stack()
                        .gap(px(8.))
                        .items_center()
                        .child(div().size(px(14.)).rounded_full().bg(accent))
                        .child(div().text_lg().font_weight(FontWeight::BOLD).child("Rechimped")),
                )
                .child(muted(theme, "Log in to view and register your time."))
                .child(label("Email"))
                .child(text_field(&login.email, cx).placeholder("you@company.com"))
                .child(label("Password"))
                .child(login.password.clone())
                .when_some(mfa_label, |d, l| {
                    d.child(label(l)).child(text_field(&login.code, cx).placeholder("123456"))
                })
                .child(message)
                .child(
                    h_stack().justify_end().child(primary_button(
                        theme,
                        "login-button",
                        if login.mfa.is_some() { "Verify" } else { "Log in" },
                        self.busy().is_some(),
                        cx.listener(|this, _, window, cx| this.login_confirm_click(window, cx)),
                    )),
                );
        let card = crate::fx::glow_frame(t, accent, px(10.), card).with_animation(
            "login-enter",
            Animation::new(std::time::Duration::from_millis(700)).with_easing(gpui::ease_out_quint()),
            |el, d| el.opacity(d).mt(px(24. * (1. - d))),
        );
        div().size_full().flex().items_center().justify_center().child(card)
    }

    pub(super) fn login_confirm_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Login(login) = &self.screen else { return };
        if login.mfa.is_some() {
            self.login_confirm(window, cx);
        } else {
            self.login(window, cx);
        }
    }
}
