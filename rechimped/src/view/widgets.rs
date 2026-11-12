use std::sync::Arc;

use chrono::{Datelike, Duration};
use gpui::{
    BoxShadow, point, ClickEvent, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::icon_button::{IconButton, icon_button};
use gpuikit::elements::tooltip::tooltip;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ControlSize, Theme, Themeable};
use gpuikit::traits::control_sized::ControlSized;

use crate::app::TimeChimpApp;
use crate::forms::Choice;
pub(super) use crate::domain::{hh_mm, plain_number};

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

pub(super) fn cell(width: Option<f32>, content: impl Into<SharedString>) -> gpui::Div {
    wrap_cell(width, content).whitespace_nowrap().text_ellipsis()
}

pub(super) fn wrap_cell(width: Option<f32>, content: impl Into<SharedString>) -> gpui::Div {
    let d = div().overflow_hidden().px(px(6.)).child(content.into());
    match width {
        Some(w) => d.w(px(w)).flex_none(),
        None => d.flex_1().min_w(px(40.)),
    }
}

pub(super) fn right_cell(width: f32, content: impl Into<SharedString>) -> gpui::Div {
    cell(Some(width), content).flex().justify_end()
}

pub(super) fn non_billable_bg(hover: bool) -> Hsla {
    gpuikit::theme::parse_hex("#7287fd").opacity(if hover { 0.16 } else { 0.09 })
}

pub(super) fn row_hover_bg(theme: &T, billable: bool) -> Hsla {
    if billable { theme.surface() } else { non_billable_bg(true) }
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

/// Shrinks below its content (`min_h_0`), so rows must go in `table_rows` to scroll.
pub(super) fn table_card(theme: &T) -> gpui::Div {
    v_stack().min_h_0().border_1().border_color(theme.border()).rounded(px(6.)).overflow_hidden().bg(theme.bg())
}

pub(super) fn panel(theme: &T, title: impl IntoElement) -> gpui::Div {
    table_card(theme).child(
        h_stack()
            .items_center()
            .px(px(10.))
            .py(px(8.))
            .border_b_1()
            .border_color(theme.border())
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title)),
    )
}

pub(super) const ROW_ACTIONS_WIDTH: f32 = 84.;

/// gpuikit's `progress` draws its Success variant in the accent colour, so it can't show "met".
pub(super) fn meter(theme: &T, fraction: f32, colour: Hsla) -> gpui::Div {
    div()
        .w_full()
        .h(px(6.))
        .rounded(px(3.))
        .overflow_hidden()
        .bg(theme.surface_secondary())
        .child(div().h_full().w(gpui::relative(fraction.clamp(0., 1.))).rounded(px(3.)).bg(colour))
}

pub(super) fn table_rows(id: &'static str, rows: impl IntoIterator<Item = impl IntoElement>) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex().flex_col().min_h_0().overflow_y_scroll().children(rows)
}

pub(super) fn muted(theme: &T, text: impl Into<SharedString>) -> gpui::Div {
    div().text_color(theme.fg_muted()).child(text.into())
}

pub(super) fn disclaimer(theme: &T) -> gpui::Div {
    div()
        .absolute()
        .right(px(24.))
        .bottom(px(40.))
        .text_xs()
        .text_color(theme.fg_muted())
        .opacity(0.6)
        .child("Rechimped is not affiliated with or endorsed by TimeChimp.")
}

pub(super) const NOT_LOADED: &str = "Not loaded. Press R to try again.";

pub(super) fn empty_message(theme: &T, text: &'static str) -> gpui::Div {
    div().w_full().py(px(32.)).flex().justify_center().text_color(theme.fg_muted()).child(text)
}

/// Drawn as box-shadows so the button keeps its size.
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

/// gpuikit's button has no primary variant, so this mirrors its metrics.
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

pub(super) fn nav_arrow(
    id: &'static str,
    icon: gpui::Svg,
    tip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> IconButton {
    icon_button(id, icon).control_size(ControlSize::Medium).tooltip(tooltip(tip)).on_click(on_click)
}

pub(super) fn period_title(theme: &T, main: String, suffix: Option<String>) -> gpui::Div {
    h_stack()
        .pl(px(4.))
        .gap(px(8.))
        .items_baseline()
        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(main))
        .children(suffix.map(|s| muted(theme, s)))
}

