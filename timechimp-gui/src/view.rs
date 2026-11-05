//! Rendering of the main window: login screen, the four tabs, popups and dialogs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{Datelike, Duration, NaiveDate};
use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, deferred, div,
    prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::elements::button::{ButtonVariant, button};
use gpuikit::elements::kbd::kbd;
use gpuikit::elements::progress::{ProgressVariant, progress};
use gpuikit::elements::text_field::text_field;
use gpuikit::elements::toast::ToastExt;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ActiveTheme, Theme, Themeable};
use timechimp::TimeEntry;

use crate::app::{CalculateDistance, Cancel, Confirm, CycleAddress, Screen, Tab, TimeChimpApp};
use crate::forms::{Form, FormBody};
use crate::logic::mileage_type_name;

type T = Arc<Theme>;

impl Render for TimeChimpApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let root = div()
            .id("root")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.bg())
            .text_color(theme.fg())
            .text_sm();
        let root = match &self.screen {
            Screen::Login(_) => root.child(self.render_login(&theme, cx)),
            Screen::Main => root.child(self.render_main(&theme, window, cx)),
        };
        root.children(self.confirm.clone()).child(cx.toast_manager().clone())
    }
}

// ---- Small building blocks -------------------------------------------------------------------

fn hours_colour(theme: &T, hours: f64, target: f64) -> Hsla {
    if hours == 0.0 && target == 0.0 {
        theme.fg_muted()
    } else if hours >= target {
        theme.success()
    } else {
        theme.warning()
    }
}

/// A table cell with a fixed width (or flexible with `None`) that truncates its text.
fn cell(width: Option<f32>, content: impl Into<SharedString>) -> gpui::Div {
    let d = div().overflow_hidden().whitespace_nowrap().text_ellipsis().px(px(6.)).child(content.into());
    match width {
        Some(w) => d.w(px(w)).flex_none(),
        None => d.flex_1().min_w(px(40.)),
    }
}

fn right_cell(width: f32, content: impl Into<SharedString>) -> gpui::Div {
    cell(Some(width), content).flex().justify_end()
}

fn header_row(theme: &T, cells: impl IntoIterator<Item = gpui::Div>) -> gpui::Div {
    h_stack()
        .w_full()
        .py(px(6.))
        .px(px(4.))
        .bg(theme.surface())
        .border_b_1()
        .border_color(theme.border())
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.fg_muted())
        .children(cells)
}

/// A bordered panel with a title bar.
fn panel(theme: &T, title: impl IntoElement, right: Option<AnyElement>) -> gpui::Div {
    v_stack()
        .border_1()
        .border_color(theme.border())
        .rounded(px(6.))
        .overflow_hidden()
        .bg(theme.bg())
        .child(
            h_stack()
                .justify_between()
                .items_center()
                .px(px(10.))
                .py(px(8.))
                .border_b_1()
                .border_color(theme.border())
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .children(right),
        )
}

fn muted(theme: &T, text: impl Into<SharedString>) -> gpui::Div {
    div().text_color(theme.fg_muted()).child(text.into())
}

fn empty_message(theme: &T, text: &'static str) -> gpui::Div {
    div().w_full().py(px(32.)).flex().justify_center().text_color(theme.fg_muted()).child(text)
}

fn hint(key: &'static str, label: &'static str) -> gpui::Div {
    h_stack().gap(px(4.)).items_center().child(kbd(key)).child(label)
}

fn week_title(app: &TimeChimpApp) -> String {
    format!(
        "Week {} · {} – {}",
        app.week_start.iso_week().week(),
        app.week_start.format("%Y-%m-%d"),
        (app.week_start + Duration::days(6)).format("%Y-%m-%d"),
    )
}

// ---- Login ------------------------------------------------------------------------------------

