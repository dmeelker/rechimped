//! Rendering of the main window: login screen, the four tabs, popups and dialogs.

use std::sync::Arc;

use chrono::{Datelike, Duration};
use gpui::{
    Animation, AnimationExt, AnyElement, BoxShadow, point, ClickEvent, Context, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, deferred, div,
    prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::elements::button::{ButtonVariant, button};
use gpuikit::elements::context_menu::{context_menu, menu_item};
use gpuikit::elements::kbd::kbd;
use gpuikit::elements::progress::{ProgressVariant, progress};
use gpuikit::elements::text_field::text_field;
use gpuikit::elements::toast::ToastExt;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ActiveTheme, ControlSize, Theme, Themeable};
use gpuikit::traits::control_sized::ControlSized;
use timechimp::TimeEntry;

use crate::app::{
    CalculateDistance, Cancel, Confirm, CycleAddress, PickRecent, RecentDown, RecentUp, Screen, Tab, TimeChimpApp,
    ToggleShowcase,
};
use crate::forms::{Form, FormBody, RecentPicker};
use crate::domain::mileage_type_name;
use crate::domain::week::RecentItem;

type T = Arc<Theme>;

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

/// A bordered card with a title bar. It may shrink below its content (`min_h_0`), so put its
/// rows in `table_rows` to have them scroll while the title and column header stay put.
fn panel(theme: &T, title: impl IntoElement, right: Option<AnyElement>) -> gpui::Div {
    v_stack()
        .min_h_0()
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

/// The rows of a table inside a `panel`: scrolls vertically when the panel runs out of room.
fn table_rows(id: &'static str, rows: impl IntoIterator<Item = impl IntoElement>) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex().flex_col().min_h_0().overflow_y_scroll().children(rows)
}

fn muted(theme: &T, text: impl Into<SharedString>) -> gpui::Div {
    div().text_color(theme.fg_muted()).child(text.into())
}

fn empty_message(theme: &T, text: &'static str) -> gpui::Div {
    div().w_full().py(px(32.)).flex().justify_center().text_color(theme.fg_muted()).child(text)
}

/// Outline + soft drop shadow around a gpuikit button, so it stands off the background.
/// Drawn as box-shadows, so the button keeps its size.
fn pop(theme: &T, button: impl IntoElement) -> gpui::Div {
    let m = theme.control(ControlSize::Medium);
    div().flex_none().rounded(m.radius).shadow(button_shadow(theme.border_secondary(), 0.10)).child(button)
}

fn button_shadow(outline: Hsla, drop: f32) -> Vec<BoxShadow> {
    vec![
        BoxShadow { color: outline, offset: point(px(0.), px(0.)), blur_radius: px(0.), spread_radius: px(1.), inset: false },
        BoxShadow {
            color: gpui::hsla(0., 0., 0., drop),
            offset: point(px(0.), px(1.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

/// The main action of a screen: solid accent with white text. gpuikit's button has no primary
/// variant, so this mirrors its metrics (same height/padding/radius as the other controls).
fn primary_button(
    theme: &T,
    id: &'static str,
    label: impl Into<SharedString>,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let m = theme.control(ControlSize::Medium);
    let accent = theme.accent();
    let darker = |by: f32| gpui::hsla(accent.h, accent.s, (accent.l - by).max(0.), accent.a);
    let (hover, active) = (darker(0.06), darker(0.12));
    h_stack()
        .id(id)
        .flex_none()
        .h(m.height)
        .px(m.padding_x)
        .items_center()
        .justify_center()
        .rounded(m.radius)
        .text_size(m.text_size)
        .line_height(m.line_height)
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .bg(accent)
        .text_color(gpui::white())
        .shadow(button_shadow(darker(0.08), 0.18))
        .child(label.into())
        .when(disabled, |d| d.opacity(0.5).cursor_not_allowed())
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(active))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    on_click(event, window, cx)
                })
        })
}

fn hint(key: &'static str, label: &'static str) -> gpui::Div {
    h_stack().gap(px(4.)).items_center().child(kbd(key).small()).child(label)
}

/// A thin marker between two entries: amber for a gap, red for an overlap.
fn junction_row(theme: &T, junction: &crate::domain::week::Junction) -> impl IntoElement {
    use crate::domain::week::Junction;
    let (colour, from, to, what) = match *junction {
        Junction::Gap { from, to } => (theme.warning(), from, to, "gap"),
        Junction::Overlap { from, to } => (theme.danger(), from, to, "overlap"),
    };
    let minutes = (to - from).num_minutes();
    let duration = if minutes >= 60 { format!("{}h{:02}", minutes / 60, minutes % 60) } else { format!("{minutes} min") };
    h_stack()
        .w_full()
        .px(px(10.))
        .py(px(3.))
        .gap(px(8.))
        .items_center()
        .bg(colour.opacity(0.08))
        .border_b_1()
        .border_color(theme.border_subtle())
        .text_xs()
        .text_color(colour)
        .child(div().w(px(3.)).h(px(14.)).rounded(px(2.)).bg(colour))
        .child(format!("{} – {} · {duration} {what}", from.format("%H:%M"), to.format("%H:%M")))
        .child(div().flex_1().h(px(1.)).bg(colour.opacity(0.4)))
}

/// Hours without trailing zeros: 24 → "24", 24.5 → "24.5", 24.75 → "24.75".
fn trim_hours(hours: f64) -> String {
    let s = format!("{hours:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// "Week 45 · 2–8 Nov 2026", "Week 44 · 26 Oct – 1 Nov 2026", "Week 1 · 29 Dec 2025 – 4 Jan 2026".
fn week_title(app: &TimeChimpApp) -> String {
    let (start, end) = (app.week_start, app.week_start + Duration::days(6));
    let range = if start.year() != end.year() {
        format!("{} – {}", start.format("%-d %b %Y"), end.format("%-d %b %Y"))
    } else if start.month() != end.month() {
        format!("{} – {}", start.format("%-d %b"), end.format("%-d %b %Y"))
    } else {
        format!("{}–{}", start.format("%-d"), end.format("%-d %b %Y"))
    };
    format!("Week {} · {range}", start.iso_week().week())
}

// ---- Login ------------------------------------------------------------------------------------

impl TimeChimpApp {
    fn render_login(&self, theme: &T, t: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let Screen::Login(login) = &self.screen else { unreachable!() };
        let accent = theme.accent();
        let label = |text: &'static str| div().text_xs().text_color(theme.fg_muted()).child(text);
        let message = match (self.busy(), &login.error) {
            (Some(busy), _) => div().text_color(accent).child(busy),
            (None, Some(error)) => div().text_color(theme.danger()).child(error.clone()),
            (None, None) => muted(theme, "Tab: switch field · Enter: log in"),
        };
        let mfa_label = login.mfa.as_ref().map(|m| if m.kind() == "SMS_MFA" { "SMS code" } else { "Authenticator code" });

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

    fn login_confirm_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Login(login) = &self.screen else { return };
        if login.mfa.is_some() {
            self.login_confirm(window, cx);
        } else {
            self.login(window, cx);
        }
    }

    // ---- Main ---------------------------------------------------------------------------------

    fn render_main(&mut self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.tabs.clone())
            .child(div().flex_1())
            .child(div().relative().mb(px(6.)).child(self.render_user_button(theme, title, cx)).when(
                self.user_menu,
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
            .when(self.user_menu, |d| d.child(self.render_user_menu_backdrop(cx)))
            .when(self.show_detail, |d| d.child(self.render_detail(theme, cx)))
            .when_some(self.form.as_ref(), |d, form| d.child(self.render_form(form, theme, cx)))
    }

    fn render_toolbar(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(primary_button(
                        theme,
                        "add",
                        if week_tab { "+ Add time" } else { "+ Add trip" },
                        false,
                        cx.listener(|this, _, w, cx| this.add(w, cx)),
                    ))
            }
            Tab::Report => bar
                .child(pop(theme, nav("prev-month", "‹", |this, w, cx| this.shift_month(-1, w, cx))))
                .child(pop(theme, nav("this-month", "This month", |this, w, cx| this.go_today(w, cx))))
                .child(pop(theme, nav("next-month", "›", |this, w, cx| this.shift_month(1, w, cx))))
                .child(div().pl(px(8.)).font_weight(FontWeight::SEMIBOLD).child(self.report_month.format("%B %Y").to_string()))
                .child(div().flex_1())
                .child(pop(theme, nav("reload", "Reload", |this, w, cx| this.reload(w, cx)))),
            Tab::Projects => bar
                .child(div().flex_1())
                .child(pop(theme, nav("reload", "Reload", |this, w, cx| this.reload(w, cx)))),
        }
        .text_color(theme.fg())
    }

    /// The user name in the header; a click opens the user menu.
    fn render_user_button(&self, theme: &T, title: String, cx: &mut Context<Self>) -> impl IntoElement {
        h_stack()
            .id("user-button")
            .gap(px(8.))
            .items_center()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(self.user_menu, |d| d.bg(theme.surface()))
            .hover(|s| s.bg(theme.surface()))
            .child(div().size(px(12.)).rounded_full().bg(theme.accent()))
            .child(muted(theme, title))
            .child(muted(theme, "▾"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.user_menu = !this.user_menu;
                cx.notify();
            }))
    }

    /// Transparent, window-wide layer under the open user menu: any click outside closes it.
    fn render_user_menu_backdrop(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
            this.user_menu = false;
            cx.stop_propagation();
            cx.notify();
        });
        deferred(div().id("user-menu-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, close))
            .with_priority(1)
    }

    /// The dropdown under the user name (a child of its `relative()` wrapper).
    fn render_user_menu(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let item = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px(px(12.))
                .py(px(6.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.accent_bg()))
                .child(label)
        };
        let menu = v_stack()
            .id("user-menu")
            .occlude()
            .min_w(px(160.))
            .p(px(4.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border())
            .bg(theme.surface())
            .shadow_lg()
            .text_color(theme.fg())
            .child(item("user-menu-logout", "Log out").on_click(cx.listener(|this, _, w, cx| this.logout(w, cx))));
        deferred(div().absolute().top_full().right_0().mt(px(4.)).child(menu)).with_priority(2)
    }

    /// Buttons acting on the selected row, for the right of the Week/Mileage panel header.
    fn selection_actions(&self, theme: &T, cx: &mut Context<Self>) -> AnyElement {
        let nav = |id: &'static str, label: &'static str, f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            button(id, label).on_click(cx.listener(move |this, _: &ClickEvent, window, cx| f(this, window, cx)))
        };
        let week_tab = self.tab == Tab::Week;
        let has = if week_tab { self.selected_entry().is_some() } else { self.selected_mileage().is_some() };
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

    fn render_footer(&self, theme: &T) -> impl IntoElement {
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

    fn entry_row(&self, theme: &T, index: usize, e: &TimeEntry, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(cell(Some(110.), e.start_end.clone().unwrap_or_else(|| "–".into())))
            .child(right_cell(60., format!("{:.2}", e.hours)))
            .child(cell(Some(160.), e.customer_name.clone()))
            .child(cell(None, format!("{} / {}", e.project_name, e.task_name)))
            .child(cell(None, e.notes.clone().unwrap_or_default()))
            .child(cell(Some(140.), e.tag_names.join(", ")).text_color(theme.fg_muted()))
    }

    /// Rows for the entries at `indices` (one day, sorted by start), with a marker between two
    /// entries whose times don't connect: a gap, or an overlap.
    fn entry_rows_with_gaps(&self, theme: &T, indices: &[usize], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        for (n, &i) in indices.iter().enumerate() {
            if let Some(&prev) = n.checked_sub(1).and_then(|p| indices.get(p)) {
                if let Some(junction) = crate::domain::week::junction(&self.entries[prev], &self.entries[i]) {
                    rows.push(junction_row(theme, &junction).into_any_element());
                }
            }
            rows.push(self.entry_row(theme, i, &self.entries[i], cx).into_any_element());
        }
        rows
    }

    /// The Week tab: a strip with the week's days on top, the selected day's entries below.
    fn render_day(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let per_day = self.hours_per_day();
        let total: f64 = per_day.iter().sum();
        let contract = self.contract_hours();

        let strip = h_stack().flex_none().gap(px(8.)).children((0..7).map(|i| {
            let day = self.week_start + Duration::days(i as i64);
            let (hours, target) = (per_day[i], self.target_for(i));
            let selected = day == self.day;
            let is_today = day == crate::domain::time::today();
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
                .child(div().text_lg().text_color(hours_colour(theme, hours, target)).child(format!("{}h", trim_hours(hours))))
                .child(progress(fraction).variant(if hours >= target { ProgressVariant::Success } else { ProgressVariant::Default }))
        }));

        // Week card: total against the contract, and billable hours against the contract (without
        // one, against what was logged), coloured by the billable goal.
        let share = crate::domain::week::billable_share(&self.entries, contract);
        let ratio = share.unwrap_or(0.0);
        let goal_met = share.is_some_and(|s| s >= self.billable_goal);
        let ratio_colour = match share {
            None => theme.fg_muted(),
            Some(_) if goal_met => theme.success(),
            Some(_) => theme.warning(),
        };
        let week_card = v_stack()
            .flex_1()
            .gap(px(4.))
            .p(px(10.))
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border())
            .bg(theme.bg())
            .child(
                h_stack()
                    .justify_between()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Week"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(ratio_colour)
                            .child(format!("{:.0}% billable", ratio * 100.0)),
                    ),
            )
            .child(
                h_stack()
                    .gap(px(4.))
                    .items_baseline()
                    .child(div().text_lg().text_color(hours_colour(theme, total, contract)).child(format!("{}h", trim_hours(total))))
                    .child(muted(theme, format!("/ {}h", trim_hours(contract)))),
            )
            .child(
                progress(ratio.min(1.0) as f32)
                    .variant(if goal_met { ProgressVariant::Success } else { ProgressVariant::Default }),
            );
        let strip = strip.child(week_card);

        let index = (self.day - self.week_start).num_days() as usize;
        let (hours, target) = (self.hours_per_day()[index], self.target_for(index));
        let visible = self.visible_entries();
        let title = h_stack()
            .gap(px(4.))
            .child(self.day.format("%A %-d %B").to_string())
            .child("·")
            .child(div().text_color(hours_colour(theme, hours, target)).child(trim_hours(hours)))
            .child(format!("/ {}h", trim_hours(target)));
        let rows = self.entry_rows_with_gaps(theme, &visible, cx);

        v_stack()
            .flex_1()
            .min_h_0()
            .gap(px(12.))
            .child(strip)
            .child(
                panel(theme, title, Some(self.selection_actions(theme, cx)))
                    .child(Self::entry_header(theme))
                    .child(table_rows("entry-rows", rows))
                    .when(
                    visible.is_empty() && self.busy().is_none(),
                    |d| d.child(empty_message(theme, "No time registered on this day. Press A to add.")),
                ),
            )
    }

    // ---- Mileage tab --------------------------------------------------------------------------

    fn trip_row(&self, theme: &T, i: usize, m: &timechimp::Mileage, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| this.click_mileage(i, ev.click_count(), window, cx)))
            .child(right_cell(60., format!("{}", m.distance)))
            .child(cell(None, m.from_address.clone().unwrap_or_default()))
            .child(cell(None, m.to_address.clone().unwrap_or_default()))
            .child(cell(Some(150.), m.project_name.clone()))
            .child(cell(Some(110.), mileage_type_name(m.mileage_type)))
            .child(cell(Some(160.), m.notes.clone().unwrap_or_default()))
    }

    /// The Mileage tab: the week's trips grouped per day (they're sorted by day in `apply_week`),
    /// each day under a header with its total.
    fn render_mileage(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let total: f64 = self.mileage.iter().map(|m| m.distance).sum();
        let actions = self.selection_actions(theme, cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut start = 0;
        for (n, day) in self.mileage.chunk_by(|a, b| a.date == b.date).enumerate() {
            let km: f64 = day.iter().map(|m| m.distance).sum();
            let trips = if day.len() == 1 { "1 trip".to_owned() } else { format!("{} trips", day.len()) };
            rows.push(
                h_stack()
                    .id(ElementId::NamedInteger("trip-day".into(), n as u64))
                    .w_full()
                    .gap(px(8.))
                    .px(px(10.))
                    .pt(px(10.))
                    .pb(px(4.))
                    .items_baseline()
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(day[0].date.format("%A %-d %B").to_string()))
                    .child(muted(theme, format!("{trips} · {} km", trim_hours(km))))
                    .into_any_element(),
            );
            for (k, m) in day.iter().enumerate() {
                rows.push(self.trip_row(theme, start + k, m, cx).into_any_element());
            }
            start += day.len();
        }
        let empty = self.mileage.is_empty() && self.busy().is_none();
        let trips = panel(theme, format!("Trips · {} km", trim_hours(total)), Some(actions))
            .child(header_row(
                theme,
                [
                    right_cell(60., "km"),
                    cell(None, "From"),
                    cell(None, "To"),
                    cell(Some(150.), "Project"),
                    cell(Some(110.), "Type"),
                    cell(Some(160.), "Notes"),
                ],
            ))
            .child(table_rows("trip-rows", rows))
            .when(empty, |d| d.child(empty_message(theme, "No mileage this week. Press A to add.")));
        v_stack().min_h_0().gap(px(10.)).child(self.render_presets(theme, cx)).child(trips)
    }

    /// One button per `[[trips]]` preset in `config.toml`; each opens the trip form for the
    /// selected day, filled in, so adding a regular trip is a click plus Save. Right-click for
    /// a menu with Delete.
    fn render_presets(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let buttons = self.trip_presets.iter().enumerate().map(|(i, preset)| {
            let trigger = pop(
                theme,
                button(ElementId::NamedInteger("preset".into(), i as u64), preset.name.clone())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_preset(i, window, cx))),
            );
            let this = this.clone();
            context_menu(ElementId::NamedInteger("preset-menu".into(), i as u64), trigger).menu(move |menu, _, _| {
                let this = this.clone();
                menu.item(menu_item("Delete").destructive().on_click(move |window, cx| {
                    let _ = this.update(cx, |this, cx| this.ask_delete_preset(i, window, cx));
                }))
            })
        });
        h_stack()
            .flex_wrap()
            .gap(px(8.))
            .items_center()
            .child(muted(theme, "Presets"))
            .children(buttons)
            .when(self.trip_presets.is_empty(), |d| {
                d.child(muted(theme, "none yet: use \"Save as preset\" in the trip form, or add [[trips]] to config.toml"))
            })
    }

    // ---- Report tab ---------------------------------------------------------------------------

    fn render_report(&self, theme: &T) -> impl IntoElement {
        let month = self.report_month.format("%B %Y").to_string();
        let report = &self.report;
        if report.month != Some(self.report_month) {
            return panel(theme, month, None).child(empty_message(theme, "Loading report…")).into_any_element();
        }

        let summary = report.summary();
        let pct = |h: f64| summary.share(h) * 100.0;

        let rows = summary.groups.iter().map(|g| {
            h_stack()
                .w_full()
                .py(px(6.))
                .px(px(4.))
                .items_center()
                .border_b_1()
                .border_color(theme.border_subtle())
                .when(!g.billable, |d| d.text_color(theme.fg_muted()))
                .child(cell(None, g.project.clone()))
                .child(cell(None, g.task.clone()))
                .child(right_cell(70., format!("{:.2}", g.hours)))
                .child(right_cell(50., format!("{:.0}%", pct(g.hours))))
                .child(div().w(px(100.)).flex_none().px(px(6.)).child(progress(summary.share(g.hours) as f32)))
        });
        let table = panel(theme, format!("{month} · hours per project / task"), None)
            .flex_1()
            .child(header_row(
                theme,
                [cell(None, "Project"), cell(None, "Task"), right_cell(70., "Hours"), right_cell(50., "%"), cell(Some(100.), "")],
            ))
            .child(table_rows("report-rows", rows))
            .when(summary.groups.is_empty(), |d| d.child(empty_message(theme, "No time registered this month.")));

        let stat = |label: &'static str, value: String| {
            h_stack()
                .justify_between()
                .py(px(4.))
                .child(muted(theme, label))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(value))
        };
        let s = &summary;
        let totals = panel(theme, "Summary", None).w(px(300.)).flex_none().child(
            v_stack()
                .p(px(12.))
                .child(stat("Total hours", format!("{:.2}", s.total)))
                .child(stat("Billable", format!("{:.2} ({:.0}%)", s.billable, pct(s.billable))).when_some(
                    s.billable_share(),
                    |d, share| d.text_color(if share >= self.billable_goal { theme.success() } else { theme.warning() }),
                ))
                .child(stat("Billable goal", format!("{:.0}%", self.billable_goal * 100.0)))
                .child(stat("Non-billable", format!("{:.2}", s.total - s.billable)))
                .child(stat("Days registered", s.days.to_string()))
                .child(stat("Entries", s.entries.to_string()))
                .child(div().h(px(1.)).my(px(8.)).bg(theme.border()))
                .child(stat("Trips", s.trips.to_string()))
                .child(stat("Distance", format!("{:.0} km", s.km)))
                .child(stat("Reimbursement", format!("€ {:.2}", s.reimbursement))),
        );
        // Panels are at most as tall as the body (`max_h_full`), so a long table scrolls.
        h_stack()
            .flex_1()
            .min_h_0()
            .items_start()
            .gap(px(12.))
            .child(table.max_h_full())
            .child(totals.max_h_full())
            .into_any_element()
    }

    // ---- Projects tab -------------------------------------------------------------------------

    fn render_projects(&self, theme: &T) -> impl IntoElement {
        let projects = panel(theme, format!("My projects ({})", self.projects.len()), None)
            .flex_1()
            .child(header_row(
                theme,
                [cell(None, "Customer"), cell(None, "Project"), right_cell(60., "Tasks"), cell(Some(70.), "Active")],
            ))
            .child(table_rows("project-rows", self.projects.iter().map(|p| {
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
            })));
        let tasks = panel(theme, format!("Tasks ({})", self.tasks.len()), None)
            .w(px(360.))
            .flex_none()
            .child(header_row(theme, [cell(None, "Task"), cell(Some(80.), "Billable")]))
            .child(table_rows("task-rows", self.tasks.iter().map(|t| {
                h_stack()
                    .w_full()
                    .py(px(6.))
                    .px(px(4.))
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .when(!t.billable, |d| d.text_color(theme.fg_muted()))
                    .child(cell(None, t.name.clone()))
                    .child(cell(Some(80.), if t.billable { "yes" } else { "no" }))
            })));
        h_stack()
            .flex_1()
            .min_h_0()
            .items_start()
            .gap(px(12.))
            .child(projects.max_h_full())
            .child(tasks.max_h_full())
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
                    .mt(px(16.))
                    .gap(px(8.))
                    .justify_end()
                    .child(pop(
                        theme,
                        button("detail-edit", "Edit").on_click(cx.listener(|this, _, w, cx| {
                            this.show_detail = false;
                            this.edit_selected(w, cx)
                        })),
                    ))
                    .child(primary_button(
                        theme,
                        "detail-close",
                        "Close",
                        false,
                        cx.listener(|this, _, w, cx| {
                            this.show_detail = false;
                            w.focus(&this.focus, cx);
                            cx.notify()
                        }),
                    )),
            );
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
            this.show_detail = false;
            cx.notify()
        });
        Self::modal(theme, "detail", 560., card, Some(Box::new(close))).into_any_element()
    }

    fn render_form(&self, form: &Form, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        // Only the mileage form has hints; the time form uses the full width for its fields.
        let show_help = matches!(form.body, FormBody::Mileage(_));
        let row = |label: &'static str, control: AnyElement, help: Option<&'static str>| {
            h_stack()
                .gap(px(16.))
                .items_center()
                .child(div().w(px(110.)).flex_none().text_color(theme.fg_muted()).child(label))
                .child(div().flex_1().min_w_0().child(control))
                .when(show_help, |d| {
                    d.child(div().w(px(150.)).flex_none().text_xs().text_color(theme.fg_muted()).children(help))
                })
        };
        let field = |input, placeholder: &'static str, cx: &mut Context<Self>| {
            text_field(input, cx).placeholder(placeholder).into_any_element()
        };
        let rows: Vec<AnyElement> = match &form.body {
            FormBody::Time(f) => vec![
                row("Recent", f.recent.as_ref().map_or_else(|| div().into_any_element(), |p| self.render_recent(p, theme, cx)), None)
                    .when(f.recent.is_none(), |d| d.hidden())
                    .items_start(),
                // Separates the picker from the fields it fills in.
                h_stack().h(px(1.)).w_full().bg(theme.border()).when(f.recent.is_none(), |d| d.hidden()),
                row("Date", field(&f.date, "YYYY-MM-DD", cx), None),
                row("Project", f.project.clone().into_any_element(), None),
                row("Task", f.task.clone().into_any_element(), None),
                row("Start", field(&f.start, "HH:MM", cx), None),
                row("End", field(&f.end, "HH:MM, +15 or +1h30", cx), None),
                row("Notes", field(&f.notes, "What did you work on?", cx), None),
                row("Hours", field(&f.hours, "used without start/end, e.g. 1.5", cx), None),
                // Only projects with tags in config.toml get the row.
                row(
                    "Tags",
                    h_stack()
                        .flex_wrap()
                        .gap(px(16.))
                        .children(f.tags.iter().map(|t| t.checkbox.clone()))
                        .into_any_element(),
                    None,
                )
                .when(f.tags.is_empty(), |d| d.hidden()),
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
                        .gap(px(8.))
                        .child(div().flex_1().child(field(&f.distance, "km", cx)))
                        .child(pop(
                            theme,
                            button("calc-distance", "Calculate")
                                .on_click(cx.listener(|this, _, w, cx| this.calculate_distance(w, cx))),
                        ))
                        .into_any_element(),
                    Some("Ctrl+D: calculate"),
                ),
                // Editing changes one trip; the return switch only applies to new ones.
                row("", f.return_trip.clone().into_any_element(), None).when(f.editing.is_some(), |d| d.hidden()),
                row("Type", f.mileage_type.clone().into_any_element(), None),
                row("", f.billable.clone().into_any_element(), None),
                row("Notes", field(&f.notes, "", cx), None),
            ]
            .into_iter()
            .map(IntoElement::into_any_element)
            .collect(),
        };
        let card = v_stack()
            .id("form-card")
            .key_context("Modal")
            .moves_focus_on_tab()
            .on_action(cx.listener(|this, _: &Confirm, w, cx| this.submit_form(w, cx)))
            .on_action(cx.listener(|this, _: &Cancel, w, cx| this.close_form(w, cx)))
            .on_action(cx.listener(|this, _: &CalculateDistance, w, cx| this.calculate_distance(w, cx)))
            .on_action(cx.listener(|this, _: &CycleAddress, w, cx| this.cycle_address(w, cx)))
            .on_action(cx.listener(|this, _: &RecentUp, _, cx| this.move_recent(-1, cx)))
            .on_action(cx.listener(|this, _: &RecentDown, _, cx| this.move_recent(1, cx)))
            .on_action(cx.listener(|this, _: &PickRecent, w, cx| this.pick_highlighted_recent(w, cx)))
            .p(px(24.))
            .gap(px(14.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).child(form.title()))
            .children(rows)
            .when_some(form.error.clone(), |d, e| {
                d.child(div().p(px(8.)).rounded(px(6.)).bg(theme.danger().opacity(0.15)).text_color(theme.danger()).child(e))
            })
            .child(
                h_stack()
                    .pt(px(8.))
                    .gap(px(8.))
                    .items_center()
                    // Any trip form, so an existing trip can become a preset too.
                    .when(matches!(&form.body, FormBody::Mileage(_)), |d| {
                        d.child(pop(
                            theme,
                            button("form-preset", "Save as preset")
                                .on_click(cx.listener(|this, _, w, cx| this.save_as_preset(w, cx))),
                        ))
                    })
                    .child(div().flex_1())
                    .when_some(self.busy(), |d, b| d.child(div().text_color(theme.accent()).child(b)))
                    .child(pop(
                        theme,
                        button("form-cancel", "Cancel").on_click(cx.listener(|this, _, w, cx| this.close_form(w, cx))),
                    ))
                    .child(primary_button(
                        theme,
                        "form-save",
                        "Save",
                        self.busy().is_some(),
                        cx.listener(|this, _, w, cx| this.submit_form(w, cx)),
                    )),
            );
        Self::modal(theme, "form", 720., card, None)
    }

    /// The time form's Recent picker: a filter field over the recent items, the highlighted one
    /// picked with Enter (or any one by clicking it).
    fn render_recent(&self, picker: &RecentPicker, theme: &T, cx: &mut Context<Self>) -> AnyElement {
        let matches: Vec<RecentItem> = self.recent_matches(cx).into_iter().cloned().collect();
        let selected = picker.selected.min(matches.len().saturating_sub(1));
        let empty = matches.is_empty();
        let typed = !crate::forms::text(&picker.filter, cx).is_empty();
        let rows = matches.into_iter().enumerate().map(|(i, item)| {
            let notes = item.notes().to_owned();
            let e = item.entry;
            let label = format!("{} / {}", e.project_name, e.task_name);
            let tags = e.tag_names.join(", ");
            h_stack()
                .id(ElementId::NamedInteger("recent".into(), i as u64))
                .py(px(4.))
                .px(px(4.))
                .cursor_pointer()
                .when(i == selected, |d| d.bg(theme.accent_bg()))
                .when(i != selected, |d| d.hover(|s| s.bg(theme.surface())))
                .child(if notes.is_empty() {
                    cell(Some(200.), "(no notes)").text_color(theme.fg_muted())
                } else {
                    cell(Some(200.), notes)
                })
                .child(cell(None, label).text_color(theme.fg_muted()))
                .child(cell(Some(110.), tags).text_color(theme.fg_muted()))
                .on_click(cx.listener(move |this, _, w, cx| this.pick_recent(e.clone(), w, cx)))
        });
        let list = v_stack()
            .border_1()
            .border_color(theme.border())
            .rounded(px(6.))
            .overflow_hidden()
            .bg(theme.bg())
            .children(rows)
            .when(empty, |d| {
                d.child(muted(theme, "Nothing matches").px(px(10.)).py(px(4.)))
            });
        v_stack()
            .gap(px(6.))
            .child(
                div()
                    .key_context("RecentPicker")
                    .child(text_field(&picker.filter, cx).placeholder("Type to search recent work · ↑/↓ and Enter to pick · Tab to skip")),
            )
            .when(typed, |d| d.child(list))
            .into_any_element()
    }
}
