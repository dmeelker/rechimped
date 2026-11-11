use chrono::Duration;
use gpui::{
    AnyElement, AppContext as _, ClickEvent, Context, ElementId, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpuikit::elements::tooltip::tooltip;
use gpuikit::icons::Icons;
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ActiveTheme, Themeable};
use timechimp_api::TimeEntry;

use crate::app::TimeChimpApp;
use crate::domain::week::{CHART_WEEKS, Junction, WeekCard};

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
            .child(right_cell(60., hh_mm(e.hours)))
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
                    let row = junction_row(theme, &junction);
                    rows.push(match junction {
                        Junction::Gap { from, to } => {
                            let date = self.session.entries()[i].date;
                            row.id(ElementId::NamedInteger("gap".into(), i as u64))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.warning().opacity(0.18)))
                                .tooltip(tooltip("Double-click to add an entry for this gap"))
                                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                    if ev.click_count() >= 2 {
                                        this.fill_gap(date, from, to, window, cx);
                                    }
                                }))
                                .into_any_element()
                        }
                        Junction::Overlap { .. } => row.into_any_element(),
                    });
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
                .child(div().text_lg().text_color(hours_colour(theme, hours, target)).child(hh_mm(hours)))
                .child(meter(theme, fraction, hours_colour(theme, hours, target)))
        }));

        let card = crate::domain::week::week_card(
            self.session.entries(),
            self.session.contract_hours(),
            &self.session.settings.excluded_tasks,
        );
        let (total, contract, share) = (card.hours, card.contract, card.share);
        let goal = self.session.settings.billable_goal;
        let ratio = share.unwrap_or(0.0);
        let ratio_colour = goal_colour(theme, share, goal);
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
                // The hover target for the breakdown; the "% billable" link above has its own tooltip.
                v_stack()
                    .id("week-breakdown")
                    .gap(px(4.))
                    .tooltip(move |_, cx| cx.new(|_| WeekBreakdown { card: card.clone(), goal }).into())
                    .child(
                        // The times share a baseline; the icon (an svg has none) is centred on the
                        // row, and its top margin moves it from the line box's centre onto the
                        // digits' centre (measured).
                        h_stack()
                            .gap(px(6.))
                            .items_center()
                            .child(
                                h_stack()
                                    .gap(px(4.))
                                    .items_baseline()
                                    .child(div().text_lg().text_color(hours_colour(theme, total, contract)).child(hh_mm(total)))
                                    // Baseline alignment leaves it a pixel low (measured).
                                    .child(muted(theme, format!("/ {}", hh_mm(contract))).relative().top(px(-1.))),
                            )
                            .child(Icons::info_circled().size(px(12.)).mt(px(3.)).text_color(theme.fg_muted())),
                    )
                    .child(meter(theme, ratio as f32, ratio_colour)),
            );
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

/// The Week card's hover popover: where the week's hours went and how the billable share follows
/// from them (`domain::week::week_card`).
struct WeekBreakdown {
    card: WeekCard,
    goal: f64,
}

impl Render for WeekBreakdown {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let theme = &theme;
        let c = &self.card;
        let (billable_colour, non_billable_colour, leave_colour) =
            (theme.success(), gpuikit::theme::parse_hex("#7287fd"), theme.fg_muted().opacity(0.5));

        // Out of the contract, or of everything logged when that is more.
        let scale = c.full_contract.max(c.hours + c.leave);
        let segment = |hours: f64, colour| {
            let fraction = if scale > 0.0 { (hours / scale) as f32 } else { 0.0 };
            div().h_full().w(gpui::relative(fraction)).bg(colour)
        };
        let bar = h_stack()
            .w_full()
            .h(px(8.))
            .rounded(px(4.))
            .overflow_hidden()
            .bg(theme.surface_secondary())
            .child(segment(c.billable, billable_colour))
            .child(segment(c.non_billable, non_billable_colour))
            .child(segment(c.leave, leave_colour));

        let category = |colour, label: &str, hours: f64| {
            breakdown_row(
                h_stack().gap(px(6.)).items_center().child(div().size(px(8.)).rounded(px(2.)).bg(colour)).child(label.to_owned()),
                hh_mm(hours),
            )
        };
        let divider = || div().h(px(1.)).my(px(2.)).bg(theme.border());

        let share = match c.share {
            Some(s) => {
                let of = if c.contract > 0.0 { c.contract } else { c.hours };
                format!("{} / {} = {:.0}%", hh_mm(c.billable), hh_mm(of), s * 100.0)
            }
            None => "–".into(),
        };
        let basis = if c.full_contract > 0.0 {
            v_stack()
                .child(breakdown_row(div().child("Contract"), hh_mm(c.full_contract)))
                .child(breakdown_row(muted(theme, "minus leave"), format!("− {}", hh_mm(c.leave))))
                .child(breakdown_row(div().child("Counts against"), hh_mm(c.contract)).font_weight(FontWeight::SEMIBOLD))
        } else {
            v_stack().child(muted(theme, "No contract hours in TimeChimp, so the share is of the hours worked."))
        };

        div().pl(px(8.)).pt(px(10.)).child(
            v_stack()
                .w(px(260.))
                .gap(px(4.))
                .p(px(10.))
                .bg(theme.surface())
                .border_1()
                .border_color(theme.border())
                .rounded(px(6.))
                .shadow_md()
                .text_sm()
                .text_color(theme.fg())
                .child(div().font_weight(FontWeight::SEMIBOLD).child("This week"))
                .child(bar)
                .child(category(billable_colour, "Billable", c.billable))
                .child(category(non_billable_colour, "Non-billable", c.non_billable))
                .child(category(leave_colour, "Leave", c.leave))
                .child(divider())
                .child(breakdown_row(div().child("Worked"), hh_mm(c.hours)))
                .child(basis)
                .child(divider())
                .child(
                    breakdown_row(div().child("Billable share"), share)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(goal_colour(theme, c.share, self.goal)),
                )
                .when_some(c.billable_to_go(self.goal), |d, to_go| {
                    let goal = format!("To reach {:.0}%", self.goal * 100.0);
                    d.child(if to_go > 0.0 {
                        breakdown_row(div().child(goal), format!("{} billable to go", hh_mm(to_go)))
                    } else {
                        breakdown_row(div().child(goal), "reached").text_color(theme.success())
                    })
                })
                .child(div().text_xs().text_color(theme.fg_muted()).child(format!(
                    "{}Leave is the tasks left unchecked on Settings › Billability.",
                    // The "To reach" row names the goal already.
                    if c.billable_to_go(self.goal).is_some() { String::new() } else { format!("Goal {:.0}%. ", self.goal * 100.0) }
                ))),
        )
    }
}

fn breakdown_row(label: gpui::Div, value: impl Into<gpui::SharedString>) -> gpui::Div {
    h_stack().justify_between().gap(px(16.)).child(label).child(div().child(value.into()))
}
