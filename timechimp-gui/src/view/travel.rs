use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use gpuikit::layout::h_stack;
use gpuikit::theme::Themeable;

use crate::app::TimeChimpApp;
use crate::domain::travel_type_name;

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn trip_row(&self, theme: &T, i: usize, m: &timechimp::Mileage, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, selected) = (m.id, self.session.selected_travel_id() == Some(m.id));
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
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| this.click_travel(id, ev.click_count(), window, cx)))
            .child(right_cell(60., format!("{}", m.distance)))
            .child(cell(None, m.from_address.clone().unwrap_or_default()))
            .child(cell(None, m.to_address.clone().unwrap_or_default()))
            .child(cell(Some(150.), m.project_name.clone()))
            .child(cell(Some(110.), travel_type_name(m.mileage_type)))
            .child(cell(Some(160.), m.notes.clone().unwrap_or_default()))
    }

    pub(super) fn render_travel(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let total: f64 = self.session.travel().iter().map(|m| m.distance).sum();
        let actions = self.selection_actions(theme, cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut start = 0;
        for (n, day) in self.session.travel().chunk_by(|a, b| a.date == b.date).enumerate() {
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
                    .child(muted(theme, format!("{trips} · {} km", plain_number(km))))
                    .into_any_element(),
            );
            for (k, m) in day.iter().enumerate() {
                rows.push(self.trip_row(theme, start + k, m, cx).into_any_element());
            }
            start += day.len();
        }
        let empty = self.session.travel().is_empty() && self.busy().is_none();
        let message = if self.session.week_loaded() { "No travel this week. Press A to add." } else { NOT_LOADED };
        panel(theme, format!("Trips · {} km", plain_number(total)), Some(actions))
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
            .when(empty, |d| d.child(empty_message(theme, message)))
    }
}
