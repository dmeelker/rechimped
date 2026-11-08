use chrono::Duration;
use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::tooltip::tooltip;
use gpuikit::icons::Icons;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;
use timechimp::TimeEntry;

use crate::app::TimeChimpApp;
use crate::domain::week::CHART_WEEKS;

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
                cell(Some(ROW_ACTIONS_WIDTH), ""),
            ],
        )
    }

    pub(super) fn entry_row(&self, theme: &T, index: usize, e: &TimeEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, selected) = (e.id, self.session.selected_entry_id() == Some(e.id));
        h_stack()
            .id(ElementId::NamedInteger("entry".into(), index as u64))
            .w_full()
            .py(px(6.))
            .px(px(4.))
            .border_b_1()
            .border_color(theme.border_subtle())
            .cursor_pointer()
            .when(!e.billable, |d| d.text_color(theme.fg_muted()).bg(non_billable_bg(false)))
            .when(selected, |d| d.bg(theme.accent_bg()))
            .when(!selected, |d| d.hover(|s| s.bg(row_hover_bg(theme, e.billable))))
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| this.click_entry(id, ev.click_count(), window, cx)))
            .child(cell(Some(110.), e.start_end.clone().unwrap_or_else(|| "–".into())))
            .child(right_cell(60., format!("{:.2}", e.hours)))
            .child(cell(Some(160.), e.customer_name.clone()))
            .child(cell(None, format!("{} / {}", e.project_name, e.task_name)))
            .child(cell(None, e.notes.clone().unwrap_or_default()))
            .child(cell(Some(140.), e.tag_names.join(", ")).text_color(theme.fg_muted()))
            .child(self.row_actions(selected, cx))
    }

    pub(super) fn entry_rows_with_gaps(&self, theme: &T, indices: &[usize], cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        for (n, &i) in indices.iter().enumerate() {
            if let Some(&prev) = n.checked_sub(1).and_then(|p| indices.get(p))
                && let Some(junction) = crate::domain::week::junction(&self.session.entries()[prev], &self.session.entries()[i]) {
                    rows.push(junction_row(theme, &junction).into_any_element());
                }
            rows.push(self.entry_row(theme, i, &self.session.entries()[i], cx).into_any_element());
        }
        rows
    }

    pub(super) fn render_day(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let per_day = self.session.hours_per_day();

        let shown = self.session.shown_days(crate::domain::time::today());
        let strip = h_stack().flex_none().gap(px(8.)).children(shown.into_iter().map(|i| {
            let day = self.session.week_start() + Duration::days(i as i64);
            let (hours, target) = (per_day[i], self.session.target_for(i));
            let selected = day == self.session.day();
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
                .child(div().text_lg().text_color(hours_colour(theme, hours, target)).child(format!("{}h", plain_number(hours))))
                .child(meter(theme, fraction, hours_colour(theme, hours, target)))
        }));

        let crate::domain::week::WeekCard { hours: total, contract, share, .. } =
            crate::domain::week::week_card(self.session.entries(), self.session.contract_hours());
        let ratio = share.unwrap_or(0.0);
        let ratio_colour = goal_colour(theme, share, self.session.settings.billable_goal);
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
                        h_stack()
                            .id("open-billability-chart")
                            .gap(px(4.))
                            .items_center()
                            .px(px(4.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.accent_bg()))
                            .tooltip(tooltip(format!("Billability of the last {CHART_WEEKS} weeks")))
                            .on_click(cx.listener(|this, _, window, cx| this.open_chart(window, cx)))
                            .child(div().text_xs().text_color(ratio_colour).child(format!("{:.0}% billable", ratio * 100.0)))
                            .child(Icons::bar_chart().size(px(12.)).text_color(theme.fg_muted())),
                    ),
            )
            .child(
                h_stack()
                    .gap(px(4.))
                    .items_baseline()
                    .child(div().text_lg().text_color(hours_colour(theme, total, contract)).child(format!("{}h", plain_number(total))))
                    .child(muted(theme, format!("/ {}h", plain_number(contract)))),
            )
            .child(meter(theme, ratio as f32, ratio_colour));
        let strip = strip.child(week_card);

        let visible = self.session.visible_entries();
        let rows = self.entry_rows_with_gaps(theme, &visible, cx);
        let message =
            if self.session.week_loaded() { "No time registered on this day. Press A to add." } else { NOT_LOADED };

        v_stack()
            .flex_1()
            .min_h_0()
            .gap(px(12.))
            .child(strip)
            .child(
                table_card(theme)
                    .child(Self::entry_header(theme).rounded_t(px(5.)))
                    .child(table_rows("entry-rows", rows))
                    .when(
                    visible.is_empty() && self.busy().is_none(),
                    |d| d.child(empty_message(theme, message)),
                ),
            )
    }
}
