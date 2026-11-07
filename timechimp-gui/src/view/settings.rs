//! The Settings and Project tags dialogs, opened from the menu under the user name.

use gpui::{Context, ElementId, FontWeight, InteractiveElement, StatefulInteractiveElement, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use gpuikit::elements::button::button;
use gpuikit::elements::checkbox::{CheckState, checkbox_box};
use gpuikit::elements::text_field::{TextField, text_field};
use gpuikit::a11y::FocusNavigation;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ControlSize, Themeable};

use crate::app::{Cancel, Confirm, TimeChimpApp};
use crate::forms::{ProjectTagsForm, SettingsForm};

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

impl TimeChimpApp {
    /// The Project tags dialog: the projects on the left, the tags of the selected one on the right.
    pub(super) fn render_project_tags(&self, form: &ProjectTagsForm, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let list = |id: &'static str| {
            v_stack()
                .id(id)
                .h(px(344.))
                .overflow_y_scroll()
                .border_1()
                .border_color(theme.border())
                .rounded(px(6.))
                .bg(theme.surface())
        };
        let projects = list("tag-projects").children(form.projects.iter().enumerate().map(
            |(i, (id, name, customer))| {
                let (selected, count) = (i == form.current, form.count(*id));
                h_stack()
                    .id(ElementId::NamedInteger("tag-project".into(), i as u64))
                    .px(px(10.))
                    .py(px(6.))
                    .gap(px(8.))
                    .items_center()
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme.accent_bg()))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.surface_secondary())))
                    .on_click(cx.listener(move |this, _, _, cx| this.show_project_tags_of(i, cx)))
                    .child(
                        v_stack()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().when(selected, |d| d.font_weight(FontWeight::SEMIBOLD)).child(name.clone()))
                            .child(muted(theme, customer.clone()).text_xs().truncate()),
                    )
                    .when(count > 0, |d| d.child(muted(theme, count.to_string()).flex_none()))
            },
        ));
        let tags = match form.current_project() {
            None => list("tag-choices").p(px(12.)).child(muted(theme, "No projects.")),
            Some(project) => list("tag-choices").p(px(8.)).children(form.tags.iter().map(|(tag_id, name)| {
                let (tag_id, checked) = (*tag_id, form.is_checked(project, *tag_id));
                h_stack()
                    .id(ElementId::NamedInteger("tag-choice".into(), tag_id as u64))
                    .flex_none()
                    .px(px(6.))
                    .py(px(4.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.surface_secondary()))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_project_tag(tag_id, cx)))
                    .child(checkbox_box(if checked { CheckState::Checked } else { CheckState::Unchecked }))
                    .child(name.clone())
            })),
        };
        let count = form.current_project().map_or(0, |p| form.count(p));
        let no_project = form.current_project().is_none();
        let set_all = |id: &'static str, label: &'static str, checked: bool, disabled: bool| {
            pop(
                theme,
                button(id, label)
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_all_project_tags(checked, cx))),
            )
        };
        // Each column: a header row as tall as a button, then its list, so both lists line up.
        let column = |header: gpui::Div, list: gpui::Stateful<gpui::Div>| {
            v_stack().gap(px(8.)).child(header.h(theme.control(ControlSize::Medium).height).items_center()).child(list)
        };
        let projects = column(h_stack().child(muted(theme, "Projects")), projects).w(px(280.)).flex_none();
        let title = form.projects.get(form.current).map_or_else(String::new, |(_, name, _)| format!("Tags for {name}"));
        let tags = column(
            h_stack()
                .gap(px(8.))
                .child(muted(theme, title).flex_1().min_w_0().truncate())
                .child(set_all("project-tags-all", "Select all", true, no_project || count == form.tags.len()))
                .child(set_all("project-tags-none", "Clear all", false, no_project || count == 0)),
            tags,
        )
        .flex_1()
        .min_w_0();
        let card = v_stack()
            .id("project-tags-card")
            .p(px(24.))
            .gap(px(14.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).child("Project tags"))
            .child(muted(theme, "The tags the time form offers as checkboxes, per project."))
            .child(h_stack().gap(px(12.)).child(projects).child(tags))
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
                        button("project-tags-cancel", "Cancel").on_click(cx.listener(|this, _, w, cx| this.close_overlay(w, cx))),
                    ))
                    .child(primary_button(
                        theme,
                        "project-tags-save",
                        "Save",
                        false,
                        cx.listener(|this, _, w, cx| this.save_project_tags(w, cx)),
                    )),
            );
        Self::modal(theme, "project-tags", 720., card, None)
    }
}
