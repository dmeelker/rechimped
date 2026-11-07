//! The calendar under the Date field of both forms.

use gpui::{AppContext, Context, Focusable, Window};
use gpuikit::date::Weekday;
use gpuikit::elements::calendar::{Calendar, CalendarEvent};
use gpuikit::input::InputStateEvent;

use crate::forms;
use crate::domain;

use super::TimeChimpApp;

impl TimeChimpApp {
    /// The calendar icon of the Date field: focuses the field and opens the calendar, or closes it.
    pub fn toggle_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let open = form.date().picker.is_some();
        window.focus(&form.date().input.read(cx).focus_handle(cx), cx);
        if open {
            self.close_calendar(cx);
        } else {
            self.open_calendar(window, cx);
        }
    }

    /// Opens a calendar under the Date field on the date typed in (or the selected day).
    ///
    /// The Date field keeps the focus and drives the calendar (`move_calendar`): gpuikit draws a
    /// focused calendar's focus ring as a filled shadow behind its transparent grid, which turns
    /// the whole grid accent-coloured, so a click on it hands the focus straight back.
    pub fn open_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        if form.date().picker.is_some() {
            return;
        }
        let date = form.date().parsed(cx).unwrap_or(self.session.day());
        let input = form.date().input.clone();
        let calendar = cx.new(|cx| {
            Calendar::new("form-calendar", forms::to_calendar_date(date), cx)
                .first_day_of_week(Weekday::Monday)
                .today(forms::to_calendar_date(domain::time::today()))
                .selected(Some(forms::to_calendar_date(date)))
        });
        let calendar_focus = calendar.read(cx).focus_handle(cx);
        let input_focus = input.read(cx).focus_handle(cx);
        let subscriptions = vec![
            // A clicked day fills in the field and closes the calendar.
            cx.subscribe_in(&calendar, window, |this, _, event: &CalendarEvent, window, cx| {
                if let CalendarEvent::Selected(day) = event
                    && let (Some(form), Some(day)) = (&this.form, forms::from_calendar_date(*day))
                {
                    let input = &form.date().input;
                    input.update(cx, |input, cx| input.set_content_silent(day.to_string(), cx));
                    // The click focused the calendar, which is about to go.
                    window.focus(&input.read(cx).focus_handle(cx), cx);
                    this.close_calendar(cx);
                }
            }),
            // A typed date shows in the calendar.
            cx.subscribe_in(&input, window, |this, _, event: &InputStateEvent, _, cx| {
                if let (InputStateEvent::TextChanged, Some(form)) = (event, &this.form)
                    && let (Some(picker), Some(date)) = (&form.date().picker, form.date().parsed(cx))
                {
                    picker.calendar.update(cx, |c, cx| c.set_selected(Some(forms::to_calendar_date(date)), cx));
                }
            }),
            cx.on_focus_in(&calendar_focus, window, {
                let input_focus = input_focus.clone();
                move |_, window, cx| window.focus(&input_focus, cx)
            }),
            // Leaving the field closes it, but not a click on the calendar: by the time this runs,
            // the focus may be on the calendar or already handed back to the field.
            cx.on_focus_out(&input_focus.clone(), window, move |this, _, window, cx| {
                if !calendar_focus.is_focused(window) && !input_focus.is_focused(window) {
                    this.close_calendar(cx);
                }
            }),
        ];
        if let Some(form) = &mut self.form {
            form.date_mut().picker = Some(forms::DatePicker { calendar, _subscriptions: subscriptions });
        }
        cx.notify();
    }

    /// Closes the calendar, if open.
    pub fn close_calendar(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form
            && form.date_mut().picker.take().is_some()
        {
            cx.notify();
        }
    }

    /// ↓ in the Date field: opens a closed calendar, or moves a week down in an open one.
    pub fn calendar_down(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.form {
            Some(form) if form.date().picker.is_none() => self.open_calendar(window, cx),
            _ => self.move_calendar(7, 0, cx),
        }
    }

    /// The arrows and PageUp/PageDown in the Date field while the calendar is open: move the date
    /// by `days` and `months`, in the field and the calendar. With it closed the key does what it
    /// normally does in a text field.
    pub fn move_calendar(&mut self, days: i64, months: i32, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return cx.propagate() };
        let field = form.date();
        let Some(picker) = &field.picker else { return cx.propagate() };
        let current = field.parsed(cx).unwrap_or(self.session.day());
        let Some(date) = domain::time::shift_date(current, days, months) else { return };
        field.input.update(cx, |input, cx| input.set_content_silent(date.to_string(), cx));
        picker.calendar.update(cx, |c, cx| c.set_selected(Some(forms::to_calendar_date(date)), cx));
        cx.notify();
    }

    /// Enter or Escape in the Date field: closes an open calendar (keeping the date); with none
    /// open the key goes on, so Enter saves and Escape cancels the form.
    pub fn dismiss_calendar(&mut self, cx: &mut Context<Self>) {
        match &self.form {
            Some(form) if form.date().picker.is_some() => self.close_calendar(cx),
            _ => cx.propagate(),
        }
    }
}
