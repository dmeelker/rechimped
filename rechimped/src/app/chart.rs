use chrono::NaiveDate;
use gpui::{Context, Window};

use crate::domain::week::{self, CHART_WEEKS, WeekCard};
use crate::net;

use super::{Overlay, TimeChimpApp};

pub struct BillabilityChart {
    pub mondays: Vec<NaiveDate>,
    pub weeks: Option<Vec<(NaiveDate, WeekCard)>>,
}

impl TimeChimpApp {
    /// The weeks may lie outside what is loaded.
    pub fn open_chart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.me().is_none() {
            return;
        }
        let mondays = week::weeks_up_to(self.session.week_start(), CHART_WEEKS);
        if self.open_overlay(Overlay::Chart(BillabilityChart { mondays, weeks: None }), cx) {
            self.load_chart(window, cx);
        }
    }

    /// Loads the open chart's weeks (again, after a save or delete).
    pub(super) fn load_chart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client().cloned(), self.session.me()) else { return };
        let (user_id, contract, version) = (me.id, self.session.contract_hours(), self.session.data_version());
        let Overlay::Chart(chart) = &mut self.overlay else { return };
        chart.weeks = None;
        let mondays = chart.mondays.clone();
        let wanted = mondays.clone();
        self.run(
            "Loading…",
            async move { net::load_weeks(&client, user_id, &mondays).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                // Unless it was closed (or reopened for other weeks), or data changed, meanwhile.
                Ok(entries) => {
                    if let Overlay::Chart(chart) = &mut this.overlay
                        && chart.mondays == wanted
                        && this.session.data_version() == version
                    {
                        chart.weeks =
                            Some(week::week_cards(&entries, &wanted, contract, &this.session.settings().excluded_tasks));
                    }
                }
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }
}