pub(super) fn hint(theme: &T, key: &'static str, label: &'static str) -> gpui::Div {
    h_stack()
        .gap(px(4.))
        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.fg()).child(key))
        .child(label)
}

pub(super) fn day_header(
    theme: &T,
    id: impl Into<gpui::ElementId>,
    date: chrono::NaiveDate,
    summary: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    h_stack()
        .id(id)
        .w_full()
        .gap(px(8.))
        .px(px(10.))
        .pt(px(10.))
        .pb(px(4.))
        .items_baseline()
        .border_b_1()
        .border_color(theme.border_subtle())
        .child(div().font_weight(FontWeight::SEMIBOLD).child(date.format("%A %-d %B").to_string()))
        .child(muted(theme, summary))
}

pub(super) fn junction_row(theme: &T, junction: &crate::domain::week::Junction) -> gpui::Div {
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

pub(super) fn goal_colour(theme: &T, share: Option<f64>, goal: f64) -> gpui::Hsla {
    match share {
        None => theme.fg_muted(),
        Some(s) if s >= goal => theme.success(),
        Some(_) => theme.warning(),
    }
}

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

/// gpuikit's trigger only opens on Enter / Space. The open popup handles its own keys as bound
/// actions, so this listener only ever sees the keys of a closed one.
pub(super) fn keyboard_select<T: Clone + PartialEq + 'static>(choice: &Choice<T>) -> gpui::AnyElement {
    let choice = choice.clone();
    let child = choice.state.clone();
    div()
        .w_full()
        .on_key_down(move |event, window, cx| {
            let key = event.keystroke.key.as_str();
            let modifiers = &event.keystroke.modifiers;
            if choice.state.read(cx).is_open() || modifiers.control || modifiers.alt || modifiers.platform {
                return;
            }
            if matches!(key, "down" | "up") && !modifiers.shift {
                cx.stop_propagation();
                // Enter on the focused trigger opens it; it needs the key up to count as a click.
                window.defer(cx, |window, cx| {
                    for up in [false, true] {
                        let keystroke = gpui::Keystroke::parse("enter").expect("a valid keystroke");
                        let input = if up {
                            gpui::PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke })
                        } else {
                            gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                                keystroke,
                                is_held: false,
                                prefer_character_input: false,
                            })
                        };
                        window.dispatch_event(input, cx);
                    }
                });
            } else if let Some(typed) = event.keystroke.key_char.as_deref().filter(|c| c.chars().all(char::is_alphanumeric)) {
                if let Some(value) = choice.type_ahead(typed) {
                    choice.state.update(cx, |state, cx| state.set_selected(Some(value), cx));
                }
                cx.stop_propagation();
            }
        })
        .child(child)
        .into_any_element()
}

/// What a vertical scrollbar needs of a scrolled area, in window pixels: the viewport's top and
/// height, how far it can scroll and how far it has (both ≥ 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct ScrollMetrics {
    pub top: f32,
    pub viewport: f32,
    pub max_offset: f32,
    pub offset: f32,
}

/// Narrow enough to stay within a row's right padding, clear of its action buttons.
const SCROLLBAR_WIDTH: f32 = 8.;
const SCROLLBAR_MIN_THUMB: f32 = 24.;

/// Where a click on the bar landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BarHit {
    /// On the thumb, this far below its top.
    Thumb(f32),
    /// On the track; the offset a page up or down from here.
    Page(f32),
}

impl ScrollMetrics {
    /// Of a `list`, as laid out last frame. gpui reports the offset negative.
    pub fn of_list(state: &gpui::ListState) -> Self {
        let bounds = state.viewport_bounds();
        Self {
            top: bounds.origin.y.as_f32(),
            viewport: bounds.size.height.as_f32(),
            max_offset: state.max_offset_for_scrollbar().y.as_f32(),
            offset: -state.scroll_px_offset_for_scrollbar().y.as_f32(),
        }
    }

