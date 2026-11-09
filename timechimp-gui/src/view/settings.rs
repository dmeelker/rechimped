use gpui::{
    Context, ElementId, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, StatefulInteractiveElement,
    Styled, div, prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::elements::button::button;
use gpuikit::elements::checkbox::{CheckState, checkbox_box};
use gpuikit::elements::text_field::{TextField, text_field};
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ControlSize, Themeable};

use crate::app::{Cancel, Confirm, TimeChimpApp};
use crate::forms::{ProjectTagsForm, ProjectVisibility, SettingsForm, SettingsPage};

use super::widgets::*;

impl TimeChimpApp {
    /// One window for all settings: the categories on the left, the chosen one's page on the right.
    pub(super) fn render_settings(&self, form: &SettingsForm, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let categories = SettingsPage::ALL.iter().map(|&page| {
            let selected = page == form.page;
            div()
                .id(ElementId::Name(format!("settings-page-{page:?}").into()))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(selected, |d| d.bg(theme.accent_bg()).font_weight(FontWeight::SEMIBOLD))
                .when(!selected, |d| d.hover(|s| s.bg(theme.surface())))
                .on_click(cx.listener(move |this, _, w, cx| this.show_settings_page(page, w, cx)))
                .child(page.title())
        });
        let sidebar = v_stack()
            .w(px(190.))
            .flex_none()
            .p(px(12.))
            .gap(px(2.))
            .bg(theme.surface_secondary())
            .border_r_1()
            .border_color(theme.border())
            // The card's own corner radius, which a child's background would otherwise poke out of.
            .rounded_l(px(10.))
            .child(
                div()
                    .px(px(10.))
                    .pt(px(8.))
                    .pb(px(12.))
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.accent())
                    .child("Settings"),
            )
            .children(categories);

