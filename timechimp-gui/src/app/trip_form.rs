//! The trip form: opening it (new, edit, preset), the address suggestions, and the route distance.

use gpui::{Context, Focusable, Window};
use gpuikit::input::InputStateEvent;
use timechimp::Mileage;

use crate::forms::{self, AddressField, AddressMenu, Form, FormBody, MileageForm, text};
use crate::domain::MILEAGE_TYPES;
use crate::store::config;

use super::TimeChimpApp;

impl TimeChimpApp {
    /// A trip preset in the "+ Add trip" dropdown: the trip form for the selected day, filled in
    /// from preset `index`.
    pub fn open_trip_preset(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.me.is_none() || self.form.is_some() {
            return;
        }
        self.menu = None;
        if let Some(preset) = self.session.trip_presets.get(index).cloned() {
            self.open_mileage_form(None, Some(&preset), window, cx);
        }
    }

    /// The trip form: for a new trip filled in from `preset`, or empty (project, type and billable
    /// from the last trip); or filled in with `editing`.
    pub(super) fn open_mileage_form(
        &mut self,
        editing: Option<Box<Mileage>>,
        preset: Option<&config::TripPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let template = editing.as_deref().or(self.session.mileage.last());
        let saved = self.session.saved_addresses();
        let types = MILEAGE_TYPES.iter().map(|(id, name)| (*id, name.to_string())).collect();
        let projects =
            self.session.project_options(editing.as_deref().map(|m| (m.project_id, &*m.project_name, &*m.customer_name)));
        let (mut project, mut mileage_type, mut billable, mut return_trip) = (
            template.map(|m| m.project_id),
            template.map(|m| m.mileage_type),
            template.is_some_and(|m| m.billable),
            false,
        );
        if let Some(p) = preset {
            project = self.session.project_named(&p.project).map(|p| p.id).or(project);
            mileage_type = p.trip_type();
            billable = p.billable;
            return_trip = p.return_trip;
        }
        let (date, from, to, distance, notes) = match (&editing, preset) {
            (None, Some(p)) => (
                self.session.day,
                p.from.clone(),
                p.to.clone(),
                p.distance.map(|d| format!("{d}")).unwrap_or_default(),
                p.notes.clone().unwrap_or_default(),
            ),
            (Some(m), _) => (
                m.date,
                m.from_address.clone().unwrap_or_default(),
                m.to_address.clone().unwrap_or_default(),
                format!("{}", m.distance),
                m.notes.clone().unwrap_or_default(),
            ),
            (None, None) => (
                self.session.day,
                saved.first().cloned().unwrap_or_default(),
                saved.get(1).cloned().unwrap_or_default(),
                String::new(),
                String::new(),
            ),
        };
        let form = MileageForm {
            editing,
            date: forms::DateField::new(cx, date),
            project: forms::choice(cx, "form-project", "Project", projects, project),
            from: forms::text_input(cx, from),
            to: forms::text_input(cx, to),
            distance: forms::text_input(cx, distance),
            return_trip: forms::toggle(cx, "form-return", "Return trip", return_trip),
            mileage_type: forms::choice(cx, "form-type", "Type", types, mileage_type),
            billable: forms::toggle(cx, "form-billable", "Billable", billable),
            notes: forms::text_input(cx, notes),
            addresses: saved,
            address_menu: None,
        };
        // Typing in From/To filters the suggestions; leaving the field closes them.
        let mut subscriptions = Vec::new();
        for field in [AddressField::From, AddressField::To] {
            let input = form.address_input(field);
            subscriptions.push(cx.subscribe_in(input, window, move |this, _, event: &InputStateEvent, _, cx| {
                if matches!(event, InputStateEvent::TextChanged) {
                    this.open_addresses(field, true, cx);
                }
            }));
            let handle = input.read(cx).focus_handle(cx);
            subscriptions.push(cx.on_focus_out(&handle, window, move |this, _, _, cx| this.close_addresses(field, cx)));
        }
        window.focus(&form.from.read(cx).focus_handle(cx), cx);
        self.form = Some(Form { body: FormBody::Mileage(form), error: None, _subscriptions: subscriptions });
        cx.notify();
    }

    /// Opens the suggestions under `field`: filtered by its text after typing (nothing
    /// highlighted, so Enter keeps what was typed), or all of them from ↓ or ▾.
    pub fn open_addresses(&mut self, field: AddressField, filtered: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.mileage_form_mut() {
            let highlighted = (!filtered).then_some(0);
            form.address_menu = Some(AddressMenu { field, filtered, highlighted });
            cx.notify();
        }
    }

