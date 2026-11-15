use std::future::Future;

use chrono::NaiveDate;
use gpui::{Context, Window};
use timechimp_api::TimeEntry;

use crate::backend::Api;
use crate::domain::report::Report;
use crate::net;
use crate::session;
use crate::store::config;

use super::{Tab, TimeChimpApp};

impl TimeChimpApp {
    /// With the user known, the week, history and report start straight away from the base data
    /// in hand, and the base refreshes alongside them: its per-project tasks take one request per
    /// project. Before that (logging in), they wait for the base, which says who the user is.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.session.client().cloned() else { return };
        self.session.invalidate_derived();
        // Cleared now rather than on success: the loads below finish in any order, and any of them
        // (or loading the config) may report one.
        self.error = None;
        let plan = plan_reload(self.tab, self.session.me().map(|m| m.id));
        let report_load = plan.report.then(|| self.session.begin_report_load());
        let (generation, week_start) = self.session.begin_week_load();
        let base_generation = self.session.begin_base_load();
        let today = self.session.today();
        let data = Data { generation, report_load };
        let report_month = report_load.map(|(_, month)| month);
        let Some(user_id) = plan.user_id else {
            let future = async move {
                let base = net::load_base(&client).await?;
                let loaded = load_data(&client, base.me.id, week_start, today, report_month).await?;
                timechimp_api::Result::Ok((base, loaded))
            };
            self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
                Ok((base, loaded)) => {
                    this.session.apply_base(base_generation, base);
                    this.load_config(window, cx);
                    let (week_applied, report_failed) = this.apply_data(data, loaded, window, cx);
                    // The catch-ups cover moving to another week, month or tab before the user was
                    // known, when no load could start.
                    if this.tab == Tab::Review && this.session.review_rows().is_none() {
                        this.load_review(None, window, cx);
                    }
                    if this.tab == Tab::Report && this.session.report().is_none() && !report_failed {
                        this.load_report(window, cx);
                    }
                    if !week_applied && !this.session.week().loaded() {
                        this.load_week(None, window, cx);
                    }
                }
                Err(e) => this.api_error(e, window, cx),
            });
            return;
        };

        let base_client = client.clone();
        self.run("Loading…", async move { net::load_base(&base_client).await }, window, cx, move |this, result, window, cx| {
            match result {
                Ok(base) => {
                    this.session.apply_base(base_generation, base);
                }
                // Logged out: there's no session left to load the config for.
                Err(e) if e.is_auth() => return this.api_error(e, window, cx),
                // The old base stays.
                Err(e) => this.api_error(e, window, cx),
            }
            // Here rather than with the week: its presets and project tags resolve against the base.
            this.load_config(window, cx);
        });
        if plan.review {
            self.load_review(None, window, cx);
        }
        let future = load_data(&client, user_id, week_start, today, report_month);
        self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
            Ok(loaded) => {
                this.apply_data(data, loaded, window, cx);
            }
            Err(e) => this.api_error(e, window, cx),
        });
    }

    /// Applies what `load_data` brought back for `data`. Returns whether the week was applied, and
    /// whether the report failed.
    fn apply_data(&mut self, data: Data, loaded: Loaded, window: &mut Window, cx: &mut Context<Self>) -> (bool, bool) {
        let Loaded { week, history, report } = loaded;
        match history {
            Ok(history) => self.session.apply_history(history, &week.entries),
            Err(e) => self.set_error(format!("Could not load recent entries: {e}"), window, cx),
        }
        let report_failed = matches!(report, Some(Err(_)));
        match (report, data.report_load) {
            // Dropped when a newer load started meanwhile (tab switch, other month, a save).
            (Some(Ok(report)), Some((report_generation, _))) => {
                let skipped = report.skipped;
                if self.session.apply_report(report_generation, report) {
                    self.warn_skipped(skipped, window, cx);
                }
            }
            (Some(Err(e)), _) => self.set_error(format!("Could not load the report: {e}"), window, cx),
            _ => {}
        }
        let week_applied = self.on_week_loaded(data.generation, week, None);
        if week_applied {
            self.check_travel_reminder(cx);
        }
        (week_applied, report_failed)
    }

    pub(super) fn load_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match config::path().and_then(|path| Ok((config::load(&path)?, path))) {
            Ok((Some(config), _)) => {
                if let Some(warning) = self.session.apply_config(Some(config)) {
                    self.set_error(warning, window, cx);
                }
            }
            Ok((None, path)) => {
                self.session.apply_config(None);
                match config::write_template(&path, self.session.projects(), self.session.tags()) {
                    Ok(()) => self.set_info(format!("Created {}", path.display()), window, cx),
                    Err(e) => self.set_error(format!("Could not create config.toml: {e}"), window, cx),
                }
            }
            Err(e) => {
                self.session.apply_config_error(e.clone());
                self.set_error(e, window, cx);
            }
        }
    }

    pub(super) fn load_week(&mut self, select_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client().cloned(), self.session.me()) else { return };
        let user_id = me.id;
        let (generation, week_start) = self.session.begin_week_load();
        self.run(
            "Loading…",
            async move { net::load_week(&client, user_id, week_start).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(week) => {
                    if this.on_week_loaded(generation, week, select_id) {
                        this.error = None;
                    }
                }
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    /// False when `week` was dropped, as for a week already left.
    fn on_week_loaded(&mut self, generation: u64, week: session::Week, select_id: Option<i64>) -> bool {
        let applied = self.session.apply_week(generation, week, select_id);
        if applied {
            self.fx.sweep += 1;
        }
        applied
    }

    pub(super) fn load_report(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client().cloned(), self.session.me()) else { return };
        let user_id = me.id;
        let (generation, month) = self.session.begin_report_load();
        self.run(
            "Loading…",
            async move { net::load_report(&client, user_id, month).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                // Only keep it if the user didn't move on to another month or change data meanwhile.
                Ok(report) => {
                    let skipped = report.skipped;
                    if this.session.apply_report(generation, report) {
                        this.error = None;
                        this.warn_skipped(skipped, window, cx);
                    }
                }
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    /// Rows of a report grid that couldn't be read are left out: say so, as figures then fall short.
    pub(super) fn warn_skipped(&mut self, skipped: usize, window: &mut Window, cx: &mut Context<Self>) {
        if skipped > 0 {
            let rows = if skipped == 1 { "row" } else { "rows" };
            self.set_error(format!("Left out {skipped} {rows} from TimeChimp that couldn't be read"), window, cx);
        }
    }
}

/// The loads a reload started, to apply what they bring back.
#[derive(Clone, Copy)]
struct Data {
    generation: u64,
    report_load: Option<(u64, NaiveDate)>,
}

struct Loaded {
    week: session::Week,
    history: timechimp_api::Result<Vec<TimeEntry>>,
    report: Option<timechimp_api::Result<Report>>,
}

/// Only the week is needed: a failed history (the Recent picker) or report (its tab then says
/// "Not loaded") is reported but doesn't fail the reload.
fn load_data(
    client: &Api,
    user_id: i64,
    week_start: NaiveDate,
    today: NaiveDate,
    report_month: Option<NaiveDate>,
) -> impl Future<Output = timechimp_api::Result<Loaded>> + Send + 'static {
    let client = client.clone();
    async move {
        let report = async {
            match report_month {
                Some(month) => Some(net::load_report(&client, user_id, month).await),
                None => None,
            }
        };
        let (week, history, report) = tokio::join!(
            net::load_week(&client, user_id, week_start),
            net::load_history(&client, user_id, today, week_start),
            report
        );
        Ok(Loaded { week: week?, history, report })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ReloadPlan {
    /// None while the user isn't known: the base then loads first, as the rest needs its user id.
    pub user_id: Option<i64>,
    /// The report loads with the week.
    pub report: bool,
    /// The review loads on its own; without a user it catches up once the base is in.
    pub review: bool,
}

/// What a reload with `tab` showing starts, for `user_id` if the base is loaded already.
pub(super) fn plan_reload(tab: Tab, user_id: Option<i64>) -> ReloadPlan {
    ReloadPlan { user_id, report: tab == Tab::Report, review: tab == Tab::Review && user_id.is_some() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_in_loads_the_base_first() {
        assert_eq!(plan_reload(Tab::Week, None), ReloadPlan { user_id: None, report: false, review: false });
        assert_eq!(plan_reload(Tab::Report, None), ReloadPlan { user_id: None, report: true, review: false });
        assert_eq!(plan_reload(Tab::Review, None), ReloadPlan { user_id: None, report: false, review: false });
    }

    #[test]
    fn a_known_user_loads_without_waiting_for_the_base() {
        assert_eq!(plan_reload(Tab::Travel, Some(7)), ReloadPlan { user_id: Some(7), report: false, review: false });
        assert_eq!(plan_reload(Tab::Report, Some(7)), ReloadPlan { user_id: Some(7), report: true, review: false });
        assert_eq!(plan_reload(Tab::Review, Some(7)), ReloadPlan { user_id: Some(7), report: false, review: true });
    }
}