impl TimeChimpApp {
    fn render_login(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let Screen::Login(login) = &self.screen else { unreachable!() };
        let accent = theme.accent();
        let label = |text: &'static str| div().text_xs().text_color(theme.fg_muted()).child(text);
        let message = match (self.busy, &login.error) {
            (Some(busy), _) => div().text_color(accent).child(busy),
            (None, Some(error)) => div().text_color(theme.danger()).child(error.clone()),
            (None, None) => muted(theme, "Tab: switch field · Enter: log in"),
        };
        let mfa_label = login.mfa.as_ref().map(|m| if m.kind() == "SMS_MFA" { "SMS code" } else { "Authenticator code" });

        div().size_full().flex().items_center().justify_center().child(
            v_stack()
                .id("login")
                .key_context("Modal")
                .moves_focus_on_tab()
                .on_action(cx.listener(|this, _: &Confirm, window, cx| this.login_confirm(window, cx)))
                .w(px(380.))
                .gap(px(10.))
                .p(px(24.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme.border())
                .bg(theme.surface())
                .child(
                    h_stack()
                        .gap(px(8.))
                        .items_center()
                        .child(div().size(px(14.)).rounded_full().bg(accent))
                        .child(div().text_lg().font_weight(FontWeight::BOLD).child("TimeChimp")),
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
                    h_stack().justify_end().child(
                        button("login-button", if login.mfa.is_some() { "Verify" } else { "Log in" })
                            
                            .disabled(self.busy.is_some())
                            .on_click(cx.listener(|this, _, window, cx| this.login_confirm_click(window, cx))),
                    ),
                ),
        )
    }

    fn login_confirm_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Login(login) = &self.screen else { return };
        if login.mfa.is_some() {
            self.login_confirm(window, cx);
        } else {
            self.login(window, cx);
        }
    }

    // ---- Main ---------------------------------------------------------------------------------