    /// The thumb's top (from the viewport's) and height; None when everything fits.
    pub fn thumb(&self) -> Option<(f32, f32)> {
        if self.viewport <= 0. || self.max_offset < 0.5 {
            return None;
        }
        let height = (self.viewport * self.viewport / (self.viewport + self.max_offset))
            .max(SCROLLBAR_MIN_THUMB)
            .min(self.viewport);
        let fraction = (self.offset / self.max_offset).clamp(0., 1.);
        Some(((self.viewport - height) * fraction, height))
    }

    /// The offset that puts the thumb's top `thumb_top` below the viewport's.
    pub fn offset_for_thumb(&self, thumb_top: f32) -> f32 {
        let Some((_, height)) = self.thumb() else { return 0. };
        let travel = self.viewport - height;
        if travel <= 0. {
            return 0.;
        }
        thumb_top.clamp(0., travel) / travel * self.max_offset
    }

    /// What a press at window y `y` on the bar means.
    pub fn hit(&self, y: f32) -> Option<BarHit> {
        let (top, height) = self.thumb()?;
        let y = y - self.top;
        Some(if y < top {
            BarHit::Page((self.offset - self.viewport).max(0.))
        } else if y >= top + height {
            BarHit::Page((self.offset + self.viewport).min(self.max_offset))
        } else {
            BarHit::Thumb(y - top)
        })
    }
}

/// A thin vertical scrollbar to lay over the right edge of a scrolled area (in a `relative`
/// parent of the viewport's size), or None when everything fits. The caller handles presses
/// (`ScrollMetrics::hit`) and drags; wheel events pass through to the area.
pub(super) fn scrollbar(theme: &T, id: &'static str, metrics: ScrollMetrics, dragging: bool) -> Option<gpui::Stateful<gpui::Div>> {
    let (top, height) = metrics.thumb()?;
    let colour = theme.fg_muted();
    Some(
        div()
            .id(id)
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(SCROLLBAR_WIDTH))
            .block_mouse_except_scroll()
            .child(
                div()
                    .absolute()
                    .top(px(top))
                    .h(px(height))
                    .right(px(1.))
                    .w(px(6.))
                    .rounded(px(3.))
                    .bg(colour.opacity(if dragging { 0.6 } else { 0.3 }))
                    .hover(move |s| s.bg(colour.opacity(0.6))),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(viewport: f32, max_offset: f32, offset: f32) -> ScrollMetrics {
        ScrollMetrics { top: 100., viewport, max_offset, offset }
    }

    #[test]
    fn no_thumb_when_it_fits() {
        assert_eq!(m(400., 0., 0.).thumb(), None);
        assert_eq!(m(0., 300., 0.).thumb(), None);
    }

    #[test]
    fn thumb_follows_the_offset() {
        assert_eq!(m(400., 400., 0.).thumb(), Some((0., 200.)));
        assert_eq!(m(400., 400., 200.).thumb(), Some((100., 200.)));
        assert_eq!(m(400., 400., 400.).thumb(), Some((200., 200.)));
        // Past the end (before the list clamps) the thumb stays at the bottom.
        assert_eq!(m(400., 400., 500.).thumb(), Some((200., 200.)));
    }

    #[test]
    fn thumb_has_a_minimum_height() {
        assert_eq!(m(400., 100_000., 100_000.).thumb(), Some((400. - SCROLLBAR_MIN_THUMB, SCROLLBAR_MIN_THUMB)));
    }

    #[test]
    fn dragging_maps_back_to_the_offset() {
        let metrics = m(400., 400., 0.);
        assert_eq!(metrics.offset_for_thumb(100.), 200.);
        assert_eq!(metrics.offset_for_thumb(-50.), 0.);
        assert_eq!(metrics.offset_for_thumb(1000.), 400.);
    }

    #[test]
    fn presses_hit_the_thumb_or_page() {
        let metrics = m(400., 1000., 500.);
        let (top, height) = metrics.thumb().unwrap();
        assert_eq!(metrics.hit(100. + top + 5.), Some(BarHit::Thumb(5.)));
        assert_eq!(metrics.hit(100. + top - 1.), Some(BarHit::Page(100.)));
        assert_eq!(metrics.hit(100. + top + height + 1.), Some(BarHit::Page(900.)));
        assert_eq!(m(400., 0., 0.).hit(150.), None);
    }
}
