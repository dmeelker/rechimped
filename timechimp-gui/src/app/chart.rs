//! The billability chart: the Week card's billable share for the selected week and the ones before.

use chrono::NaiveDate;
use gpui::{Context, Window};

use crate::domain::week::{self, CHART_WEEKS, WeekCard};
use crate::net;

use super::TimeChimpApp;

/// The open billability chart.
pub struct BillabilityChart {
    /// The Mondays of the weeks shown, oldest first; the last is the week selected when it opened.
    pub mondays: Vec<NaiveDate>,
    /// Each week's figures, in `mondays` order; None while loading or after loading failed.
    pub weeks: Option<Vec<(NaiveDate, WeekCard)>>,
}

impl TimeChimpApp {
    /// Opens the chart for the selected week and the ones before it, fetched afresh (the weeks
    /// may lie outside what is loaded already).
    pub fn open_chart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client.clone(), self.session.me.as_ref()) else { return };
        let (user_id, contract) = (me.id, self.session.contract_hours());
        let mondays = week::weeks_up_to(self.session.week_start(), CHART_WEEKS);
        self.chart = Some(BillabilityChart { mondays: mondays.clone(), weeks: None });
        self.menu = None;
        cx.notify();
        let wanted = mondays.clone();
        self.run(
            "Loading…",
            async move { net::load_weeks(&client, user_id, &mondays).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                // Unless it was closed (or reopened for other weeks) meanwhile.
                Ok(entries) => {
                    if let Some(chart) = this.chart.as_mut().filter(|c| c.mondays == wanted) {
                        chart.weeks = Some(week::week_cards(&entries, &wanted, contract));
                    }
                }
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    pub fn close_chart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chart = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
