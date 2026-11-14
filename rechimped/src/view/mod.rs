mod cached;
mod chart;
mod chrome;
mod frame;
mod login;
mod travel;
mod popups;
mod report;
mod review;
mod settings;
mod week;
mod widgets;

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder,
};
use gpuikit::elements::toast::ToastExt;
use gpuikit::theme::{ActiveTheme, Themeable};

pub use cached::CachedViews;

use crate::app::{Overlay, Screen, TimeChimpApp, ToggleShowcase};

impl Render for TimeChimpApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let login = matches!(self.screen, Screen::Login(_));
        // Loads need no frames of their own here: the sweep bar is a gpui animation and asks for its own.
        let started = self.fx.begin_frame(login || self.fx.showcase, window, cx);
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
            Screen::Main => root.child(self.render_main(window, &theme, cx)),
        };
        let root = root.when(self.fx.showcase, |d| {
            d.child(self.fx.hud(theme.fg(), theme.fg_muted(), theme.accent(), theme.surface()))
        })
        .children(match &self.overlay {
            Overlay::Confirm { dialog, .. } => Some(dialog.clone()),
            Overlay::Settings(form) => form.discard.as_ref().map(|(dialog, _)| dialog.clone()),
            _ => None,
        })
        .child(cx.toast_manager().clone())
        .child(self.fx.end_frame(started, || self.perf_view()));
        self.frame(root.into_any_element(), window, &theme, cx)
    }
}

impl TimeChimpApp {
    /// What is on screen, for the perf log: `Login`, or the tab plus any overlay and open menu.
    fn perf_view(&self) -> String {
        if matches!(self.screen, Screen::Login(_)) {
            return "Login".into();
        }
        let overlay = match &self.overlay {
            Overlay::None => None,
            Overlay::Detail => Some("Detail"),
            Overlay::About => Some("About"),
            Overlay::Chart(_) => Some("Chart"),
            Overlay::Form(_) => Some("Form"),
            Overlay::Settings(_) => Some("Settings"),
            Overlay::Confirm { .. } => Some("Confirm"),
            Overlay::TravelReminder => Some("TravelReminder"),
        };
        let mut view = self.tab.label().to_string();
        for part in overlay.into_iter().chain(self.menu.map(|_| "Menu")) {
            view.push('+');
            view.push_str(part);
        }
        view
    }
}
