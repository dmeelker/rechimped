//! Loading data: the full reload, `config.toml`, a week, a month's report.

use gpui::{Context, Window};

use crate::domain::report::Report;
use crate::domain;
use crate::net;
use crate::store::config;

use super::{Tab, TimeChimpApp};

impl TimeChimpApp {
    /// Loads everything.
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.session.client.clone() else { return };
        let (week_start, report_month, want_report) = (self.session.week_start, self.session.report_month, self.tab == Tab::Report);
        // The report is stale from here on; the Report tab loads it again when shown.
        self.session.report = Report::default();
        self.session.week_generation += 1;
        let generation = self.session.week_generation;
        let today = domain::time::today();
        let future = async move {
            let base = net::load_base(&client).await?;
            let user_id = base.me.id;
            // The history only feeds the Recent picker; failing to load it doesn't fail the reload.
            let (week, history) =
                tokio::join!(net::load_week(&client, user_id, week_start), net::load_history(&client, user_id, today, week_start));
            let report = if want_report { Some(net::load_report(&client, user_id, report_month).await?) } else { None };
            timechimp::Result::Ok((base, week?, report, history))
        };
        self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
            Ok((base, week, report, history)) => {
                // Clear the old error first: loading the config or the history may report one.
                this.error = None;
                let history = history.map_err(|e| this.set_error(format!("Could not load recent entries: {e}"), window, cx));
                this.session.apply_base(base, history.ok(), &week.entries);
                this.load_config(window, cx);
                // A report loaded meanwhile (tab switch, other month) is newer than this one.
                if let Some(report) = report.filter(|r| r.month == Some(this.session.report_month)) {
                    this.session.report = report;
                }
                if generation == this.session.week_generation {
                    this.on_week_loaded(week, None);
                }
            }
            Err(e) => this.api_error(e, window, cx),
        });
    }

    /// Reads `config.toml`, creating a starter file (listing projects and tags) if missing.
    pub(super) fn load_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match config::path().and_then(|path| Ok((config::load(&path)?, path))) {
            Ok((Some(config), _)) => {
                if let Some(warning) = self.session.apply_config(Some(config)) {
                    self.set_error(warning, window, cx);
                }
            }
            Ok((None, path)) => {
                self.session.apply_config(None);
                match config::write_template(&path, &self.session.projects, &self.session.tags) {
                    Ok(()) => self.set_info(format!("Created {}; add the tags per project there", path.display()), window, cx),
                    Err(e) => self.set_error(format!("Could not create config.toml: {e}"), window, cx),
                }
            }
            Err(e) => {
                self.session.apply_config(None);
                self.set_error(e, window, cx);
            }
        }
    }

    /// Loads the current week; afterwards selects the item with `select_id`, if given.
    pub(super) fn load_week(&mut self, select_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client.clone(), self.session.me.as_ref()) else { return };
        let (user_id, week_start) = (me.id, self.session.week_start);
        self.session.week_generation += 1;
        let generation = self.session.week_generation;
        self.run(
            "Loading…",
            async move { net::load_week(&client, user_id, week_start).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(week) if generation == this.session.week_generation => {
                    this.on_week_loaded(week, select_id);
                    this.error = None;
                }
                Ok(_) => {}
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    /// A loaded week: into the session, with the sweep animation under the header.
    fn on_week_loaded(&mut self, week: net::Week, select_id: Option<i64>) {
        self.fx.sweep += 1;
        self.session.apply_week(week, select_id);
    }

    pub(super) fn load_report(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client.clone(), self.session.me.as_ref()) else { return };
        let (user_id, month) = (me.id, self.session.report_month);
        self.run(
            "Loading…",
            async move { net::load_report(&client, user_id, month).await },
            window,
            cx,
            |this, result, window, cx| match result {
                // Only keep it if the user didn't move on to another month meanwhile.
                Ok(report) if report.month == Some(this.session.report_month) => {
                    this.session.report = report;
                    this.error = None;
                }
                Ok(_) => {}
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }
}
