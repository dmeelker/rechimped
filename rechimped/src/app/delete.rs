use chrono::NaiveDate;
use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{AppContext, Context, Window};
use gpuikit::elements::dialog::{DialogClosed, DialogState, dialog};

use crate::backend::Api;
use crate::session;
use crate::store::config;

use super::{Overlay, Tab, TimeChimpApp};

impl TimeChimpApp {
    pub fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_locked(window, cx) {
            return;
        }
        let target = if self.tab == Tab::Week {
            self.session.selected_entry().map(|e| {
                let label = entry_label(e.date, e.start_end.as_deref(), e.hours, &e.project_name, &e.task_name, e.notes.as_deref());
                (DeleteTarget::Time { id: e.id, review_date: None }, "Delete time entry?", label)
            })
        } else if self.tab == Tab::Review {
            self.session.selected_review().map(|r| {
                let label = entry_label(r.date, r.start_end.as_deref(), r.time, &r.project, &r.task, r.notes.as_deref());
                (DeleteTarget::Time { id: r.id, review_date: Some(r.date) }, "Delete time entry?", label)
            })
        } else {
            self.session.selected_travel().map(|m| {
                (
                    DeleteTarget::Travel(m.id),
                    "Delete trip?",
                    format!(
                        "{} {} km {} → {}",
                        m.date.format("%a %d %b"),
                        m.distance,
                        m.from_address.as_deref().unwrap_or("?"),
                        m.to_address.as_deref().unwrap_or("?")
                    ),
                )
            })
        };
        let Some((target, question, label)) = target else { return };
        self.confirm_delete(target, question, label, window, cx);
    }

    pub fn ask_delete_trip_preset(&mut self, preset: config::TripPreset, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        let label = format!("\"{}\": {} → {}", preset.name, preset.from, preset.to);
        let target = DeleteTarget::TripPreset(preset);
        self.confirm_delete(target, "Delete preset?", label, window, cx);
    }

    pub fn ask_delete_time_preset(&mut self, preset: config::TimePreset, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        let task = preset.task.as_deref().map(|t| format!(" / {t}")).unwrap_or_default();
        let label = format!("\"{}\": {}{task}", session::time_preset_label(&preset), preset.project);
        let target = DeleteTarget::TimePreset(preset);
        self.confirm_delete(target, "Delete preset?", label, window, cx);
    }

    fn confirm_delete(
        &mut self,
        target: DeleteTarget,
        question: &'static str,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.overlay_open() {
            return;
        }
        let this = cx.weak_entity();
        let state = cx.new(|_| {
            DialogState::new(dialog("confirm-delete").confirm(question, label).confirm_label("Delete").on_confirm(
                move |window, cx| {
                    let target = target.clone();
                    let _ = this.update(cx, |this, cx| this.delete(target, window, cx));
                },
            ))
        });
        let sub = cx.subscribe_in(&state, window, |this, _, _: &DialogClosed, window, cx| {
            if matches!(this.overlay, Overlay::Confirm { .. }) {
                this.close_overlay(window, cx);
            }
        });
        state.update(cx, |d, cx| d.open(window, cx));
        self.open_overlay(Overlay::Confirm { dialog: state, _closed: sub }, cx);
    }

    fn delete(&mut self, target: DeleteTarget, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            DeleteTarget::Time { id, review_date } => self.delete_remote(review_date, window, cx, move |client| {
                async move { client.delete_time_entry(id).await }.boxed()
            }),
            DeleteTarget::Travel(id) => {
                self.delete_remote(None, window, cx, move |client| async move { client.delete_mileage(id).await }.boxed())
            }
            DeleteTarget::TripPreset(p) => self.delete_preset(&p, window, cx),
            DeleteTarget::TimePreset(p) => self.delete_preset(&p, window, cx),
        }
    }

    /// `review_date` is the deleted entry's date when deleted on the Review tab.
    fn delete_remote(
        &mut self,
        review_date: Option<NaiveDate>,
        window: &mut Window,
        cx: &mut Context<Self>,
        request: impl FnOnce(Api) -> BoxFuture<'static, timechimp_api::Result<()>>,
    ) {
        let Some(client) = self.session.client().cloned() else { return };
        self.run_write("Deleting…", request(client), window, cx, move |this, result, window, cx| match result {
            Ok(()) => {
                this.session.invalidate_report();
                this.session.invalidate_review();
                match review_date {
                    Some(date) => this.reload_after_review_change(&[date], None, window, cx),
                    None => this.load_week(None, window, cx),
                }
                this.set_info("Deleted.", window, cx);
            }
            Err(e) => this.api_error(e, window, cx),
        });
    }

    fn delete_preset(&mut self, preset: &impl config::Preset, window: &mut Window, cx: &mut Context<Self>) {
        let removed = self.session.config_writable().and_then(|()| config::path()).and_then(|path| config::remove_preset(&path, preset));
        match removed {
            Ok(()) => {
                self.load_config(window, cx);
                self.set_info("Preset deleted from config.toml", window, cx);
            }
            Err(e) => self.set_error(e, window, cx),
        }
    }
}

/// A time entry as the delete dialog names it.
fn entry_label(date: NaiveDate, start_end: Option<&str>, hours: f64, project: &str, task: &str, notes: Option<&str>) -> String {
    let notes = notes.map(str::trim).filter(|n| !n.is_empty());
    format!(
        "{} {} {} {project} / {task}{}",
        date.format("%a %d %b"),
        start_end.unwrap_or(""),
        crate::domain::hh_mm(hours),
        notes.map(|n| format!("\n\"{n}\"")).unwrap_or_default()
    )
}

#[derive(Clone)]
enum DeleteTarget {
    /// `review_date` is the entry's date when deleted on the Review tab.
    Time { id: i64, review_date: Option<NaiveDate> },
    Travel(i64),
    TripPreset(config::TripPreset),
    TimePreset(config::TimePreset),
}
