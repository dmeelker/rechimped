//! The Settings dialog, opened from the menu under the user name.

use gpui::{Context, FontWeight, InteractiveElement, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use gpuikit::elements::button::button;
use gpuikit::elements::text_field::{TextField, text_field};
use gpuikit::a11y::FocusNavigation;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use crate::app::{Cancel, Confirm, TimeChimpApp};
use crate::forms::SettingsForm;

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_settings(&self, form: &SettingsForm, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, control: gpui::AnyElement| {
            h_stack()
                .gap(px(16.))
                .items_center()
                .child(div().w(px(130.)).flex_none().text_color(theme.fg_muted()).child(label))
                .child(div().flex_1().min_w_0().child(control))
        };
        let card = v_stack()
            .id("settings-card")
            .key_context("Modal")
            .moves_focus_on_tab()
            .on_action(cx.listener(|this, _: &Confirm, w, cx| this.save_settings(w, cx)))
            .on_action(cx.listener(|this, _: &Cancel, w, cx| this.close_overlay(w, cx)))
            .p(px(24.))
            .gap(px(14.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).child("Settings"))
            .child(row(
                "Work days",
                h_stack().gap(px(14.)).children(form.work_days.iter().cloned()).into_any_element(),
            ))
            .child(row(
                "Billable target",
                number_field(text_field(&form.billable_goal, cx).placeholder("80"), muted(theme, "%")),
            ))
            .when_some(form.error.clone(), |d, e| {
                d.child(div().p(px(8.)).rounded(px(6.)).bg(theme.danger().opacity(0.15)).text_color(theme.danger()).child(e))
            })
            .child(
                h_stack()
                    .pt(px(8.))
                    .gap(px(8.))
                    .justify_end()
                    .child(pop(
                        theme,
                        button("settings-cancel", "Cancel").on_click(cx.listener(|this, _, w, cx| this.close_overlay(w, cx))),
                    ))
                    .child(primary_button(
                        theme,
                        "settings-save",
                        "Save",
                        false,
                        cx.listener(|this, _, w, cx| this.save_settings(w, cx)),
                    )),
            );
        Self::modal(theme, "settings", 660., card, None)
    }
}

/// A short number field with its unit after it.
fn number_field(field: TextField, unit: gpui::Div) -> gpui::AnyElement {
    h_stack().gap(px(8.)).items_center().child(div().w(px(100.)).child(field)).child(unit).into_any_element()
}
