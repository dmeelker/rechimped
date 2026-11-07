use gpui::{Context, Focusable, Window};
use gpuikit::input::InputStateEvent;
use timechimp::Mileage;

use crate::forms::{self, AddressField, AddressMenu, Form, FormBody, MileageForm, text};
use crate::domain::MILEAGE_TYPES;
use crate::seed;
use crate::store::config;

use super::{Overlay, TimeChimpApp};

impl TimeChimpApp {
    pub fn open_trip_preset(&mut self, preset: config::TripPreset, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if self.session.me().is_none() || self.overlay_open() {
            return;
        }
        self.open_mileage_form(None, Some(&preset), window, cx);
    }

    pub(super) fn open_mileage_form(
        &mut self,
        editing: Option<Box<Mileage>>,
        preset: Option<&config::TripPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.overlay_open() {
            return;
        }
        let seed = seed::trip_form(&self.session, editing.as_deref(), preset);
        let types = MILEAGE_TYPES.iter().map(|(id, name)| (*id, name.to_string())).collect();
        let form = MileageForm {
            editing,
            date: forms::DateField::new(cx, seed.date),
            project: forms::choice(cx, "form-project", "Project", seed.projects, seed.project_id),
            from: forms::text_input(cx, seed.from),
            to: forms::text_input(cx, seed.to),
            distance: forms::text_input(cx, seed.distance),
            return_trip: forms::toggle(cx, "form-return", "Return trip", seed.return_trip),
            mileage_type: forms::choice(cx, "form-type", "Type", types, seed.mileage_type),
            billable: forms::toggle(cx, "form-billable", "Billable", seed.billable),
            notes: forms::text_input(cx, seed.notes),
            addresses: seed.addresses,
            address_menu: None,
        };
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
        let id = self.new_form_id();
        self.overlay =
            Overlay::Form(Box::new(Form { id, body: FormBody::Mileage(form), error: None, _subscriptions: subscriptions }));
        cx.notify();
    }

    pub fn open_addresses(&mut self, field: AddressField, filtered: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.mileage_form_mut() {
            let highlighted = (!filtered).then_some(0);
            form.address_menu = Some(AddressMenu { field, filtered, highlighted });
            cx.notify();
        }
    }

    pub fn close_addresses(&mut self, field: AddressField, cx: &mut Context<Self>) {
        if let Some(form) = self.mileage_form_mut()
            && form.address_menu.as_ref().is_some_and(|m| m.field == field)
        {
            form.address_menu = None;
            cx.notify();
        }
    }

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

    /// With nothing highlighted, propagate so Enter saves the form.
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

    pub fn pick_address(&mut self, field: AddressField, address: String, cx: &mut Context<Self>) {
        let Some(form) = self.mileage_form_mut() else { return };
        form.address_menu = None;
        // Silent, or the TextChanged subscription would reopen the list filtered by the address.
        form.address_input(field).update(cx, |input, cx| input.set_content_silent(address, cx));
        cx.notify();
    }

    /// With no suggestions open, propagate so Escape closes the form.
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
        let (Some(form), Some(client)) = (self.mileage_form(), self.session.client().cloned()) else { return };
        let (form_id, from, to) = (self.form().map(|f| f.id), text(&form.from, cx), text(&form.to, cx));
        if from.is_empty() || to.is_empty() {
            return self.form_error("Fill in both addresses first", cx);
        }
        let future = async move {
            let lookup = |query: String| {
                let client = client.clone();
                async move {
                    let place = client.place_predictions(&query).await?.into_iter().next();
                    place.ok_or_else(|| timechimp::Error::NotFound(format!("Address not found: {query}")))
                }
            };
            let (from_place, to_place) = tokio::try_join!(lookup(from), lookup(to))?;
            let route = client.route_distance(&from_place.place_id, &to_place.place_id).await?;
            timechimp::Result::Ok((from_place.description, to_place.description, route))
        };
        self.run("Calculating distance…", future, window, cx, move |this, result, window, cx| match result {
            Ok((from, to, route)) => {
                // Only into the form it was asked for: it may have been closed or replaced meanwhile.
                if let Some(form) = this.form_mut().filter(|f| Some(f.id) == form_id) {
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
            Err(e) => this.form_api_error(form_id, e, window, cx),
        });
    }
}
