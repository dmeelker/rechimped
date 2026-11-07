use gpui::{Context, Window};

use crate::domain;
use crate::net;
use crate::session;
use crate::store::config;

use super::{Tab, TimeChimpApp};

impl TimeChimpApp {
    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.session.client().cloned() else { return };
        let (report_month, want_report) = (self.session.report_month(), self.tab == Tab::Report);
        self.session.invalidate_report();
        let (generation, week_start) = self.session.begin_week_load();
        let today = domain::time::today();
        let future = async move {
            let base = net::load_base(&client).await?;
            let user_id = base.me.id;
            // Only the week is needed: a failed history (the Recent picker) or report (its tab
            // then says "Not loaded") is reported but doesn't fail the reload.
            let report = async {
                if want_report { Some(net::load_report(&client, user_id, report_month).await) } else { None }
            };
            let (week, history, report) = tokio::join!(
                net::load_week(&client, user_id, week_start),
                net::load_history(&client, user_id, today, week_start),
                report
            );
            timechimp::Result::Ok((base, week?, report, history))
        };
        self.run("Loading…", future, window, cx, move |this, result, window, cx| match result {
            Ok((base, week, report, history)) => {
                // Clear the old error first: loading the config or the history may report one.
                this.error = None;
                let history = history.map_err(|e| this.set_error(format!("Could not load recent entries: {e}"), window, cx));
                this.session.apply_base(base, history.ok(), &week.entries);
                this.load_config(window, cx);
                match report {
                    // Unless a report loaded meanwhile (tab switch, other month), which is newer.
                    Some(Ok(report)) if this.session.report().is_none() => {
                        this.session.apply_report(report);
                    }
                    Some(Err(e)) => this.set_error(format!("Could not load the report: {e}"), window, cx),
                    _ => {}
                }
                if this.session.is_latest_week_load(generation) {
                    this.on_week_loaded(week, None);
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
                self.session.apply_config(None);
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
                Ok(week) if this.session.is_latest_week_load(generation) => {
                    this.on_week_loaded(week, select_id);
                    this.error = None;
                }
                Ok(_) => {}
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    fn on_week_loaded(&mut self, week: session::Week, select_id: Option<i64>) {
        self.fx.sweep += 1;
        self.session.apply_week(week, select_id);
    }

    pub(super) fn load_report(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client().cloned(), self.session.me()) else { return };
        let (user_id, month) = (me.id, self.session.report_month());
        self.run(
            "Loading…",
            async move { net::load_report(&client, user_id, month).await },
            window,
            cx,
            |this, result, window, cx| match result {
                // Only keep it if the user didn't move on to another month meanwhile.
                Ok(report) => {
                    if this.session.apply_report(report) {
                        this.error = None;
                    }
                }
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }
}
