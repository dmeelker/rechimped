//! The window frame for compositors that leave decorations to the app (GNOME on Wayland has no
//! server-side ones): a title bar with the window buttons, a shadow, and resize edges. Elsewhere
//! (Windows, KDE, X11 window managers that decorate) `frame` returns the content unchanged.

use gpui::{
    AnyElement, Context, CursorStyle, Decorations, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    ResizeEdge, StatefulInteractiveElement, Styled, Svg, Tiling, Window, div, prelude::FluentBuilder, px,
};
use gpuikit::icons::Icons;
use gpuikit::theme::Themeable;

use crate::app::TimeChimpApp;

use super::widgets::T;

/// The invisible border around the window that holds its shadow and catches resize drags.
const INSET: Pixels = px(8.);
const ROUNDING: Pixels = px(8.);
const TITLE_HEIGHT: Pixels = px(32.);

impl TimeChimpApp {
    pub(super) fn frame(&mut self, content: AnyElement, window: &mut Window, theme: &T, cx: &mut Context<Self>) -> AnyElement {
        let Decorations::Client { tiling } = window.window_decorations() else {
            return content;
        };
        window.set_client_inset(INSET);
        let shadow = Hsla { h: 0., s: 0., l: 0., a: 0.3 };
        div()
            .id("frame")
            .size_full()
            .relative()
            .when(!tiling.top, |d| d.pt(INSET))
            .when(!tiling.bottom, |d| d.pb(INSET))
            .when(!tiling.left, |d| d.pl(INSET))
            .when(!tiling.right, |d| d.pr(INSET))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .bg(theme.bg())
                    .border_color(theme.border())
                    .when(!tiling.top, |d| d.border_t_1())
                    .when(!tiling.bottom, |d| d.border_b_1())
                    .when(!tiling.left, |d| d.border_l_1())
                    .when(!tiling.right, |d| d.border_r_1())
                    .when(!(tiling.top || tiling.left), |d| d.rounded_tl(ROUNDING))
                    .when(!(tiling.top || tiling.right), |d| d.rounded_tr(ROUNDING))
                    .when(!tiling.is_tiled(), |d| {
                        d.shadow(vec![gpui::BoxShadow::new(px(0.), px(0.), shadow).blur_radius(INSET / 2.)])
                    })
                    .child(self.render_title_bar(window, theme, cx))
                    .child(div().flex_1().min_h_0().relative().child(content)),
            )
            // After the content, so the corners, which reach a little into it, are on top.
            .children(resize_edges(tiling))
            .into_any_element()
    }

    fn render_title_bar(&mut self, window: &Window, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let controls = window.window_controls();
        let maximized = window.is_maximized();
        div()
            .id("title-bar")
            .flex_none()
            .h(TITLE_HEIGHT)
            .w_full()
            .flex()
            .items_center()
            .pl(px(14.))
            .text_xs()
            .text_color(theme.fg_muted())
            // Moving starts on the first drag motion, not on the press, so a double-click still
            // reaches the app (the compositor swallows events during a move).
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &gpui::MouseDownEvent, window, _| {
                    if e.click_count == 2 {
                        this.title_drag = false;
                        window.zoom_window();
                    } else {
                        this.title_drag = true;
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if std::mem::take(&mut this.title_drag) {
                    window.start_window_move();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.title_drag = false))
            .on_mouse_down(MouseButton::Right, |e, window, _| window.show_window_menu(e.position))
            .child("Rechimped")
            .child(div().flex_1())
            .when(controls.minimize, |d| {
                d.child(window_button("minimize", Icons::minus(), theme, false, |window| window.minimize_window()))
            })
            .when(controls.maximize, |d| {
                let icon = if maximized { Icons::copy() } else { Icons::square() };
                d.child(window_button("maximize", icon, theme, false, |window| window.zoom_window()))
            })
            .child(window_button("close", Icons::cross_1(), theme, true, |window| window.remove_window()))
    }
}

fn window_button(
    id: &'static str,
    icon: Svg,
    theme: &T,
    close: bool,
    on_click: impl Fn(&mut Window) + 'static,
) -> impl IntoElement {
    let (hover_bg, hover_fg) = if close { (theme.danger(), gpui::white()) } else { (theme.fg().opacity(0.08), theme.fg()) };
    div()
        .id(id)
        .w(px(44.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(move |s| s.bg(hover_bg))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_click(window);
        })
        .group(id)
        .child(icon.size(px(14.)).text_color(theme.fg_muted()).group_hover(id, move |s| s.text_color(hover_fg)))
}

/// Strips in the inset that start a resize; tiled sides have no inset and so no strips.
fn resize_edges(tiling: Tiling) -> Vec<AnyElement> {
    use ResizeEdge::*;
    let edges = [
        (Top, !tiling.top),
        (Bottom, !tiling.bottom),
        (Left, !tiling.left),
        (Right, !tiling.right),
        (TopLeft, !(tiling.top || tiling.left)),
        (TopRight, !(tiling.top || tiling.right)),
        (BottomLeft, !(tiling.bottom || tiling.left)),
        (BottomRight, !(tiling.bottom || tiling.right)),
    ];
    // Corners come last so they sit on top of the edges they overlap.
    edges
        .into_iter()
        .filter(|(_, shown)| *shown)
        .map(|(edge, _)| {
            let cursor = match edge {
                Top | Bottom => CursorStyle::ResizeUpDown,
                Left | Right => CursorStyle::ResizeLeftRight,
                TopLeft | BottomRight => CursorStyle::ResizeUpLeftDownRight,
                TopRight | BottomLeft => CursorStyle::ResizeUpRightDownLeft,
            };
            let corner = ROUNDING + INSET;
            let strip = div().absolute().cursor(cursor);
            let strip = match edge {
                Top => strip.top_0().left_0().right_0().h(INSET),
                Bottom => strip.bottom_0().left_0().right_0().h(INSET),
                Left => strip.left_0().top_0().bottom_0().w(INSET),
                Right => strip.right_0().top_0().bottom_0().w(INSET),
                TopLeft => strip.top_0().left_0().size(corner),
                TopRight => strip.top_0().right_0().size(corner),
                BottomLeft => strip.bottom_0().left_0().size(corner),
                BottomRight => strip.bottom_0().right_0().size(corner),
            };
            strip
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    window.start_window_resize(edge);
                })
                .into_any_element()
        })
        .collect()
}
