use gpui::{FontWeight, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use crate::app::TimeChimpApp;

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_report(&self, theme: &T, cx: &gpui::Context<Self>) -> impl IntoElement {
        let month = self.session().report_month().format("%B %Y").to_string();
        let Some(summary) = self.session().report_summary() else {
            let message = if self.busy().is_some() { "Loading report…" } else { NOT_LOADED };
            return panel(theme, month).child(empty_message(theme, message)).into_any_element();
        };

        let pct = |h: f64| summary.share(h) * 100.0;

        let rows = summary.groups.iter().map(|g| {
            h_stack()
                .w_full()
                .py(px(6.))
                .px(px(4.))
                .items_center()
                .border_b_1()
                .border_color(theme.border_subtle())
                .when(!g.billable, |d| d.text_color(theme.fg_muted()).bg(non_billable_bg(false)))
                .child(cell(None, g.project.clone()))
                .child(cell(None, g.task.clone()))
                .child(right_cell(70., hh_mm(g.hours)))
                .child(right_cell(50., format!("{:.0}%", pct(g.hours))))
                .child(div().w(px(100.)).flex_none().px(px(6.)).child(meter(
                    theme,
                    summary.share(g.hours) as f32,
                    if g.billable { theme.success() } else { theme.fg_muted() },
                )))
        });
        let table = panel(theme, format!("{month} · hours per project / task"))
            .flex_1()
            .child(header_row(
                theme,
                [cell(None, "Project"), cell(None, "Task"), right_cell(70., "Hours"), right_cell(50., "%"), cell(Some(100.), "")],
            ))
            .child(table_rows(theme, "report-rows", rows, &self.report_scroll, cx.entity_id()))
            .when(summary.groups.is_empty(), |d| d.child(empty_message(theme, "No time registered this month.")));

        let stat = |label: &'static str, value: String| {
            h_stack()
                .justify_between()
                .py(px(4.))
                .child(muted(theme, label))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(value))
        };
        let s = summary;
        let totals = panel(theme, "Summary").w(px(300.)).flex_none().child(
            v_stack()
                .p(px(12.))
                .child(stat("Total hours", hh_mm(s.total)))
                .child(
                    stat("Billable", format!("{} ({:.0}%)", hh_mm(s.billable), pct(s.billable)))
                        .when_some(s.billable_share(), |d, share| {
                            d.text_color(goal_colour(theme, Some(share), self.session().settings().billable_goal))
                        }),
                )
                .child(stat("Billable goal", format!("{:.0}%", self.session().settings().billable_goal * 100.0)))
                .child(stat("Non-billable", hh_mm(s.total - s.billable)))
                .child(stat("Days registered", s.days.to_string()))
                .child(stat("Entries", s.entries.to_string()))
                .child(div().h(px(1.)).my(px(8.)).bg(theme.border()))
                .child(stat("Trips", s.trips.to_string()))
                .child(stat("Distance", format!("{:.0} km", s.km)))
                .child(stat("Reimbursement", format!("€ {:.2}", s.reimbursement))),
        );
        h_stack()
            .flex_1()
            .min_h_0()
            .items_start()
            .gap(px(12.))
            .child(table.max_h_full())
            .child(totals.max_h_full())
            .into_any_element()
    }
}
