//! The Mileage tab: the week's trips per day.

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use gpuikit::layout::h_stack;
use gpuikit::theme::Themeable;

use crate::app::TimeChimpApp;
use crate::domain::mileage_type_name;

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn trip_row(&self, theme: &T, i: usize, m: &timechimp::Mileage, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.session.selected_mileage == Some(i);
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

    /// The Mileage tab: the week's trips grouped per day (`net::load_week` sorts them by day),
    /// each day under a header with its total.
    pub(super) fn render_mileage(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let total: f64 = self.session.mileage.iter().map(|m| m.distance).sum();
        let actions = self.selection_actions(theme, cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut start = 0;
        for (n, day) in self.session.mileage.chunk_by(|a, b| a.date == b.date).enumerate() {
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
        let empty = self.session.mileage.is_empty() && self.busy().is_none();
        panel(theme, format!("Trips · {} km", trim_hours(total)), Some(actions))
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
            .when(empty, |d| d.child(empty_message(theme, "No mileage this week. Press A to add.")))
    }
}
