//! The main screen around the tabs: header, user menu, toolbar, footer and the row actions.

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, deferred, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::button::{ButtonVariant, button};
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use crate::app::{Menu, Tab, TimeChimpApp};

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_main(&mut self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.session.me {
            Some(me) => format!("{} @ {}", me.display_name, me.company_name),
            None => String::new(),
        };
        let header = h_stack()
            .w_full()
            .px(px(16.))
            .pt(px(10.))
            .gap(px(16.))
            .items_center()
            .border_b_1()
            .border_color(theme.border())
            .child(self.tabs.clone())
            .child(div().flex_1())
            .child(div().relative().mb(px(6.)).child(self.render_user_button(theme, title, cx)).when(
                self.menu == Some(Menu::User),
                |d| d.child(self.render_user_menu(theme, cx)),
            ));

        let body: AnyElement = match self.tab {
            Tab::Week => self.render_day(theme, cx).into_any_element(),
            Tab::Mileage => self.render_mileage(theme, cx).into_any_element(),
            Tab::Report => self.render_report(theme).into_any_element(),
            Tab::Projects => self.render_projects(theme).into_any_element(),
        };

        let browse = v_stack()
            .id("browse")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|this, _: &gpuikit::a11y::FocusNext, window, cx| {
                this.switch_tab((this.tab.index() + 1) % Tab::ALL.len(), window, cx)
            }))
            .on_action(cx.listener(|this, _: &gpuikit::a11y::FocusPrevious, window, cx| {
                this.switch_tab((this.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len(), window, cx)
            }))
            .flex_1()
            .min_h_0()
            .child(header)
            .child(crate::fx::sweep_bar(self.fx.sweep, self.busy().is_some(), theme.accent()))
            .child(self.render_toolbar(theme, cx))
            // The body fills the rest of the window; tables scroll inside their panels (`table_rows`).
            .child(div().id("body").flex_1().min_h_0().flex().flex_col().px(px(16.)).pb(px(12.)).child(body))
            .child(self.render_footer(theme));

        let focus = self.focus.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !focus.contains_focused(window, cx) {
                    window.focus(&focus, cx);
                }
            })
            .child(browse)
            .when(self.menu.is_some(), |d| d.child(self.render_menu_backdrop(cx)))
            .when(self.show_detail, |d| d.child(self.render_detail(theme, cx)))
            .when_some(self.form.as_ref(), |d, form| d.child(self.render_form(form, theme, cx)))
    }

    pub(super) fn render_toolbar(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = |id: &'static str, label: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            button(id, label).on_click(cx.listener(move |this, _: &ClickEvent, window, cx| f(this, window, cx)))
        };
        let bar = h_stack().w_full().px(px(16.)).py(px(12.)).gap(px(8.)).items_center();
        match self.tab {
            Tab::Week | Tab::Mileage => {
                let week_tab = self.tab == Tab::Week;
                bar.child(pop(theme, nav("prev", "‹", |this, w, cx| this.step(-1, w, cx))))
                    .child(pop(theme, nav("today", "Today", |this, w, cx| this.go_today(w, cx))))
                    .child(pop(theme, nav("next", "›", |this, w, cx| this.step(1, w, cx))))
                    .child(div().pl(px(8.)).font_weight(FontWeight::SEMIBOLD).child(week_title(self)))
                    .child(div().flex_1())
                    .when(week_tab, |d| {
                        d.child(primary_button(theme, "add", "+ Add time", false, cx.listener(|this, _, w, cx| this.add(w, cx))))
                    })
                    .when(!week_tab, |d| d.child(self.render_add_trip(theme, cx)))
            }
            Tab::Report => bar
                .child(pop(theme, nav("prev-month", "‹", |this, w, cx| this.shift_month(-1, w, cx))))
                .child(pop(theme, nav("this-month", "This month", |this, w, cx| this.go_today(w, cx))))
                .child(pop(theme, nav("next-month", "›", |this, w, cx| this.shift_month(1, w, cx))))
                .child(div().pl(px(8.)).font_weight(FontWeight::SEMIBOLD).child(self.session.report_month.format("%B %Y").to_string()))
                .child(div().flex_1())
                .child(pop(theme, nav("reload", "Reload", |this, w, cx| this.reload(w, cx)))),
            Tab::Projects => bar
                .child(div().flex_1())
                .child(pop(theme, nav("reload", "Reload", |this, w, cx| this.reload(w, cx)))),
        }
        .text_color(theme.fg())
    }

    /// The user name in the header; a click opens the user menu.
    pub(super) fn render_user_button(&self, theme: &T, title: String, cx: &mut Context<Self>) -> impl IntoElement {
        h_stack()
            .id("user-button")
            .gap(px(8.))
            .items_center()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(self.menu == Some(Menu::User), |d| d.bg(theme.surface()))
            .hover(|s| s.bg(theme.surface()))
            .child(div().size(px(12.)).rounded_full().bg(theme.accent()))
            .child(muted(theme, title))
            .child(muted(theme, "▾"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.menu = if this.menu == Some(Menu::User) { None } else { Some(Menu::User) };
                cx.notify();
            }))
    }

    /// "+ Add trip" as a split button: the left part opens a blank trip form, the ▾ opens a
    /// dropdown with the trip presets (`render_add_trip_menu`).
    pub(super) fn render_add_trip(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.menu == Some(Menu::AddTrip);
        let main = primary_button(theme, "add", "+ Add trip", false, cx.listener(|this, _, w, cx| this.add(w, cx)))
            .rounded_r(px(0.));
        let caret = primary_button(
            theme,
            "add-trip-presets",
            "▾",
            false,
            cx.listener(|this, _, _, cx| {
                this.menu = if this.menu == Some(Menu::AddTrip) { None } else { Some(Menu::AddTrip) };
                cx.notify();
            }),
        )
        .rounded_l(px(0.))
        .px(px(8.))
        .border_l_1()
        .border_color(gpui::white().opacity(0.35));
        div()
            .relative()
            .child(h_stack().child(main).child(caret))
            .when(open, |d| d.child(self.render_add_trip_menu(theme, cx)))
    }

    /// The `[[trips]]` presets from `config.toml`, under the ▾ of "+ Add trip". Clicking one opens
    /// the trip form for the selected day, filled in; its × asks to delete it.
    pub(super) fn render_add_trip_menu(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let danger = theme.danger();
        let items = self.session.trip_presets.iter().enumerate().map(|(i, preset)| {
            let remove = div()
                .id(ElementId::NamedInteger("add-trip-preset-delete".into(), i as u64))
                .px(px(6.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(px(16.))
                .text_color(theme.fg_muted())
                .hover(move |s| s.text_color(danger))
                .child("×")
                .on_click(cx.listener(move |this, _, w, cx| {
                    this.menu = None;
                    this.ask_delete_preset(i, w, cx);
                }));
            h_stack()
                .items_center()
                .gap(px(4.))
                .rounded(px(4.))
                .hover(|s| s.bg(theme.accent_bg()))
                .child(
                    menu_item(theme, ElementId::NamedInteger("add-trip-preset".into(), i as u64), preset.name.clone())
                        .flex_1()
                        .on_click(cx.listener(move |this, _, w, cx| this.open_preset(i, w, cx))),
                )
                .child(remove)
        });
        let menu = menu_panel(theme, "add-trip-menu").children(items).when(self.session.trip_presets.is_empty(), |d| {
            d.child(
                div()
                    .max_w(px(260.))
                    .px(px(12.))
                    .py(px(6.))
                    .child(muted(theme, "No presets yet: use \"Save as preset\" in the trip form, or add [[trips]] to config.toml")),
            )
        });
        deferred(div().absolute().top_full().right_0().mt(px(4.)).child(menu)).with_priority(2)
    }

    /// Transparent, window-wide layer under an open dropdown: any click outside closes it.
    pub(super) fn render_menu_backdrop(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
            this.menu = None;
            cx.stop_propagation();
            cx.notify();
        });
        deferred(div().id("menu-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, close))
            .with_priority(1)
    }

    /// The dropdown under the user name (a child of its `relative()` wrapper).
    pub(super) fn render_user_menu(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = menu_panel(theme, "user-menu")
            .child(menu_item(theme, "user-menu-logout", "Log out").on_click(cx.listener(|this, _, w, cx| this.logout(w, cx))));
        deferred(div().absolute().top_full().right_0().mt(px(4.)).child(menu)).with_priority(2)
    }

    /// Buttons acting on the selected row, for the right of the Week/Mileage panel header.
    pub(super) fn selection_actions(&self, theme: &T, cx: &mut Context<Self>) -> AnyElement {
        let nav = |id: &'static str, label: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            button(id, label).on_click(cx.listener(move |this, _: &ClickEvent, window, cx| f(this, window, cx)))
        };
        let week_tab = self.tab == Tab::Week;
        let has = if week_tab { self.session.selected_entry().is_some() } else { self.session.selected_mileage().is_some() };
        h_stack()
            .gap(px(8.))
            .items_center()
            .font_weight(FontWeight::NORMAL)
            .child(pop(theme, nav("edit", "Edit", |this, w, cx| this.edit_selected(w, cx)).disabled(!has)))
            .when(week_tab, |d| {
                d.child(pop(theme, nav("copy", "Copy", |this, w, cx| this.copy_selected(w, cx)).disabled(!has)))
            })
            .child(pop(
                theme,
                nav("delete", "Delete", |this, w, cx| this.ask_delete(w, cx))
                    .variant(ButtonVariant::Destructive)
                    .disabled(!has),
            ))
            .into_any_element()
    }

    pub(super) fn render_footer(&self, theme: &T) -> impl IntoElement {
        let left: AnyElement = match (self.busy(), &self.error) {
            (Some(busy), _) => div().text_color(theme.accent()).child(busy).into_any_element(),
            (None, Some(error)) => div().text_color(theme.danger()).child(error.clone()).into_any_element(),
            (None, None) => {
                let hints: Vec<gpui::Div> = match self.tab {
                    Tab::Week => vec![
                        hint("←/→", "day"),
                        hint("[ ]", "week"),
                        hint("T", "today"),
                        hint("A", "add"),
                        hint("E", "edit"),
                        hint("C", "copy"),
                        hint("D", "delete"),
                        hint("Enter", "details"),
                    ],
                    Tab::Mileage => vec![
                        hint("←/→", "week"),
                        hint("T", "today"),
                        hint("A", "add"),
                        hint("E", "edit"),
                        hint("D", "delete"),
                    ],
                    Tab::Report => vec![hint("←/→", "month"), hint("T", "this month")],
                    Tab::Projects => vec![hint("Shift+L", "log out")],
                };
                h_stack()
                    .gap(px(12.))
                    .children(hints)
                    .child(hint("1–4", "tabs"))
                    .child(hint("R", "reload"))
                    .into_any_element()
            }
        };
        h_stack()
            .w_full()
            .px(px(16.))
            .py(px(6.))
            .border_t_1()
            .border_color(theme.border())
            .bg(theme.surface())
            .text_xs()
            .text_color(theme.fg_muted())
            .child(left)
    }
}

/// The box of a dropdown menu; its items are `menu_item`s.
fn menu_panel(theme: &T, id: &'static str) -> gpui::Stateful<gpui::Div> {
    v_stack()
        .id(id)
        .occlude()
        .min_w(px(160.))
        .p(px(4.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme.border())
        .bg(theme.surface())
        .shadow_lg()
        .text_color(theme.fg())
}

fn menu_item(theme: &T, id: impl Into<ElementId>, label: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(12.))
        .py(px(6.))
        .rounded(px(4.))
        .cursor_pointer()
        .whitespace_nowrap()
        .hover(|s| s.bg(theme.accent_bg()))
        .child(label.into())
}
