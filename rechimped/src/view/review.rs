use gpui::{
    AnyElement, Context, DispatchPhase, ElementId, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Styled, Window, canvas, div, list, px,
};
use gpuikit::layout::v_stack;
use gpuikit::theme::{ActiveTheme, Themeable};

use crate::app::TimeChimpApp;
use crate::domain::review::Line;

use super::week::{EntryLine, EntryRow};
use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_review(&self, window: &Window, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let card = table_card(theme).child(Self::entry_header(theme).rounded_t(px(5.)));
        let body = v_stack().flex_1().min_h_0().gap(px(8.));
        let (Some(rows), Some(analysis)) = (self.session.review_rows(), self.session.review_analysis()) else {
            let message = if self.busy().is_some() { "Loading review…" } else { NOT_LOADED };
            return body.child(card.child(empty_message(theme, message)));
        };
        if rows.is_empty() {
            return body.child(card.child(empty_message(theme, "No time registered this month.")));
        }
        let findings = analysis.findings;
        let summary = div()
            .px(px(4.))
            .text_color(if findings.any() { theme.warning() } else { theme.success() })
            .child(findings.label());
        let selected = rows
            .iter()
            .position(|r| Some(r.id) == self.session.selected_review_id())
            .and_then(|index| analysis.line_of(index));
        self.review_list.sync(self.session.review_revision(), analysis.lines.len(), selected, window);
        // Only the lines in view are drawn, each frame; `sync` resets the list when they change.
        // The list has no height of its own, so the card fills the page.
        let lines = list(self.review_list.state.clone(), cx.processor(|this, line, _, cx| this.render_review_line(line, cx)))
            .flex_1()
            .min_h_0();
        // Over the rows, so the columns stay in line with the header.
        let area = v_stack().relative().flex_1().min_h_0().child(lines).children(self.render_review_scrollbar(theme, cx));
        body.child(summary).child(card.flex_1().child(area))
    }

    /// The bar is drawn from last frame's layout; `watch` redraws once when this frame's differs
    /// (the first frame after a load, a resize), and while dragging follows the mouse window-wide.
    fn render_review_scrollbar(&self, theme: &T, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let list = &self.review_list;
        let metrics = ScrollMetrics::of_list(&list.state);
        let dragging = list.grab().is_some();
        let state = list.state.clone();
        let app = cx.entity().downgrade();
        let watch = canvas(
            move |_, window, _| {
                if ScrollMetrics::of_list(&state) != metrics {
                    window.request_animation_frame();
                }
            },
            move |_, _, window, _| {
                if !dragging {
                    return;
                }
                let on_move = app.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let _ = on_move.update(cx, |this, cx| {
                        let list = &this.review_list;
                        let Some(grab) = list.grab() else { return };
                        if event.pressed_button == Some(MouseButton::Left) {
                            let metrics = ScrollMetrics::of_list(&list.state);
                            list.scroll_to_offset(metrics.offset_for_thumb(event.position.y.as_f32() - metrics.top - grab));
                        } else {
                            list.end_drag();
                        }
                        cx.notify();
                    });
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = app.update(cx, |this, cx| {
                            this.review_list.end_drag();
                            cx.notify();
                        });
                    }
                });
            },
        )
        .absolute()
        .size_0();
        let bar = scrollbar(theme, "review-scrollbar", metrics, dragging).map(|bar| {
            bar.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let list = &this.review_list;
                    let metrics = ScrollMetrics::of_list(&list.state);
                    match metrics.hit(event.position.y.as_f32()) {
                        Some(BarHit::Thumb(grab)) => list.start_drag(grab),
                        Some(BarHit::Page(offset)) => list.scroll_to_offset(offset),
                        None => {}
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
        });
        let mut elements = vec![watch.into_any_element()];
        elements.extend(bar.map(|b| b.into_any_element()));
        elements
    }

    fn render_review_line(&self, n: usize, cx: &mut Context<Self>) -> AnyElement {
        let (Some(rows), Some(analysis)) = (self.session.review_rows(), self.session.review_analysis()) else {
            return div().into_any_element();
        };
        let theme = cx.theme().clone();
        match analysis.lines.get(n) {
            Some(&Line::Day { date, total }) => {
                day_header(&theme, ElementId::NamedInteger("review-day".into(), n as u64), date, hh_mm(total))
                    .into_any_element()
            }
            Some(&Line::Entry(i)) => {
                let line = EntryLine::from(&rows[i]);
                let tags_differ = analysis.mismatches.get(&line.id).map(|m| m.label());
                let row = EntryRow { selected: self.session.selected_review_id() == Some(line.id), wrap: true, tags_differ };
                self.entry_row(&theme, i, &line, row, cx).into_any_element()
            }
            None => div().into_any_element(),
        }
    }
}