    fn render_main(&mut self, theme: &T, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match &self.me {
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
            .child(
                h_stack()
                    .gap(px(8.))
                    .items_center()
                    .pb(px(10.))
                    .child(div().size(px(12.)).rounded_full().bg(theme.accent()))
                    .child(div().font_weight(FontWeight::BOLD).child("TimeChimp"))
                    .child(muted(theme, title)),
            )
            .child(div().flex_1())
            .child(self.tabs.clone());

        let body: AnyElement = match self.tab {
            Tab::Week if self.day_view => self.render_day(theme, cx).into_any_element(),
            Tab::Week => self.render_week_grouped(theme, cx).into_any_element(),
            Tab::Mileage => self.render_mileage(theme, cx).into_any_element(),
            Tab::Report => self.render_report(theme).into_any_element(),
            Tab::Projects => self.render_projects(theme).into_any_element(),
        };

        let browse = v_stack()
            .id("browse")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|this, _: &gpuikit::a11y::FocusNext, window, cx| {
                this.switch_tab((this.tab_index() + 1) % Tab::ALL.len(), window, cx)
            }))
            .on_action(cx.listener(|this, _: &gpuikit::a11y::FocusPrevious, window, cx| {
                this.switch_tab((this.tab_index() + Tab::ALL.len() - 1) % Tab::ALL.len(), window, cx)
            }))
            .flex_1()
            .min_h_0()
            .child(header)
            .child(self.render_toolbar(theme, cx))
            .child(div().id("body").flex_1().min_h_0().overflow_y_scroll().px(px(16.)).pb(px(12.)).child(body))
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
            .when(self.show_detail, |d| d.child(self.render_detail(theme, cx)))
            .when_some(self.form.as_ref(), |d, form| d.child(self.render_form(form, theme, window, cx)))
    }

    fn tab_index(&self) -> usize {
        Tab::ALL.iter().position(|t| *t == self.tab).expect("tab in ALL")
    }

    fn render_toolbar(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = |id: &'static str, label: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            button(id, label).on_click(cx.listener(move |this, _: &ClickEvent, window, cx| f(this, window, cx)))
        };
        let bar = h_stack().w_full().px(px(16.)).py(px(10.)).gap(px(6.)).items_center();
        match self.tab {
            Tab::Week | Tab::Mileage => {
                let week_tab = self.tab == Tab::Week;
                bar.child(nav("prev", "‹", |this, w, cx| this.step(-1, w, cx)))
                    .child(nav("today", "Today", |this, w, cx| this.go_today(w, cx)))
                    .child(nav("next", "›", |this, w, cx| this.step(1, w, cx)))
                    .when(week_tab, |d| {
                        d.child(
                            button("view", if self.day_view { "Week view" } else { "Day view" })
                                
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_day_view(cx))),
                        )
                    })
                    .child(div().flex_1())
                    .when(week_tab, |d| {
                        let has = self.selected_entry().is_some();
                        d.child(nav("edit", "Edit", |this, w, cx| this.edit_selected(w, cx)).disabled(!has))
                            .child(nav("copy", "Copy", |this, w, cx| this.copy_selected(w, cx)).disabled(!has))
                    })
                    .child(
                        nav("delete", "Delete", |this, w, cx| this.ask_delete(w, cx))
                            .variant(ButtonVariant::Destructive)
                            .disabled(if week_tab { self.selected_entry().is_none() } else { self.selected_mileage().is_none() }),
                    )
                    .child(
                        nav("add", if week_tab { "+ Add time" } else { "+ Add trip" }, |this, w, cx| this.add(w, cx))
                            ,
                    )
            }
            Tab::Report => bar
                .child(nav("prev-month", "‹", |this, w, cx| this.shift_month(-1, w, cx)))
                .child(nav("this-month", "This month", |this, w, cx| this.go_today(w, cx)))
                .child(nav("next-month", "›", |this, w, cx| this.shift_month(1, w, cx)))
                .child(div().pl(px(8.)).font_weight(FontWeight::SEMIBOLD).child(self.report_month.format("%B %Y").to_string()))
                .child(div().flex_1())
                .child(nav("reload", "Reload", |this, w, cx| this.reload(false, w, cx))),
            Tab::Projects => bar
                .child(div().flex_1())
                .child(nav("reload", "Reload", |this, w, cx| this.reload(false, w, cx)))
                .child(
                    button("logout", "Log out")
                        
                        .on_click(cx.listener(|this, _, w, cx| this.logout_click(w, cx))),
                ),
        }
        .text_color(theme.fg())
    }

    fn logout_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.logout(window, cx);
    }

    fn render_footer(&self, theme: &T) -> impl IntoElement {
        let left: AnyElement = match (self.busy, &self.error) {
            (Some(busy), _) => div().text_color(theme.accent()).child(busy).into_any_element(),
            (None, Some(error)) => div().text_color(theme.danger()).child(error.clone()).into_any_element(),
            (None, None) => {
                let hints: Vec<gpui::Div> = match self.tab {
                    Tab::Week if self.day_view => vec![
                        hint("←/→", "day"),
                        hint("[ ]", "week"),
                        hint("T", "today"),
                        hint("V", "week view"),
                        hint("A", "add"),
                        hint("E", "edit"),
                        hint("C", "copy"),
                        hint("D", "delete"),
                        hint("Enter", "details"),
                    ],
                    Tab::Week => vec![
                        hint("←/→", "week"),
                        hint("T", "today"),
                        hint("V", "day view"),
                        hint("A", "add"),
                        hint("E", "edit"),
                        hint("C", "copy"),
                        hint("D", "delete"),
                        hint("Enter", "details"),
                    ],
                    Tab::Mileage => vec![hint("←/→", "week"), hint("T", "today"), hint("A", "add"), hint("D", "delete")],
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

    // ---- Week tab -----------------------------------------------------------------------------

    fn entry_header(theme: &T) -> gpui::Div {
        header_row(
            theme,
            [
                cell(Some(110.), "Time"),
                right_cell(60., "Hours"),
                cell(Some(160.), "Customer"),
                cell(None, "Project / Task"),
                cell(None, "Notes"),
                cell(Some(140.), "Tags"),
            ],
        )
    }

    fn entry_row(&self, theme: &T, index: usize, e: &TimeEntry, indent: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected_entry == Some(index);
        h_stack()
            .id(ElementId::NamedInteger("entry".into(), index as u64))
            .w_full()
            .py(px(6.))
            .px(px(4.))
            .border_b_1()
            .border_color(theme.border_subtle())
            .cursor_pointer()
            .when(!e.billable, |d| d.text_color(theme.fg_muted()))
            .when(selected, |d| d.bg(theme.accent_bg()))
            .when(!selected, |d| d.hover(|s| s.bg(theme.surface())))
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| this.click_entry(index, ev.click_count(), window, cx)))
            .child(cell(Some(110.), format!("{}{}", if indent { "   " } else { "" }, e.start_end.as_deref().unwrap_or("–"))))
            .child(right_cell(60., format!("{:.2}", e.hours)))
            .child(cell(Some(160.), e.customer_name.clone()))
            .child(cell(None, format!("{} / {}", e.project_name, e.task_name)))
            .child(cell(None, e.notes.clone().unwrap_or_default()))
            .child(cell(Some(140.), e.tag_names.join(", ")).text_color(theme.fg_muted()))
    }

    /// One day at a time: a strip with the week's days on top, the selected day's entries below.
    fn render_day(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let per_day = self.hours_per_day();
        let total: f64 = per_day.iter().sum();
        let contract = self.contract_hours();

        let strip = h_stack().gap(px(8.)).children((0..7).map(|i| {
            let day = self.week_start + Duration::days(i as i64);
            let (hours, target) = (per_day[i], self.target_for(i));
            let selected = day == self.day;
            let is_today = day == crate::logic::today();
            let fraction = if target > 0.0 { (hours / target).min(1.0) as f32 } else { 0.0 };
            v_stack()
                .id(ElementId::NamedInteger("day".into(), i as u64))
                .flex_1()
                .gap(px(4.))
                .p(px(10.))
                .rounded(px(6.))
                .border_1()
                .cursor_pointer()
                .border_color(if selected { theme.accent() } else { theme.border() })
                .bg(if selected { theme.accent_bg() } else { theme.surface() })
                .hover(|s| s.border_color(theme.accent()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.focus, cx);
                    this.select_day(day, window, cx)
                }))
                .child(
                    h_stack()
                        .justify_between()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(day.format("%a %d").to_string()))
                        .when(is_today, |d| d.child(div().text_xs().text_color(theme.accent()).child("today"))),
                )
                .child(div().text_lg().text_color(hours_colour(theme, hours, target)).child(format!("{hours:.2}h")))
                .child(progress(fraction).variant(if hours >= target { ProgressVariant::Success } else { ProgressVariant::Default }))
        }));

        let index = (self.day - self.week_start).num_days() as usize;
        let (hours, target) = (self.hours_per_day()[index], self.target_for(index));
        let visible = self.visible_entries();
        let title = h_stack()
            .gap(px(4.))
            .child(self.day.format("%A %-d %B %Y").to_string())
            .child("·")
            .child(div().text_color(hours_colour(theme, hours, target)).child(format!("{hours:.2}")))
            .child(format!("/ {target:.2}h"));
        let rows: Vec<AnyElement> =
            visible.iter().map(|&i| self.entry_row(theme, i, &self.entries[i], false, cx).into_any_element()).collect();

        v_stack()
            .gap(px(12.))
            .child(
                h_stack()
                    .justify_between()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{} · {total:.2} / {contract:.2}h", week_title(self))))
                    .child(muted(theme, "grey = non-billable").text_xs()),
            )
            .child(strip)
            .child(
                panel(theme, title, None).child(Self::entry_header(theme)).children(rows).when(
                    visible.is_empty() && self.busy.is_none(),
                    |d| d.child(empty_message(theme, "No time registered on this day. Press A to add.")),
                ),
            )
    }

    /// The whole week, grouped per day with a header row showing each day's total.
    fn render_week_grouped(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let per_day = self.hours_per_day();
        let total: f64 = per_day.iter().sum();
        let contract = self.contract_hours();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, &hours) in per_day.iter().enumerate() {
            let day = self.week_start + Duration::days(i as i64);
            let target = self.target_for(i);
            let day_entries: Vec<(usize, &TimeEntry)> =
                self.entries.iter().enumerate().filter(|(_, e)| e.date.date() == day).collect();
            // Days without entries are shown only if hours are contracted for them.
            if day_entries.is_empty() && target == 0.0 {
                continue;
            }
            rows.push(self.day_header(theme, day, hours, target).into_any_element());
            for (index, e) in day_entries {
                rows.push(self.entry_row(theme, index, e, true, cx).into_any_element());
            }
        }
        let empty = self.entries.is_empty() && self.busy.is_none();
        panel(
            theme,
            format!("{} · {total:.2} / {contract:.2}h", week_title(self)),
            Some(muted(theme, "grey = non-billable").text_xs().into_any_element()),
        )
        .child(Self::entry_header(theme))
        .children(rows)
        .when(empty, |d| d.child(empty_message(theme, "No time registered this week. Press A to add.")))
    }

    /// Day header row: date, total hours (coloured against the target) and a progress bar.
    fn day_header(&self, theme: &T, day: NaiveDate, hours: f64, target: f64) -> impl IntoElement {
        let colour = hours_colour(theme, hours, target);
        h_stack()
            .w_full()
            .py(px(6.))
            .px(px(4.))
            .items_center()
            .bg(theme.surface_secondary())
            .border_b_1()
            .border_color(theme.border())
            .child(cell(Some(110.), day.format("%a %d %b").to_string()).font_weight(FontWeight::BOLD))
            .child(right_cell(60., format!("{hours:.2}")).font_weight(FontWeight::BOLD).text_color(colour))
            .child(
                h_stack().px(px(6.)).gap(px(8.)).items_center().map(|d| {
                    if target > 0.0 {
                        d.child(
                            progress((hours / target).min(1.0) as f32)
                                .width(px(120.))
                                .variant(if hours >= target { ProgressVariant::Success } else { ProgressVariant::Default }),
                        )
                        .child(muted(theme, format!("of {target:.2}h")))
                    } else {
                        d.child(muted(theme, "no contracted hours"))
                    }
                }),
            )
    }

    // ---- Mileage tab --------------------------------------------------------------------------

    fn render_mileage(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let total: f64 = self.mileage.iter().map(|m| m.distance).sum();
        let rows = self.mileage.iter().enumerate().map(|(i, m)| {
            let selected = self.selected_mileage == Some(i);
            h_stack()
                .id(ElementId::NamedInteger("trip".into(), i as u64))
                .w_full()
                .py(px(6.))
                .px(px(4.))
                .border_b_1()
                .border_color(theme.border_subtle())
                .cursor_pointer()
                .when(!m.billable, |d| d.text_color(theme.fg_muted()))
                .when(selected, |d| d.bg(theme.accent_bg()))
                .when(!selected, |d| d.hover(|s| s.bg(theme.surface())))
                .on_click(cx.listener(move |this, _, window, cx| this.click_mileage(i, window, cx)))
                .child(cell(Some(70.), m.date.format("%a %d").to_string()))
                .child(right_cell(60., format!("{}", m.distance)))
                .child(cell(None, m.from_address.clone().unwrap_or_default()))
                .child(cell(None, m.to_address.clone().unwrap_or_default()))
                .child(cell(Some(150.), m.project_name.clone()))
                .child(cell(Some(110.), mileage_type_name(m.mileage_type)))
                .child(cell(None, m.notes.clone().unwrap_or_default()))
        });
        let empty = self.mileage.is_empty() && self.busy.is_none();
        panel(theme, format!("{} · {total:.0} km", week_title(self)), None)
            .child(header_row(
                theme,
                [
                    cell(Some(70.), "Day"),
                    right_cell(60., "km"),
                    cell(None, "From"),
                    cell(None, "To"),
                    cell(Some(150.), "Project"),
                    cell(Some(110.), "Type"),
                    cell(None, "Notes"),
                ],
            ))
            .children(rows)
            .when(empty, |d| d.child(empty_message(theme, "No mileage this week. Press A to add.")))
    }

    // ---- Report tab ---------------------------------------------------------------------------

    fn render_report(&self, theme: &T) -> impl IntoElement {
        let month = self.report_month.format("%B %Y").to_string();
        let report = &self.report;
        if report.month != Some(self.report_month) {
            return panel(theme, month, None).child(empty_message(theme, "Loading report…")).into_any_element();
        }

        // Group hours per project / task.
        let mut groups: BTreeMap<(String, String), (f64, bool)> = BTreeMap::new();
        for r in &report.time {
            let g = groups.entry((r.project.clone(), r.task.clone())).or_insert((0.0, r.billable));
            g.0 += r.time;
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));

        let total: f64 = report.time.iter().map(|r| r.time).sum();
        let billable: f64 = report.time.iter().filter(|r| r.billable).map(|r| r.time).sum();
        let km: f64 = report.mileage.iter().map(|r| r.distance).sum();
        let amount: f64 = report.mileage.iter().filter_map(|r| r.amount).sum();
        let days = report.time.iter().map(|r| r.date.date()).collect::<BTreeSet<_>>().len();
        let pct = |h: f64| h / total.max(f64::EPSILON) * 100.0;

        let rows = groups.iter().map(|((project, task), (hours, billable))| {
            h_stack()
                .w_full()
                .py(px(6.))
                .px(px(4.))
                .items_center()
                .border_b_1()
                .border_color(theme.border_subtle())
                .when(!billable, |d| d.text_color(theme.fg_muted()))
                .child(cell(None, project.clone()))
                .child(cell(None, task.clone()))
                .child(right_cell(70., format!("{hours:.2}")))
                .child(right_cell(50., format!("{:.0}%", pct(*hours))))
                .child(div().w(px(100.)).flex_none().px(px(6.)).child(progress((hours / total.max(f64::EPSILON)) as f32)))
        });
        let table = panel(theme, format!("{month} · hours per project / task"), None)
            .flex_1()
            .child(header_row(
                theme,
                [cell(None, "Project"), cell(None, "Task"), right_cell(70., "Hours"), right_cell(50., "%"), cell(Some(100.), "")],
            ))
            .children(rows)
            .when(groups.is_empty(), |d| d.child(empty_message(theme, "No time registered this month.")));

        let stat = |label: &'static str, value: String| {
            h_stack()
                .justify_between()
                .py(px(4.))
                .child(muted(theme, label))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(value))
        };
        let summary = panel(theme, "Summary", None).w(px(300.)).flex_none().child(
            v_stack()
                .p(px(12.))
                .child(stat("Total hours", format!("{total:.2}")))
                .child(stat("Billable", format!("{billable:.2} ({:.0}%)", pct(billable))))
                .child(stat("Non-billable", format!("{:.2}", total - billable)))
                .child(stat("Days registered", days.to_string()))
                .child(stat("Entries", report.time.len().to_string()))
                .child(div().h(px(1.)).my(px(8.)).bg(theme.border()))
                .child(stat("Trips", report.mileage.len().to_string()))
                .child(stat("Distance", format!("{km:.0} km")))
                .child(stat("Reimbursement", format!("€ {amount:.2}"))),
        );
        h_stack().items_start().gap(px(12.)).child(table).child(summary).into_any_element()
    }

    // ---- Projects tab -------------------------------------------------------------------------

    fn render_projects(&self, theme: &T) -> impl IntoElement {
        let projects = panel(theme, format!("My projects ({})", self.projects.len()), None)
            .flex_1()
            .child(header_row(
                theme,
                [cell(None, "Customer"), cell(None, "Project"), right_cell(60., "Tasks"), cell(Some(70.), "Active")],
            ))
            .children(self.projects.iter().map(|p| {
                let tasks = self.project_tasks.get(&p.id).map_or(0, |t| t.len());
                h_stack()
                    .w_full()
                    .py(px(6.))
                    .px(px(4.))
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .when(!p.active, |d| d.text_color(theme.fg_muted()))
                    .child(cell(None, p.customer_name.clone()))
                    .child(cell(None, p.name.clone()))
                    .child(right_cell(60., tasks.to_string()))
                    .child(cell(Some(70.), if p.active { "yes" } else { "no" }))
            }));
        let tasks = panel(theme, format!("Tasks ({})", self.tasks.len()), None)
            .w(px(360.))
            .flex_none()
            .child(header_row(theme, [cell(None, "Task"), cell(Some(80.), "Billable")]))
            .children(self.tasks.iter().map(|t| {
                h_stack()
                    .w_full()
                    .py(px(6.))
                    .px(px(4.))
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .when(!t.billable, |d| d.text_color(theme.fg_muted()))
                    .child(cell(None, t.name.clone()))
                    .child(cell(Some(80.), if t.billable { "yes" } else { "no" }))
            }));
        h_stack().items_start().gap(px(12.)).child(projects).child(tasks)
    }

    // ---- Popups -------------------------------------------------------------------------------

    /// A dimmed backdrop with a centred card, painted above everything else.
    fn modal(
        theme: &T,
        id: &'static str,
        width: f32,
        card: gpui::Stateful<gpui::Div>,
        on_backdrop: Option<Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App)>>,
    ) -> impl IntoElement {
        let card = card.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        deferred(
            div()
                .id(id)
                .when_some(on_backdrop, |d, f| d.on_mouse_down(MouseButton::Left, f))
                .absolute()
                .inset_0()
                .occlude()
                .bg(gpui::hsla(0., 0., 0., 0.3))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    card.w(px(width))
                        .max_h_full()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border())
                        .bg(theme.surface())
                        .shadow_lg(),
                ),
        )
        .with_priority(1)
    }

    fn render_detail(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(e) = self.selected_entry() else { return div().into_any_element() };
        let field = |label: &'static str, value: String| {
            h_stack().py(px(3.)).child(muted(theme, label).w(px(90.)).flex_none()).child(value)
        };
        let card = v_stack()
            .id("detail-card")
            .p(px(20.))
            .gap(px(2.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).mb(px(8.)).child("Entry"))
            .child(field("Date", e.date.format("%A %Y-%m-%d").to_string()))
            .child(field("Time", e.start_end.clone().unwrap_or_else(|| "-".into())))
            .child(field("Hours", format!("{:.2} (pause {:.2})", e.hours, e.pause)))
            .child(field("Customer", e.customer_name.clone()))
            .child(field("Project", e.project_name.clone()))
            .child(field("Task", e.task_name.clone()))
            .child(field("Billable", if e.billable { "yes" } else { "no" }.into()))
            .child(field("Tags", if e.tag_names.is_empty() { "-".into() } else { e.tag_names.join(", ") }))
            .child(field("Modified", e.modified.format("%Y-%m-%d %H:%M").to_string()))
            .child(field("Id", e.id.to_string()))
            .when_some(e.notes.clone().filter(|n| !n.is_empty()), |d, notes| {
                d.child(div().mt(px(10.)).p(px(10.)).rounded(px(6.)).bg(theme.bg()).child(notes))
            })
            .child(
                h_stack()
                    .mt(px(14.))
                    .gap(px(6.))
                    .justify_end()
                    .child(
                        button("detail-edit", "Edit").on_click(cx.listener(|this, _, w, cx| {
                            this.show_detail = false;
                            this.edit_selected(w, cx)
                        })),
                    )
                    .child(button("detail-close", "Close").on_click(cx.listener(
                        |this, _, w, cx| {
                            this.show_detail = false;
                            w.focus(&this.focus, cx);
                            cx.notify()
                        },
                    ))),
            );
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
            this.show_detail = false;
            cx.notify()
        });
        Self::modal(theme, "detail", 560., card, Some(Box::new(close))).into_any_element()
    }

    fn render_form(&self, form: &Form, theme: &T, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, control: AnyElement, help: Option<&'static str>| {
            h_stack()
                .gap(px(12.))
                .items_center()
                .child(div().w(px(110.)).flex_none().text_color(theme.fg_muted()).child(label))
                .child(div().flex_1().min_w_0().child(control))
                .child(div().w(px(150.)).flex_none().text_xs().text_color(theme.fg_muted()).children(help))
        };
        let field = |input, placeholder: &'static str, cx: &mut Context<Self>| {
            text_field(input, cx).placeholder(placeholder).into_any_element()
        };
        let rows: Vec<AnyElement> = match &form.body {
            FormBody::Time(f) => vec![
                row("Date", field(&f.date, "YYYY-MM-DD", cx), Some("YYYY-MM-DD")),
                row("Project", f.project.clone().into_any_element(), None),
                row("Task", f.task.clone().into_any_element(), None),
                row("Start", field(&f.start, "HH:MM", cx), Some("e.g. 9, 930, 12:30")),
                row("End", field(&f.end, "HH:MM", cx), Some("e.g. 17, 1730")),
                row("Hours", field(&f.hours, "e.g. 1.5", cx), Some("used without start/end")),
                row("Notes", field(&f.notes, "What did you work on?", cx), None),
                row(
                    "Tag",
                    f.tag.clone().into_any_element(),
                    (f.extra_tags > 0).then_some("other tags are kept"),
                ),
            ]
            .into_iter()
            .map(IntoElement::into_any_element)
            .collect(),
            FormBody::Mileage(f) => vec![
                row("Date", field(&f.date, "YYYY-MM-DD", cx), Some("YYYY-MM-DD")),
                row("Project", f.project.clone().into_any_element(), None),
                row("From", field(&f.from, "Address", cx), Some("F2: saved addresses")),
                row("To", field(&f.to, "Address", cx), Some("F2: saved addresses")),
                row(
                    "Distance (km)",
                    h_stack()
                        .gap(px(6.))
                        .child(div().flex_1().child(field(&f.distance, "km", cx)))
                        .child(
                            button("calc-distance", "Calculate")
                                .on_click(cx.listener(|this, _, w, cx| this.calculate_distance(w, cx))),
                        )
                        .into_any_element(),
                    Some("Ctrl+D: calculate"),
                ),
                row("", f.return_trip.clone().into_any_element(), None),
                row("Type", f.mileage_type.clone().into_any_element(), None),
                row("", f.billable.clone().into_any_element(), None),
                row("Notes", field(&f.notes, "", cx), None),
            ]
            .into_iter()
            .map(IntoElement::into_any_element)
            .collect(),
        };
        let _ = window;
        let card = v_stack()
            .id("form-card")
            .key_context("Modal")
            .moves_focus_on_tab()
            .on_action(cx.listener(|this, _: &Confirm, w, cx| this.submit_form(w, cx)))
            .on_action(cx.listener(|this, _: &Cancel, w, cx| this.close_form(w, cx)))
            .on_action(cx.listener(|this, _: &CalculateDistance, w, cx| this.calculate_distance(w, cx)))
            .on_action(cx.listener(|this, _: &CycleAddress, w, cx| this.cycle_address(w, cx)))
            .p(px(20.))
            .gap(px(10.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).child(form.title()))
            .children(rows)
            .when_some(form.error.clone(), |d, e| {
                d.child(div().p(px(8.)).rounded(px(6.)).bg(theme.danger().opacity(0.15)).text_color(theme.danger()).child(e))
            })
            .child(
                h_stack()
                    .pt(px(6.))
                    .gap(px(6.))
                    .items_center()
                    .child(muted(theme, "Enter save · Esc cancel · Tab next field").text_xs())
                    .child(div().flex_1())
                    .when_some(self.busy, |d, b| d.child(div().text_color(theme.accent()).child(b)))
                    .child(button("form-cancel", "Cancel").on_click(cx.listener(|this, _, w, cx| this.close_form(w, cx))))
                    .child(
                        button("form-save", "Save")
                            
                            .disabled(self.busy.is_some())
                            .on_click(cx.listener(|this, _, w, cx| this.submit_form(w, cx))),
                    ),
            );
        Self::modal(theme, "form", 720., card, None)
    }
}
