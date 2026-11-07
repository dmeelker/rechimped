//! The Report tab: hours per project / task and the month's totals.

use gpui::{FontWeight, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use gpuikit::elements::progress::progress;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use crate::app::TimeChimpApp;

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_report(&self, theme: &T) -> impl IntoElement {
        let month = self.session.report_month.format("%B %Y").to_string();
        let report = &self.session.report;
        if report.month != Some(self.session.report_month) {
            return panel(theme, month, None).child(empty_message(theme, "Loading report…")).into_any_element();
        }

        let summary = report.summary();
        let pct = |h: f64| summary.share(h) * 100.0;

        let rows = summary.groups.iter().map(|g| {
            h_stack()
                .w_full()
                .py(px(6.))
                .px(px(4.))
                .items_center()
                .border_b_1()
                .border_color(theme.border_subtle())
                .when(!g.billable, |d| d.text_color(theme.fg_muted()))
                .child(cell(None, g.project.clone()))
                .child(cell(None, g.task.clone()))
                .child(right_cell(70., format!("{:.2}", g.hours)))
                .child(right_cell(50., format!("{:.0}%", pct(g.hours))))
                .child(div().w(px(100.)).flex_none().px(px(6.)).child(progress(summary.share(g.hours) as f32)))
        });
        let table = panel(theme, format!("{month} · hours per project / task"), None)
            .flex_1()
            .child(header_row(
                theme,
                [cell(None, "Project"), cell(None, "Task"), right_cell(70., "Hours"), right_cell(50., "%"), cell(Some(100.), "")],
            ))
            .child(table_rows("report-rows", rows))
            .when(summary.groups.is_empty(), |d| d.child(empty_message(theme, "No time registered this month.")));

        let stat = |label: &'static str, value: String| {
            h_stack()
                .justify_between()
                .py(px(4.))
                .child(muted(theme, label))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(value))
        };
        let s = &summary;
        let totals = panel(theme, "Summary", None).w(px(300.)).flex_none().child(
            v_stack()
                .p(px(12.))
                .child(stat("Total hours", format!("{:.2}", s.total)))
                .child(stat("Billable", format!("{:.2} ({:.0}%)", s.billable, pct(s.billable))).when_some(
                    s.billable_share(),
                    |d, share| d.text_color(if share >= self.session.billable_goal { theme.success() } else { theme.warning() }),
                ))
                .child(stat("Billable goal", format!("{:.0}%", self.session.billable_goal * 100.0)))
                .child(stat("Non-billable", format!("{:.2}", s.total - s.billable)))
                .child(stat("Days registered", s.days.to_string()))
                .child(stat("Entries", s.entries.to_string()))
                .child(div().h(px(1.)).my(px(8.)).bg(theme.border()))
                .child(stat("Trips", s.trips.to_string()))
                .child(stat("Distance", format!("{:.0} km", s.km)))
                .child(stat("Reimbursement", format!("€ {:.2}", s.reimbursement))),
        );
        // Panels are at most as tall as the body (`max_h_full`), so a long table scrolls.
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