        let content = match form.page {
            SettingsPage::General => self.render_general(form, theme, cx),
            SettingsPage::Projects => self.render_project_visibility(&form.projects, theme, cx),
            SettingsPage::ProjectTags => self.render_project_tags(&form.tags, theme, cx),
        };
        let footer = h_stack()
            .gap(px(8.))
            .items_center()
            .child(div().flex_1().min_w_0().when_some(form.error.clone(), |d, e| {
                d.p(px(8.)).rounded(px(6.)).bg(theme.danger().opacity(0.15)).text_color(theme.danger()).child(e)
            }))
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
            ));
        let page = v_stack()
            .flex_1()
            .min_w_0()
            .p(px(24.))
            .gap(px(14.))
            .child(
                v_stack()
                    .gap(px(4.))
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(form.page.title()))
                    .child(muted(theme, form.page.description())),
            )
            .child(v_stack().flex_1().min_h_0().child(content))
            .child(footer);

        let card = h_stack()
            .id("settings-card")
            .key_context("Modal")
            .track_focus(&form.focus)
            .moves_focus_on_tab()
            .on_action(cx.listener(|this, _: &Confirm, w, cx| this.save_settings(w, cx)))
            .on_action(cx.listener(|this, _: &Cancel, w, cx| this.close_overlay(w, cx)))
            // Only on the window itself: in a field Enter is `Confirm` already, on a control it's the control's.
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, w, cx| {
                let on_window = this.settings_form().is_some_and(|f| f.focus.is_focused(w));
                if on_window && event.keystroke.key == "enter" && !event.keystroke.modifiers.modified() {
                    this.save_settings(w, cx);
                }
            }))
            .h(px(540.))
            .items_stretch()
            .child(sidebar)
            .child(page);
        Self::modal(theme, "settings", 880., card, None)
    }

    fn render_general(&self, form: &SettingsForm, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = |label: &'static str, control: gpui::AnyElement| {
            h_stack()
                .gap(px(16.))
                .items_center()
                .child(div().w(px(130.)).flex_none().text_color(theme.fg_muted()).child(label))
                .child(div().flex_1().min_w_0().child(control))
        };
        v_stack()
            .gap(px(14.))
            .child(row("Work days", h_stack().gap(px(14.)).children(form.work_days.iter().cloned()).into_any_element()))
            .child(row(
                "Billable target",
                number_field(text_field(&form.billable_goal, cx).placeholder("80"), muted(theme, "%")),
            ))
            .into_any_element()
    }

    fn render_project_visibility(&self, p: &ProjectVisibility, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows = p.projects.iter().map(|(id, name, customer)| {
            let (id, shown) = (*id, !p.hidden.contains(id));
            h_stack()
                .id(ElementId::NamedInteger("settings-project".into(), id as u64))
                .flex_none()
                .px(px(8.))
                .py(px(4.))
                .gap(px(10.))
                .items_center()
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.surface_secondary()))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_project_hidden(id, cx)))
                .child(checkbox_box(if shown { CheckState::Checked } else { CheckState::Unchecked }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when(!shown, |d| d.text_color(theme.fg_muted()))
                        .child(name.clone()),
                )
                .child(muted(theme, customer.clone()).text_xs().flex_none())
        });
        let list = list_box(theme, "settings-projects")
            .p(px(6.))
            .when(p.projects.is_empty(), |d| d.child(muted(theme, "No projects.")))
            .children(rows);
        let shown = p.projects.len() - p.hidden.len();
        let set_all = |id: &'static str, label: &'static str, show: bool, disabled: bool| {
            pop(
                theme,
                button(id, label).disabled(disabled).on_click(cx.listener(move |this, _, _, cx| this.show_all_projects(show, cx))),
            )
        };
        v_stack()
            .flex_1()
            .min_h_0()
            .gap(px(8.))
            .child(
                h_stack()
                    .gap(px(8.))
                    .items_center()
                    .child(muted(theme, format!("{shown} of {} offered in the forms", p.projects.len())).flex_1().min_w_0())
                    .child(set_all("settings-projects-all", "Show all", true, p.hidden.is_empty()))
                    .child(set_all("settings-projects-none", "Hide all", false, shown == 0)),
            )
            .child(list)
            .into_any_element()
    }

    fn render_project_tags(&self, form: &ProjectTagsForm, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let projects = list_box(theme, "tag-projects").children(form.projects.iter().enumerate().map(
            |(i, (id, name, customer))| {
                let (selected, count) = (i == form.current, form.count(*id));
                h_stack()
                    .id(ElementId::NamedInteger("tag-project".into(), i as u64))
                    .flex_none()
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
            None => list_box(theme, "tag-choices").p(px(12.)).child(muted(theme, "No projects.")),
            Some(project) => list_box(theme, "tag-choices").p(px(8.)).children(form.tags.iter().map(|(tag_id, name)| {
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
        // Header rows as tall as a button, so both lists line up.
        let column = |header: gpui::Div, list: gpui::Stateful<gpui::Div>| {
            v_stack()
                .min_h_0()
                .gap(px(8.))
                .child(header.h(theme.control(ControlSize::Medium).height).items_center())
                .child(list)
        };
        let projects = column(h_stack().child(muted(theme, "Projects")), projects).w(px(240.)).flex_none();
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
        h_stack().flex_1().min_h_0().items_stretch().gap(px(12.)).child(projects).child(tags).into_any_element()
    }
}

/// A bordered list that fills the rest of its page and scrolls.
fn list_box(theme: &T, id: &'static str) -> gpui::Stateful<gpui::Div> {
    v_stack()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .border_1()
        .border_color(theme.border())
        .rounded(px(6.))
        .bg(theme.surface())
}

fn number_field(field: TextField, unit: gpui::Div) -> gpui::AnyElement {
    h_stack().gap(px(8.)).items_center().child(div().w(px(100.)).child(field)).child(unit).into_any_element()
}

impl TimeChimpApp {
    /// Like the chart: no fields, so any key, the backdrop or Close shuts it.
    pub(super) fn render_about(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let card = v_stack()
            .id("about-card")
            .p(px(24.))
            .gap(px(6.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).child("Rechimped"))
            .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
            .child(muted(theme, env!("CARGO_PKG_DESCRIPTION")))
            .child(
                h_stack().mt(px(16.)).justify_end().child(primary_button(
                    theme,
                    "about-close",
                    "Close",
                    false,
                    cx.listener(|this, _, w, cx| this.close_overlay(w, cx)),
                )),
            );
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, w, cx| this.close_overlay(w, cx));
        Self::modal(theme, "about", 400., card, Some(Box::new(close)))
    }
}
