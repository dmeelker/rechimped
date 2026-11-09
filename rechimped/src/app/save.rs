use chrono::NaiveDate;
use gpui::{Context, Window};
use timechimp_api::NewMileage;

use crate::forms::{Form, FormBody};
use crate::validate::{self, SaveRequest};
use crate::session;
use crate::store::config;

use super::{SAVING, TimeChimpApp};

impl TimeChimpApp {
    pub fn save_as_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = match self.form() {
            Some(Form { body: FormBody::Travel(f), .. }) => validate::preset(&f.values(cx), &self.session)
                .and_then(|p| config::path().and_then(|path| config::append_preset(&path, &p)).map(|_| p.name)),
            Some(Form { body: FormBody::Time(f), .. }) => validate::time_preset(&f.values(cx), &self.session).and_then(|p| {
                let label = session::time_preset_label(&p).to_owned();
                config::path().and_then(|path| config::append_preset(&path, &p)).map(|_| label)
            }),
            None => return,
        };
        match saved {
            Ok(name) => {
                if let Some(form) = self.form_mut() {
                    form.error = None;
                }
                self.load_config(window, cx);
                self.set_info(format!("Preset \"{name}\" added to config.toml"), window, cx);
            }
            Err(e) => self.form_error(e, cx),
        }
    }

    pub(super) fn form_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        if let Some(form) = self.form_mut() {
            form.error = Some(message.into());
        }
        cx.notify();
    }

    pub fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Only another save blocks this one; a load running meanwhile doesn't.
        if self.saving() {
            return;
        }
        let form_id = self.form().map(|f| f.id);
        let Some(client) = self.session.client().cloned() else { return };
        let (date, request) = match self.build_save(cx) {
            Ok(r) => r,
            Err(message) => return self.form_error(message, cx),
        };
        let future = async move {
            match request {
                SaveRequest::Time { id: Some(id), entry } => {
                    Ok(Saved::Done(client.update_time_entry(id, &entry).await?.id, "Time entry updated."))
                }
                SaveRequest::Time { id: None, entry } => {
                    Ok(Saved::Done(client.create_time_entry(&entry).await?.id, "Time entry saved."))
                }
                SaveRequest::Travel { trip, editing: Some(original), .. } => {
                    Ok(Saved::Done(client.update_mileage(&original, &trip).await?.id, "Trip updated."))
                }
                SaveRequest::Travel { trip, return_trip, editing: None } => {
                    let id = client.create_mileage(&trip).await?.id;
                    if !return_trip {
                        return Ok(Saved::Done(id, "Trip saved."));
                    }
                    let back = NewMileage { from_address: trip.to_address.clone(), to_address: trip.from_address.clone(), ..trip };
                    match client.create_mileage(&back).await {
                        Ok(_) => Ok(Saved::Done(id, "Trip saved (return trip included).")),
                        // The outbound trip is in: closing the form keeps Save from adding it twice.
                        Err(error) => Ok(Saved::Partial(id, error)),
                    }
                }
            }
        };
        self.run(SAVING, future, window, cx, move |this, result: timechimp_api::Result<Saved>, window, cx| {
            let (id, outcome) = match result {
                Ok(Saved::Done(id, message)) => (id, Ok(message)),
                Ok(Saved::Partial(id, e)) => (id, Err(e)),
                Err(e) => return this.form_api_error(form_id, e, window, cx),
            };
            // Unless it was cancelled and another form opened meanwhile: that one stays.
            if this.form().map(|f| f.id) == form_id {
                this.close_overlay(window, cx);
            }
            this.session.select_day(date);
            this.session.invalidate_report();
            this.load_week(Some(id), window, cx);
            match outcome {
                Ok(message) => this.set_info(message, window, cx),
                Err(e) if e.is_auth() => this.api_error(e, window, cx),
                Err(e) => this.set_error(format!("Outbound trip saved, but the return trip failed: {e}"), window, cx),
            }
        });
    }

    fn build_save(&self, cx: &Context<Self>) -> Result<(NaiveDate, SaveRequest), String> {
        match &self.form().ok_or("No form open")?.body {
            FormBody::Time(f) => validate::time_entry(&f.values(cx), &self.session),
            FormBody::Travel(f) => validate::trip(&f.values(cx), &self.session),
        }
    }
}

enum Saved {
    Done(i64, &'static str),
    Partial(i64, timechimp_api::Error),
}
