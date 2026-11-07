//! Rendering of the main window: login screen, the four tabs, popups and dialogs (`settings`). One file per
//! screen or tab; `widgets` holds the shared building blocks.

mod chrome;
mod login;
mod mileage;
mod popups;
mod projects;
mod report;
mod settings;
mod week;
mod widgets;

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder,
};
use gpuikit::elements::toast::ToastExt;
use gpuikit::theme::{ActiveTheme, Themeable};

use crate::app::{Screen, TimeChimpApp, ToggleShowcase};

impl Render for TimeChimpApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let login = matches!(self.screen, Screen::Login(_));
        // The aurora animates on the login screen and in showcase mode; the busy bar while loading.
        let started = self.fx.begin_frame(login || self.fx.showcase || self.busy().is_some(), window, cx);
        let t = self.fx.time(cx);
        let root = div()
            .id("root")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.bg())
            .text_color(theme.fg())
            .text_sm()
            .on_action(cx.listener(|this, _: &ToggleShowcase, _, cx| {
                this.fx.toggle_showcase();
                cx.notify();
            }))
            .when(login || self.fx.showcase, |d| {
                let strength = if login { 1.0 } else { 0.7 };
                d.child(div().absolute().inset_0().child(crate::fx::aurora(t, theme.accent(), theme.bg(), strength)))
            });
        let root = match &self.screen {
            Screen::Login(_) => root.child(self.render_login(&theme, t, cx)),
            Screen::Main => root.child(self.render_main(&theme, cx)),
        };
        root.when(self.fx.showcase, |d| {
            d.child(self.fx.hud(theme.fg(), theme.fg_muted(), theme.accent(), theme.surface()))
        })
        .children(self.confirm.as_ref().map(|(dialog, _)| dialog.clone()))
        .child(cx.toast_manager().clone())
        .child(self.fx.end_frame(started))
    }
}
