//! Small building blocks shared by the screens: table cells, panels, buttons, hints.

use std::sync::Arc;

use chrono::{Datelike, Duration};
use gpui::{
    AnyElement, BoxShadow, point, ClickEvent, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::kbd::kbd;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ControlSize, Theme, Themeable};
use gpuikit::traits::control_sized::ControlSized;

use crate::app::TimeChimpApp;

pub(super) type T = Arc<Theme>;

pub(super) fn hours_colour(theme: &T, hours: f64, target: f64) -> Hsla {
    if hours == 0.0 && target == 0.0 {
        theme.fg_muted()
    } else if hours >= target {
        theme.success()
    } else {
        theme.warning()
    }
}

/// A table cell with a fixed width (or flexible with `None`) that truncates its text.
pub(super) fn cell(width: Option<f32>, content: impl Into<SharedString>) -> gpui::Div {
    let d = div().overflow_hidden().whitespace_nowrap().text_ellipsis().px(px(6.)).child(content.into());
    match width {
        Some(w) => d.w(px(w)).flex_none(),
        None => d.flex_1().min_w(px(40.)),
    }
}

pub(super) fn right_cell(width: f32, content: impl Into<SharedString>) -> gpui::Div {
    cell(Some(width), content).flex().justify_end()
}

pub(super) fn header_row(theme: &T, cells: impl IntoIterator<Item = gpui::Div>) -> gpui::Div {
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
pub(super) fn panel(theme: &T, title: impl IntoElement, right: Option<AnyElement>) -> gpui::Div {
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
pub(super) fn table_rows(id: &'static str, rows: impl IntoIterator<Item = impl IntoElement>) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex().flex_col().min_h_0().overflow_y_scroll().children(rows)
}

pub(super) fn muted(theme: &T, text: impl Into<SharedString>) -> gpui::Div {
    div().text_color(theme.fg_muted()).child(text.into())
}

/// The empty message of a table whose data failed to load (the error toast says why).
pub(super) const NOT_LOADED: &str = "Not loaded. Press R to try again.";

pub(super) fn empty_message(theme: &T, text: &'static str) -> gpui::Div {
    div().w_full().py(px(32.)).flex().justify_center().text_color(theme.fg_muted()).child(text)
}

/// Outline + soft drop shadow around a gpuikit button, so it stands off the background.
/// Drawn as box-shadows, so the button keeps its size.
pub(super) fn pop(theme: &T, button: impl IntoElement) -> gpui::Div {
    let m = theme.control(ControlSize::Medium);
    div().flex_none().rounded(m.radius).shadow(button_shadow(theme.border_secondary(), 0.10)).child(button)
}

pub(super) fn button_shadow(outline: Hsla, drop: f32) -> Vec<BoxShadow> {
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
pub(super) fn primary_button(
    theme: &T,
    id: &'static str,
    label: impl Into<SharedString>,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
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

/// The ‹ Today › control of a toolbar: three joined segments under one outline (the `pop` look),
/// so they read as one widget. `at_today` shows the middle segment as selected (accent tint).
pub(super) fn period_nav(
    theme: &T,
    prev: (&'static str, &'static str, ClickHandler),
    today: (&'static str, &'static str, ClickHandler),
    next: (&'static str, &'static str, ClickHandler),
    at_today: bool,
) -> gpui::Div {
    let m = theme.control(ControlSize::Medium);
    let divider = theme.button_border();
    let arrow = |id, glyph, tip, on_click| nav_segment(theme, id, glyph, tip, false, on_click).w(m.height).text_size(px(18.));
    h_stack()
        .flex_none()
        .rounded(m.radius)
        .shadow(button_shadow(theme.border_secondary(), 0.10))
        .child(arrow(prev.0, "‹", prev.1, prev.2).rounded_l(m.radius))
        .child(
            nav_segment(theme, today.0, today.1, format!("{} (T)", today.1), at_today, today.2)
                .px(m.padding_x)
                .border_x_1()
                .border_color(divider),
        )
        .child(arrow(next.0, "›", next.1, next.2).rounded_r(m.radius))
}

pub(super) type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>;

fn nav_segment(
    theme: &T,
    id: &'static str,
    label: &'static str,
    tip: impl Into<SharedString> + 'static,
    selected: bool,
    on_click: ClickHandler,
) -> gpui::Stateful<gpui::Div> {
    let m = theme.control(ControlSize::Medium);
    let (bg, fg, hover, active) = if selected {
        // The accent tints are translucent; blend them over the button fill so the segment is opaque.
        let base = theme.button_bg();
        let hover = base.blend(theme.accent_bg_hover());
        (base.blend(theme.accent_bg()), theme.accent(), hover, hover)
    } else {
        (theme.button_bg(), theme.fg(), theme.button_bg_hover(), theme.button_bg_active())
    };
    h_stack()
        .id(id)
        .flex_none()
        .h(m.height)
        .items_center()
        .justify_center()
        .text_size(m.text_size)
        .line_height(m.line_height)
        .whitespace_nowrap()
        .bg(bg)
        .text_color(fg)
        .when(selected, |d| d.font_weight(FontWeight::SEMIBOLD))
        .child(label)
        .tooltip(gpuikit::elements::tooltip::tooltip(tip))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(active))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            on_click(event, window, cx)
        })
}

/// The heading next to a `period_nav`: the period in large type, an optional muted suffix.
pub(super) fn period_title(theme: &T, main: String, suffix: Option<String>) -> gpui::Div {
    h_stack()
        .pl(px(12.))
        .gap(px(8.))
        .items_baseline()
        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(main))
        .children(suffix.map(|s| muted(theme, s)))
}

pub(super) fn hint(key: &'static str, label: &'static str) -> gpui::Div {
    h_stack().gap(px(4.)).items_center().child(kbd(key).small()).child(label)
}

/// A thin marker between two entries: amber for a gap, red for an overlap.
pub(super) fn junction_row(theme: &T, junction: &crate::domain::week::Junction) -> impl IntoElement {
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
pub(super) fn trim_hours(hours: f64) -> String {
    let s = format!("{hours:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// "Week 45 · 2–8 Nov 2026", "Week 44 · 26 Oct – 1 Nov 2026", "Week 1 · 29 Dec 2025 – 4 Jan 2026".
/// The shown week as (date range, "Week 45").
pub(super) fn week_title(app: &TimeChimpApp) -> (String, String) {
    let (start, end) = (app.session.week_start(), app.session.week_start() + Duration::days(6));
    let range = if start.year() != end.year() {
        format!("{} – {}", start.format("%-d %b %Y"), end.format("%-d %b %Y"))
    } else if start.month() != end.month() {
        format!("{} – {}", start.format("%-d %b"), end.format("%-d %b %Y"))
    } else {
        format!("{}–{}", start.format("%-d"), end.format("%-d %b %Y"))
    };
    (range, format!("Week {}", start.iso_week().week()))
}

/// The box of a dropdown menu; its items are `menu_item`s.
pub(super) fn menu_panel(theme: &T, id: &'static str) -> gpui::Stateful<gpui::Div> {
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
