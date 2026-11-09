use futures::FutureExt;
use futures::future::BoxFuture;
use timechimp_api::Client;

use gpui::{AppContext, Context, Window};
use gpuikit::elements::dialog::{DialogClosed, DialogState, dialog};

use crate::session;
use crate::store::config;

use super::{DELETING, Overlay, Tab, TimeChimpApp};

impl TimeChimpApp {
    pub fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = if self.tab == Tab::Week {
            self.session.selected_entry().map(|e| {
                let notes = e.notes.as_deref().map(str::trim).filter(|n| !n.is_empty());
                (
                    DeleteTarget::Time(e.id),
                    "Delete time entry?",
                    format!(
                        "{} {} {:.2}h {} / {}{}",
                        e.date.format("%a %d %b"),
                        e.start_end.as_deref().unwrap_or(""),
                        e.hours,
                        e.project_name,
                        e.task_name,
                        notes.map(|n| format!("\n\"{n}\"")).unwrap_or_default()
                    ),
                )
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
        self.menu = None;
        let label = format!("\"{}\": {} → {}", preset.name, preset.from, preset.to);
        let target = DeleteTarget::TripPreset(preset);
        self.confirm_delete(target, "Delete preset?", label, window, cx);
    }

    pub fn ask_delete_time_preset(&mut self, preset: config::TimePreset, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
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
        self.overlay = Overlay::Confirm { dialog: state, _closed: sub };
        cx.notify();
    }

    fn delete(&mut self, target: DeleteTarget, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            DeleteTarget::Time(id) => {
                self.delete_remote(window, cx, move |client| async move { client.delete_time_entry(id).await }.boxed())
            }
            DeleteTarget::Travel(id) => {
                self.delete_remote(window, cx, move |client| async move { client.delete_mileage(id).await }.boxed())
            }
            DeleteTarget::TripPreset(p) => self.delete_preset(&p, window, cx),
            DeleteTarget::TimePreset(p) => self.delete_preset(&p, window, cx),
        }
    }

    fn delete_remote(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        request: impl FnOnce(Client) -> BoxFuture<'static, timechimp_api::Result<()>>,
    ) {
        let Some(client) = self.session.client().cloned() else { return };
        self.run(DELETING, request(client), window, cx, |this, result, window, cx| match result {
            Ok(()) => {
                this.session.invalidate_report();
                this.load_week(None, window, cx);
                this.set_info("Deleted.", window, cx);
            }
            Err(e) => this.api_error(e, window, cx),
        });
    }

    fn delete_preset(&mut self, preset: &impl config::Preset, window: &mut Window, cx: &mut Context<Self>) {
        match config::path().and_then(|path| config::remove_preset(&path, preset)) {
            Ok(()) => {
                self.load_config(window, cx);
                self.set_info("Preset deleted from config.toml", window, cx);
            }
            Err(e) => self.set_error(e, window, cx),
        }
    }
}

#[derive(Clone)]
enum DeleteTarget {
    Time(i64),
    Travel(i64),
    TripPreset(config::TripPreset),
    TimePreset(config::TimePreset),
}
