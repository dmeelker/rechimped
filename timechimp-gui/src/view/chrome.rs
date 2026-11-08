use std::rc::Rc;

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Window, deferred, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::button::button;
use gpuikit::elements::icon_button::icon_button;
use gpuikit::elements::tooltip::tooltip;
use gpuikit::icons::Icons;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use crate::app::{Menu, Overlay, Tab, TimeChimpApp};
use crate::{domain, session};

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_main(&mut self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match self.session.me() {
            Some(me) => format!("{} @ {}", me.display_name, me.company_name),
            None => String::new(),
        };
        let header = h_stack()
            .w_full()
            .px(px(16.))
            .pt(px(10.))
            .gap(px(16.))
            .items_end()
            .border_b_1()
            .border_color(theme.border())
            // Pulled 1px down so the tab strip's rule lies on the header's border and reads as one line.
            .child(div().mb(px(-1.)).font_weight(FontWeight::MEDIUM).child(self.tabs.clone()))
            .child(div().flex_1())
            .child(div().relative().mb(px(6.)).child(self.render_user_button(theme, title, cx)).when(
                self.menu == Some(Menu::User),
                |d| d.child(self.render_user_menu(theme, cx)),
            ));

        let body: AnyElement = match self.tab {
            Tab::Week => self.render_day(theme, cx).into_any_element(),
            Tab::Travel => self.render_travel(theme, cx).into_any_element(),
            Tab::Report => self.render_report(theme).into_any_element(),
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
            .child(div().id("body").flex_1().min_h_0().flex().flex_col().px(px(16.)).pb(px(12.)).child(body))
            .child(self.render_footer(theme));

        let focus = self.focus.clone();
        // Painted first so it sits behind the panels.
        let disclaimer = div()
            .absolute()
            .right(px(24.))
            .bottom(px(40.))
            .text_xs()
            .text_color(theme.fg_muted())
            .opacity(0.6)
            .child("Rechimped is not affiliated with or endorsed by TimeChimp.");

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !focus.contains_focused(window, cx) {
                    window.focus(&focus, cx);
                }
            })
            .child(disclaimer)
            .child(browse)
            .when(self.menu.is_some(), |d| d.child(self.render_menu_backdrop(cx)))
            .when(matches!(self.overlay, Overlay::Detail), |d| d.child(self.render_detail(theme, cx)))
            .when_some(self.chart(), |d, chart| d.child(self.render_chart(chart, theme, cx)))
            .when_some(self.form(), |d, form| d.child(self.render_form(form, theme, cx)))
            .when_some(self.settings_form(), |d, form| d.child(self.render_settings(form, theme, cx)))
            .when_some(self.project_tags_form(), |d, form| d.child(self.render_project_tags(form, theme, cx)))
    }

    pub(super) fn render_toolbar(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let today = domain::time::today();
        let (today_label, at_today, prev_tip, next_tip, title, suffix) = match self.tab {
            Tab::Week | Tab::Travel => {
                let (range, week) = week_title(self);
                let start = self.session.week_start();
                let (at_today, prev_tip, next_tip) = if self.tab == Tab::Week {
                    (self.session.day() == today, "Previous week ([)", "Next week (])")
                } else {
                    (start <= today && today < start + chrono::Duration::days(7), "Previous week (←)", "Next week (→)")
                };
                let suffix = if self.tab == Tab::Travel && self.session.week_loaded() {
                    let km: f64 = self.session.travel().iter().map(|m| m.distance).sum();
                    format!("{week} · {} km", plain_number(km))
                } else {
                    week
                };
                ("Today", at_today, prev_tip, next_tip, range, Some(suffix))
            }
            Tab::Report => (
                "This month",
                self.session.report_month() == domain::time::first_of_month(today),
                "Previous month (←)",
                "Next month (→)",
                self.session.report_month().format("%B %Y").to_string(),
                None,
            ),
        };
        let today_button = button("period-today", today_label)
            .tooltip(tooltip(format!("{today_label} (T)")))
            .disabled(at_today)
            .on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.go_today(w, cx)));
        let arrows = h_stack()
            .child(nav_arrow(
                "period-prev",
                Icons::chevron_left(),
                prev_tip,
                cx.listener(|this, _: &ClickEvent, w, cx| this.step_period(-1, w, cx)),
            ))
            .child(nav_arrow(
                "period-next",
                Icons::chevron_right(),
                next_tip,
                cx.listener(|this, _: &ClickEvent, w, cx| this.step_period(1, w, cx)),
            ));
        let bar = h_stack()
            .w_full()
            .px(px(16.))
            .py(px(12.))
            .gap(px(8.))
            .items_center()
            .child(pop(theme, today_button))
            .child(arrows)
            .child(period_title(theme, title, suffix))
            .child(div().flex_1());
        match self.tab {
            Tab::Week | Tab::Travel => bar.child(self.render_add(theme, cx)),
            Tab::Report => bar.child(pop(
                theme,
                button("reload", "Reload").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.reload(w, cx))),
            )),
        }
        .text_color(theme.fg())
    }

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
            .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(Menu::User, cx)))
    }

    pub(super) fn render_add(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, kind) = match self.tab {
            Tab::Week => ("+ Add time", Menu::AddTime),
            _ => ("+ Add trip", Menu::AddTrip),
        };
        let main =
            primary_button(theme, "add", label, false, cx.listener(|this, _, w, cx| this.add(w, cx))).rounded_r(px(0.));
        let caret = primary_button(
            theme,
            "add-presets",
            "▾",
            false,
            cx.listener(move |this, _, _, cx| this.toggle_menu(kind, cx)),
        )
        .rounded_l(px(0.))
        .px(px(8.))
        .border_l_1()
        .border_color(gpui::white().opacity(0.35));
        div()
            .relative()
            .child(h_stack().child(main).child(caret))
            .when(self.menu == Some(kind), |d| d.child(self.render_presets_menu(theme, kind, cx)))
    }

    fn render_presets_menu(&self, theme: &T, kind: Menu, cx: &mut Context<Self>) -> impl IntoElement {
        // Each row's actions hold a copy of its preset, so they act on the one that was shown even
        // if config.toml is reread in between.
        type Act = Rc<dyn Fn(&mut TimeChimpApp, &mut Window, &mut Context<TimeChimpApp>)>;
        let (presets, empty): (Vec<(AnyElement, Act, Act)>, &str) = match kind {
            Menu::AddTime => (
                self.session
                    .time_presets
                    .iter()
                    .map(|p| {
                        let label = v_stack()
                            .child(session::time_preset_label(p).to_owned())
                            .child(div().text_xs().text_color(theme.fg_muted()).child(p.project.clone()))
                            .into_any_element();
                        let (open, delete) = (p.clone(), p.clone());
                        let open: Act = Rc::new(move |this, w, cx| this.open_time_preset(open.clone(), w, cx));
                        let delete: Act = Rc::new(move |this, w, cx| this.ask_delete_time_preset(delete.clone(), w, cx));
                        (label, open, delete)
                    })
                    .collect(),
                "No presets yet: use \"Save as preset\" in the time form",
            ),
            _ => (
                self.session
                    .trip_presets
                    .iter()
                    .map(|p| {
                        let (open, delete) = (p.clone(), p.clone());
                        let open: Act = Rc::new(move |this, w, cx| this.open_trip_preset(open.clone(), w, cx));
                        let delete: Act = Rc::new(move |this, w, cx| this.ask_delete_trip_preset(delete.clone(), w, cx));
                        (p.name.clone().into_any_element(), open, delete)
                    })
                    .collect(),
                "No presets yet: use \"Save as preset\" in the trip form",
            ),
        };
        let danger = theme.danger();
        let is_empty = presets.is_empty();
        let rows = presets.into_iter().enumerate().map(|(i, (label, open, delete))| {
            let remove = div()
                .id(ElementId::NamedInteger("preset-delete".into(), i as u64))
                .px(px(6.))
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(px(16.))
                .text_color(theme.fg_muted())
                .hover(move |s| s.text_color(danger))
                .child("×")
                .on_click(cx.listener(move |this, _, w, cx| delete(this, w, cx)));
            h_stack()
                .items_center()
                .gap(px(4.))
                .rounded(px(4.))
                .hover(|s| s.bg(theme.accent_bg()))
                .child(
                    menu_item(theme, ElementId::NamedInteger("preset".into(), i as u64), label)
                        .flex_1()
                        .on_click(cx.listener(move |this, _, w, cx| open(this, w, cx))),
                )
                .child(remove)
        });
        let menu = menu_panel(theme, "presets-menu")
            .max_h(px(420.))
            .overflow_y_scroll()
            .children(rows)
            .when(is_empty, |d| d.child(div().w(px(280.)).px(px(12.)).py(px(6.)).child(muted(theme, empty))));
        deferred(div().absolute().top_full().right_0().mt(px(4.)).child(menu)).with_priority(2)
    }

    pub(super) fn render_menu_backdrop(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
            this.close_menu(cx);
            cx.stop_propagation();
        });
        deferred(div().id("menu-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, close))
            .with_priority(1)
    }

    pub(super) fn render_user_menu(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = menu_panel(theme, "user-menu")
            .child(
                menu_item(theme, "user-menu-settings", "Settings…")
                    .on_click(cx.listener(|this, _, w, cx| this.open_settings(w, cx))),
            )
            .child(
                menu_item(theme, "user-menu-project-tags", "Project tags…")
                    .on_click(cx.listener(|this, _, w, cx| this.open_project_tags(w, cx))),
            )
            .child(menu_item(theme, "user-menu-logout", "Log out").on_click(cx.listener(|this, _, w, cx| this.logout(w, cx))));
        deferred(div().absolute().top_full().right_0().mt(px(4.)).child(menu)).with_priority(2)
    }

    /// The actions work on the selection, so only the selected row shows them.
    pub(super) fn row_actions(&self, selected: bool, cx: &mut Context<Self>) -> gpui::Div {
        let action = |id: &'static str, icon: gpui::Svg, tip: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            icon_button(id, icon)
                .box_size(gpui::rems(1.5))
                .icon_size(gpui::rems(0.875))
                .tooltip(tooltip(tip))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| f(this, window, cx)))
        };
        h_stack()
            .w(px(ROW_ACTIONS_WIDTH))
            .flex_none()
            .h(px(20.))
            .gap(px(2.))
            .pr(px(4.))
            .items_center()
            .justify_end()
            .when(selected, |d| {
                d.child(action("row-edit", Icons::pencil_1(), "Edit (E)", |this, w, cx| this.edit_selected(w, cx)))
                    .when(self.tab == Tab::Week, |d| {
                        d.child(action("row-copy", Icons::copy(), "Copy (C)", |this, w, cx| this.copy_selected(w, cx)))
                    })
                    .child(action("row-delete", Icons::trash(), "Delete (D)", |this, w, cx| this.ask_delete(w, cx)))
            })
    }

    pub(super) fn render_footer(&self, theme: &T) -> impl IntoElement {
        let left: AnyElement = match (self.busy(), &self.error) {
            (Some(busy), _) => div().text_color(theme.accent()).child(busy).into_any_element(),
            (None, Some(error)) => div().text_color(theme.danger()).child(error.clone()).into_any_element(),
            (None, None) => {
                let hints: Vec<gpui::Div> = match self.tab {
                    Tab::Week => vec![
                        hint(theme, "←/→", "day"),
                        hint(theme, "[ ]", "week"),
                        hint(theme, "T", "today"),
                        hint(theme, "A", "add"),
                        hint(theme, "E", "edit"),
                        hint(theme, "C", "copy"),
                        hint(theme, "D", "delete"),
                        hint(theme, "Enter", "details"),
                    ],
                    Tab::Travel => vec![
                        hint(theme, "←/→", "week"),
                        hint(theme, "T", "today"),
                        hint(theme, "A", "add"),
                        hint(theme, "E", "edit"),
                        hint(theme, "D", "delete"),
                    ],
                    Tab::Report => vec![hint(theme, "←/→", "month"), hint(theme, "T", "this month")],
                };
                h_stack()
                    .gap(px(16.))
                    .children(hints)
                    .child(hint(theme, "1–3", "tabs"))
                    .child(hint(theme, "R", "reload"))
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

fn menu_item(theme: &T, id: impl Into<ElementId>, label: impl IntoElement) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(12.))
        .py(px(6.))
        .rounded(px(4.))
        .cursor_pointer()
        .whitespace_nowrap()
        .hover(|s| s.bg(theme.accent_bg()))
        .child(label)
}