    /// Closes the suggestions if they are open under `field`.
    pub fn close_addresses(&mut self, field: AddressField, cx: &mut Context<Self>) {
        if let Some(form) = self.mileage_form_mut()
            && form.address_menu.as_ref().is_some_and(|m| m.field == field)
        {
            form.address_menu = None;
            cx.notify();
        }
    }

    /// The ▾ of an address field: focuses it and shows all suggestions, or closes them.
    pub fn toggle_addresses(&mut self, field: AddressField, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form() else { return };
        let open = form.address_menu.as_ref().is_some_and(|m| m.field == field);
        window.focus(&form.address_input(field).read(cx).focus_handle(cx), cx);
        if open {
            self.close_addresses(field, cx);
        } else {
            self.open_addresses(field, false, cx);
        }
    }

    /// ↑/↓ in an address field: moves through the suggestions, opening the full list first.
    pub fn move_address(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form() else { return cx.propagate() };
        let Some(field) = form.focused_address(window, cx) else { return cx.propagate() };
        let count = form.address_matches(cx).len();
        let Some(menu) = self.mileage_form_mut().and_then(|f| f.address_menu.as_mut()).filter(|m| m.field == field) else {
            return self.open_addresses(field, false, cx);
        };
        let last = count.saturating_sub(1);
        menu.highlighted = Some(match menu.highlighted {
            None => 0,
            Some(i) => i.saturating_add_signed(delta).min(last),
        });
        cx.notify();
    }

    /// Enter in an address field: takes the highlighted suggestion; with none, Enter saves the form.
    pub fn pick_highlighted_address(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form() else { return cx.propagate() };
        let picked = form
            .address_menu
            .as_ref()
            .and_then(|m| Some((m.field, form.address_matches(cx).get(m.highlighted?)?.to_string())));
        match picked {
            Some((field, address)) => self.pick_address(field, address, cx),
            None => {
                if let Some(form) = self.mileage_form_mut() {
                    form.address_menu = None;
                }
                cx.propagate();
            }
        }
    }

    /// Puts `address` in `field` and closes the suggestions.
    pub fn pick_address(&mut self, field: AddressField, address: String, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form_mut() else { return };
        form.address_menu = None;
        // Silent, or the TextChanged subscription would reopen the list filtered by the address.
        form.address_input(field).update(cx, |input, cx| input.set_content_silent(address, cx));
        cx.notify();
    }

    /// Escape in an address field: closes the suggestions; with none open, Escape closes the form.
    pub fn dismiss_addresses(&mut self, cx: &mut Context<Self>) {
        match self.mileage_form_mut() {
            Some(form) if form.address_menu.is_some() => {
                form.address_menu = None;
                cx.notify();
            }
            _ => cx.propagate(),
        }
    }

    pub fn calculate_distance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(form), Some(client)) = (self.mileage_form(), self.session.client.clone()) else { return };
        let (from, to) = (text(&form.from, cx), text(&form.to, cx));
        if from.is_empty() || to.is_empty() {
            return self.form_error("Fill in both addresses first", cx);
        }
        // Ok(Err(message)): an address the lookup doesn't know.
        let future = async move {
            let lookup = |query: String| {
                let client = client.clone();
                async move {
                    let place = client.place_predictions(&query).await?.into_iter().next();
                    timechimp::Result::Ok(place.ok_or(format!("Address not found: {query}")))
                }
            };
            let (from_place, to_place) = tokio::try_join!(lookup(from), lookup(to))?;
            let (from_place, to_place) = match (from_place, to_place) {
                (Ok(from), Ok(to)) => (from, to),
                (Err(message), _) | (_, Err(message)) => return Ok(Err(message)),
            };
            let route = client.route_distance(&from_place.place_id, &to_place.place_id).await?;
            timechimp::Result::Ok(Ok((from_place.description, to_place.description, route)))
        };
        self.run("Calculating distance…", future, window, cx, |this, result, window, cx| match result {
            Ok(Ok((from, to, route))) => {
                if let Some(form) = &mut this.form {
                    form.error = None;
                    if let Some(f) = form.trip() {
                        forms::set_text(&f.from, from, cx);
                        forms::set_text(&f.to, to, cx);
                        forms::set_text(&f.distance, format!("{:.0}", route.kilometres()), cx);
                    }
                }
                this.set_info(
                    format!("Route: {:.1} km, {} min by car", route.kilometres(), route.seconds / 60),
                    window,
                    cx,
                );
            }
            Ok(Err(message)) => this.form_error(message, cx),
            Err(e) => this.form_api_error(e, window, cx),
        });
    }
}
