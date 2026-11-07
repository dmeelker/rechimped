//! The Week tab: the week strip and the selected day's entries.

use chrono::Duration;
use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::progress::{ProgressVariant, progress};
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;
use timechimp::TimeEntry;

use crate::app::TimeChimpApp;

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn entry_header(theme: &T) -> gpui::Div {
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

    pub(super) fn entry_row(&self, theme: &T, index: usize, e: &TimeEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.session.selected_entry == Some(index);
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
    pub(super) fn entry_rows_with_gaps(&self, theme: &T, indices: &[usize], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        for (n, &i) in indices.iter().enumerate() {
            if let Some(&prev) = n.checked_sub(1).and_then(|p| indices.get(p))
                && let Some(junction) = crate::domain::week::junction(&self.session.entries[prev], &self.session.entries[i]) {
                    rows.push(junction_row(theme, &junction).into_any_element());
                }
            rows.push(self.entry_row(theme, i, &self.session.entries[i], cx).into_any_element());
        }
        rows
    }

    /// The Week tab: a strip with the week's days on top, the selected day's entries below.
    pub(super) fn render_day(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let per_day = self.session.hours_per_day();
        let total: f64 = per_day.iter().sum();
        let contract = self.session.contract_hours();

        let strip = h_stack().flex_none().gap(px(8.)).children((0..7).map(|i| {
            let day = self.session.week_start + Duration::days(i as i64);
            let (hours, target) = (per_day[i], self.session.target_for(i));
            let selected = day == self.session.day;
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
        let share = crate::domain::week::billable_share(&self.session.entries, contract);
        let ratio = share.unwrap_or(0.0);
        let goal_met = share.is_some_and(|s| s >= self.session.billable_goal);
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

        let index = (self.session.day - self.session.week_start).num_days() as usize;
        let (hours, target) = (self.session.hours_per_day()[index], self.session.target_for(index));
        let visible = self.session.visible_entries();
        let title = h_stack()
            .gap(px(4.))
            .child(self.session.day.format("%A %-d %B").to_string())
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
}
