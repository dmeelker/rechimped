use chrono::{Datelike, Duration};
use gpui::{
    Context, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::tooltip::tooltip;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::Themeable;

use crate::app::{BillabilityChart, TimeChimpApp};
use crate::domain::week::WeekCard;

use super::widgets::*;

const PLOT_HEIGHT: f32 = 200.;
const LABEL_ROOM: f32 = 22.;
const BAR_WIDTH: f32 = 44.;
const GUTTER: f32 = 60.;

impl TimeChimpApp {
    pub(super) fn render_chart(&self, chart: &BillabilityChart, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let goal = self.session.settings.billable_goal;
        let body = match &chart.weeks {
            Some(weeks) => plot(theme, weeks, goal).into_any_element(),
            None => {
                let message = if self.busy().is_some() { "Loading…" } else { "Could not load these weeks." };
                div()
                    .h(px(PLOT_HEIGHT + LABEL_ROOM + 40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.fg_muted())
                    .child(message)
                    .into_any_element()
            }
        };
        let card = v_stack()
            .id("chart-card")
            .p(px(20.))
            .gap(px(4.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).child("Billability"))
            .child(muted(
                theme,
                format!(
                    "Billable hours per week as a share of the contract hours, leave left out · goal {:.0}%",
                    goal * 100.0
                ),
            ))
            .child(div().mt(px(16.)).child(body))
            .child(
                h_stack().mt(px(16.)).justify_end().child(primary_button(
                    theme,
                    "chart-close",
                    "Close",
                    false,
                    cx.listener(|this, _, w, cx| this.close_overlay(w, cx)),
                )),
            );
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, w, cx| this.close_overlay(w, cx));
        Self::modal(theme, "chart", 560., card, Some(Box::new(close)))
    }
}

fn plot(theme: &T, weeks: &[(chrono::NaiveDate, WeekCard)], goal: f64) -> impl IntoElement {
    let scale = weeks.iter().filter_map(|(_, c)| c.share).fold(1.0_f64, f64::max).max(goal);
    let y = |share: f64| (share / scale) as f32 * PLOT_HEIGHT;
    let selected = weeks.last().map(|(m, _)| *m);

    let columns = weeks.iter().enumerate().map(|(i, (monday, card))| {
        let share = card.share.unwrap_or(0.0);
        let colour = goal_colour(theme, card.share, goal);
        let label = card.share.map_or("–".into(), |s| format!("{:.0}%", s * 100.0));
        let sunday = *monday + Duration::days(6);
        let tip = format!(
            "Week {} ({} – {}): {} billable of {} contract, {} logged (leave left out)",
            monday.iso_week().week(),
            monday.format("%-d %b"),
            sunday.format("%-d %b"),
            hh_mm(card.billable),
            hh_mm(card.contract),
            hh_mm(card.hours),
        );
        let is_selected = Some(*monday) == selected;
        // The column is the hover target, larger than the bar itself.
        v_stack()
            .id(ElementId::NamedInteger("chart-week".into(), i as u64))
            .flex_1()
            .items_center()
            .justify_end()
            .h_full()
            .tooltip(tooltip(tip))
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).mb(px(4.)).child(label))
            .child(div().w(px(BAR_WIDTH)).h(px(y(share))).rounded_t(px(4.)).bg(colour))
            .when(is_selected, |d| d.font_weight(FontWeight::BOLD))
    });

    let axis_labels = weeks.iter().map(|(monday, card)| {
        let is_selected = Some(*monday) == selected;
        v_stack()
            .flex_1()
            .items_center()
            .pt(px(6.))
            .child(
                div()
                    .when(is_selected, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .child(format!("Week {}", monday.iso_week().week())),
            )
            .child(div().text_xs().text_color(theme.fg_muted()).child(format!(
                "{} / {}",
                hh_mm(card.hours),
                hh_mm(card.contract)
            )))
    });

    let goal_line = div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(y(goal)))
        .h(px(1.))
        .bg(theme.fg_muted().opacity(0.6))
        .child(
            div()
                .absolute()
                .right_0()
                .bottom(px(2.))
                .text_xs()
                .text_color(theme.fg_muted())
                .child(format!("goal {:.0}%", goal * 100.0)),
        );

    let bars = h_stack()
        .relative()
        .h(px(PLOT_HEIGHT + LABEL_ROOM))
        .items_end()
        .pr(px(GUTTER))
        .border_b_1()
        .border_color(theme.border())
        .children(columns)
        .child(goal_line);
    v_stack().child(bars).child(h_stack().pr(px(GUTTER)).children(axis_labels))
}
