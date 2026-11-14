use gpui::{Context, Window};

use crate::domain;
use crate::net;
use crate::session;
use crate::store::config;

use super::{Tab, TimeChimpApp};

impl TimeChimpApp {
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.session.client().cloned() else { return };
        self.session.invalidate_derived();
        let report_load = (self.tab == Tab::Report).then(|| self.session.begin_report_load());
        let (generation, week_start) = self.session.begin_week_load();
        let today = domain::time::today();
        let future = async move {
            let base = net::load_base(&client).await?;
            let user_id = base.me.id;
            // Only the week is needed: a failed history (the Recent picker) or report (its tab
            // then says "Not loaded") is reported but doesn't fail the reload.
            let report = async {
                match report_load {
                    Some((_, month)) => Some(net::load_report(&client, user_id, month).await),
                    None => None,
                }
            };
            let (week, history, report) = tokio::join!(
                net::load_week(&client, user_id, week_start),
                net::load_history(&client, user_id, today, week_start),
                report
            );
            timechimp_api::Result::Ok((base, week?, report, history))
        };
        self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
            Ok((base, week, report, history)) => {
                let report_failed = matches!(report, Some(Err(_)));
                // Clear the old error first: loading the config or the history may report one.
                this.error = None;
                let history = history.map_err(|e| this.set_error(format!("Could not load recent entries: {e}"), window, cx));
                this.session.apply_base(base, history.ok(), &week.entries);
                this.load_config(window, cx);
                // The catch-ups also cover moving to another week, month or tab before the user was
                // known, when no load could start.
                if this.tab == Tab::Review && this.session.review_rows().is_none() {
                    this.load_review(None, window, cx);
                }
                match (report, report_load) {
                    // Dropped when a newer load started meanwhile (tab switch, other month, a save).
                    (Some(Ok(report)), Some((report_generation, _))) => {
                        let skipped = report.skipped;
                        if this.session.apply_report(report_generation, report) {
                            this.warn_skipped(skipped, window, cx);
                        }
                    }
                    (Some(Err(e)), _) => this.set_error(format!("Could not load the report: {e}"), window, cx),
                    _ => {}
                }
                if this.tab == Tab::Report && this.session.report().is_none() && !report_failed {
                    this.load_report(window, cx);
                }
                if this.on_week_loaded(generation, week, None) {
                    this.check_travel_reminder(cx);
                } else if !this.session.week_loaded() {
                    this.load_week(None, window, cx);
                }
            }
            Err(e) => this.api_error(e, window, cx),
        });
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
