//! Popups: the entry details, the add/edit forms and the Recent picker.

use gpui::{
    AnyElement, Context, ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Window, deferred, div, prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::elements::button::button;
use gpuikit::elements::text_field::text_field;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use crate::app::{CalculateDistance, Cancel, Confirm, CycleAddress, PickRecent, RecentDown, RecentUp, TimeChimpApp};
use crate::forms::{Form, FormBody, RecentPicker};
use crate::domain::week::RecentItem;

use super::widgets::*;

/// What a click on the dimmed backdrop around a popup does.
type BackdropHandler = Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App)>;

impl TimeChimpApp {
    /// A dimmed backdrop with a centred card, painted above everything else.
    pub(super) fn modal(
        theme: &T,
        id: &'static str,
        width: f32,
        card: gpui::Stateful<gpui::Div>,
        on_backdrop: Option<BackdropHandler>,
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

    pub(super) fn render_detail(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(e) = self.session.selected_entry() else { return div().into_any_element() };
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

    pub(super) fn render_form(&self, form: &Form, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
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
    pub(super) fn render_recent(&self, picker: &RecentPicker, theme: &T, cx: &mut Context<Self>) -> AnyElement {
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
