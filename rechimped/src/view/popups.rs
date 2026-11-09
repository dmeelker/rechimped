use gpui::{
    AnyElement, Context, ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Window, deferred, div, prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::elements::button::button;
use gpuikit::elements::text_field::{Adornment, text_field};
use gpuikit::icons::Icons;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ControlSize, Themeable};

use crate::app::{
    AddressDown, AddressUp, CalculateDistance, Cancel, CloseAddresses, CloseCalendar, Confirm, NextDay, NextMonth,
    NextWeek, PickAddress, PickRecent, PreviousDay, PreviousMonth, PreviousWeek, RecentDown, RecentUp, TimeChimpApp,
};
use crate::forms::{AddressField, DateField, Form, FormBody, TravelForm, RecentPicker};
use crate::domain::week::RecentItem;

use super::widgets::*;

type BackdropHandler = Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App)>;

impl TimeChimpApp {
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
                            this.close_overlay(w, cx);
                            this.edit_selected(w, cx)
                        })),
                    ))
                    .child(primary_button(
                        theme,
                        "detail-close",
                        "Close",
                        false,
                        cx.listener(|this, _, w, cx| {
                            this.close_overlay(w, cx)
                        }),
                    )),
            );
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, w, cx| this.close_overlay(w, cx));
        Self::modal(theme, "detail", 560., card, Some(Box::new(close))).into_any_element()
    }

    pub(super) fn render_form(&self, form: &Form, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &'static str, control: AnyElement| {
            h_stack()
                .gap(px(16.))
                .items_center()
                .child(div().w(px(110.)).flex_none().text_color(theme.fg_muted()).child(label))
                .child(div().flex_1().min_w_0().child(control))
        };
        let field = |input, placeholder: &'static str, cx: &mut Context<Self>| {
            text_field(input, cx).placeholder(placeholder).into_any_element()
        };
        let rows: Vec<AnyElement> = match &form.body {
            FormBody::Time(f) => vec![
                row("Recent", f.recent.as_ref().map_or_else(|| div().into_any_element(), |p| self.render_recent(p, theme, cx)))
                    .when(f.recent.is_none(), |d| d.hidden())
                    .items_start(),
                h_stack().h(px(1.)).w_full().bg(theme.border()).when(f.recent.is_none(), |d| d.hidden()),
                row("Date", self.render_date(&f.date, theme, cx)),
                row("Project", keyboard_select(&f.project)),
                row("Task", keyboard_select(&f.task)),
                row("Start", field(&f.start, "HH:MM", cx)),
                row("End", field(&f.end, "HH:MM, +15 or +1h30", cx)),
                row("Notes", field(&f.notes, "What did you work on?", cx)),
                row("Hours", field(&f.hours, "used without start/end, e.g. 1.5", cx)),
                row(
                    "Tags",
                    h_stack()
                        .flex_wrap()
                        .gap(px(16.))
                        .children(f.tags.iter().map(|t| t.checkbox.clone()))
                        .into_any_element(),
                )
                .when(f.tags.is_empty(), |d| d.hidden()),
            ]
            .into_iter()
            .map(IntoElement::into_any_element)
            .collect(),
            FormBody::Travel(f) => vec![
                row("Date", self.render_date(&f.date, theme, cx)),
                row("Project", keyboard_select(&f.project)),
                row("From", self.render_address(f, AddressField::From, theme, cx)),
                row("To", self.render_address(f, AddressField::To, theme, cx)),
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
                ),
                // Editing changes one trip; the return switch only applies to new ones.
                row("", f.return_trip.clone().into_any_element()).when(f.editing.is_some(), |d| d.hidden()),
                row("Type", keyboard_select(&f.travel_type)),
                row("", f.billable.clone().into_any_element()),
                row("Notes", field(&f.notes, "", cx)),
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
            .on_action(cx.listener(|this, _: &Cancel, w, cx| this.close_overlay(w, cx)))
            .on_action(cx.listener(|this, _: &CalculateDistance, w, cx| this.calculate_distance(w, cx)))
            .on_action(cx.listener(|this, _: &AddressUp, w, cx| this.move_address(-1, w, cx)))
            .on_action(cx.listener(|this, _: &AddressDown, w, cx| this.move_address(1, w, cx)))
            .on_action(cx.listener(|this, _: &PickAddress, _, cx| this.pick_highlighted_address(cx)))
            .on_action(cx.listener(|this, _: &CloseAddresses, _, cx| this.dismiss_addresses(cx)))
            .on_action(cx.listener(|this, _: &PreviousDay, _, cx| this.move_calendar(-1, 0, cx)))
            .on_action(cx.listener(|this, _: &NextDay, _, cx| this.move_calendar(1, 0, cx)))
            .on_action(cx.listener(|this, _: &PreviousWeek, _, cx| this.move_calendar(-7, 0, cx)))
            .on_action(cx.listener(|this, _: &NextWeek, w, cx| this.calendar_down(w, cx)))
            .on_action(cx.listener(|this, _: &PreviousMonth, _, cx| this.move_calendar(0, -1, cx)))
            .on_action(cx.listener(|this, _: &NextMonth, _, cx| this.move_calendar(0, 1, cx)))
            .on_action(cx.listener(|this, _: &CloseCalendar, _, cx| this.dismiss_calendar(cx)))
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
                    .child(pop(
                        theme,
                        button("form-preset", "Save as preset").on_click(cx.listener(|this, _, w, cx| this.save_as_preset(w, cx))),
                    ))
                    .child(div().flex_1())
                    .when_some(self.busy(), |d, b| d.child(div().text_color(theme.accent()).child(b)))
                    .child(pop(
                        theme,
                        button("form-cancel", "Cancel").on_click(cx.listener(|this, _, w, cx| this.close_overlay(w, cx))),
                    ))
                    .child(primary_button(
                        theme,
                        "form-save",
                        "Save",
                        self.saving(),
                        cx.listener(|this, _, w, cx| this.submit_form(w, cx)),
                    )),
            );
        Self::modal(theme, "form", 720., card, None)
    }

    fn render_date(&self, field: &DateField, theme: &T, cx: &mut Context<Self>) -> AnyElement {
        let color = if field.picker.is_some() { theme.accent() } else { theme.fg_muted() };
        let icon = div()
            .id("date-calendar-toggle")
            .flex()
            .items_center()
            .px(px(2.))
            .cursor_pointer()
            .child(Icons::calendar().size(px(14.)).text_color(color))
            // On press, so the field underneath doesn't take the click.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, w, cx| {
                    cx.stop_propagation();
                    this.toggle_calendar(w, cx)
                }),
            );
        let input = text_field(&field.input, cx).placeholder("YYYY-MM-DD").suffix(Adornment::element(icon));
        let popup = field.picker.as_ref().map(|picker| {
            // gpuikit colours its month buttons on the div, which doesn't reach their chevron svgs, so
            // they draw nothing. These chevrons take no mouse events, so clicks reach the buttons underneath.
            let m = theme.control(ControlSize::default());
            let chevron = |icon: gpui::Svg| {
                div()
                    .size(m.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon.size(m.text_size).text_color(theme.fg_muted()))
            };
            let chevrons = div().absolute().inset_0().border_1().border_color(gpui::transparent_black()).child(
                h_stack()
                    .p(m.padding_x)
                    .justify_between()
                    .child(chevron(Icons::chevron_left()))
                    .child(chevron(Icons::chevron_right())),
            );
            let panel = div()
                .id("date-calendar")
                .relative()
                .occlude()
                .rounded(px(6.))
                .bg(theme.surface())
                .shadow_lg()
                .child(picker.calendar.clone())
                .child(chevrons);
            deferred(div().absolute().top_full().left_0().mt(px(4.)).child(panel)).with_priority(3)
        });
        div().relative().key_context("DateField").child(input).children(popup).into_any_element()
    }

    fn render_address(&self, form: &TravelForm, field: AddressField, theme: &T, cx: &mut Context<Self>) -> AnyElement {
        let (chevron_id, menu_id) = match field {
            AddressField::From => ("from-addresses-toggle", "from-addresses"),
            AddressField::To => ("to-addresses-toggle", "to-addresses"),
        };
        let chevron = div()
            .id(chevron_id)
            .flex()
            .items_center()
            .px(px(2.))
            .cursor_pointer()
            .text_color(theme.fg_muted())
            .child("▾")
            // On press, so the field underneath doesn't take the click.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, w, cx| {
                    cx.stop_propagation();
                    this.toggle_addresses(field, w, cx)
                }),
            );
        let input = text_field(form.address_input(field), cx).placeholder("Address").suffix(Adornment::element(chevron));
        let menu = form.address_menu.as_ref().filter(|m| m.field == field).and_then(|menu| {
            let matches = form.address_matches(cx);
            if matches.is_empty() && menu.filtered {
                return None;
            }
            let rows = matches.into_iter().enumerate().map(|(i, address)| {
                let address = address.to_owned();
                div()
                    .id(ElementId::NamedInteger("address".into(), i as u64))
                    .px(px(10.))
                    .py(px(5.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .when(menu.highlighted == Some(i), |d| d.bg(theme.accent_bg()))
                    .hover(|s| s.bg(theme.accent_bg()))
                    .child(address.clone())
                    // On press: picking before the field could lose focus and close the list.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.pick_address(field, address.clone(), cx)
                        }),
                    )
            });
            let panel = menu_panel(theme, menu_id)
                .max_h(px(240.))
                .overflow_y_scroll()
                .children(rows)
                .when(form.addresses.is_empty(), |d| d.child(muted(theme, "No saved addresses").px(px(10.)).py(px(5.))));
            Some(deferred(div().absolute().top_full().left_0().right_0().mt(px(4.)).child(panel)).with_priority(3))
        });
        div().relative().key_context("AddressField").child(input).children(menu).into_any_element()
    }

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
