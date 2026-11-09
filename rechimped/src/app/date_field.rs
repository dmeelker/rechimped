use gpui::{AppContext, Context, Focusable, Window};
use gpuikit::date::Weekday;
use gpuikit::elements::calendar::{Calendar, CalendarEvent};
use gpuikit::input::InputStateEvent;

use crate::forms;
use crate::domain;

use super::TimeChimpApp;

impl TimeChimpApp {
    pub fn toggle_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form() else { return };
        let open = form.date().picker.is_some();
        window.focus(&form.date().input.read(cx).focus_handle(cx), cx);
        if open {
            self.close_calendar(cx);
        } else {
            self.open_calendar(window, cx);
        }
    }

    /// gpuikit draws a focused calendar's focus ring as a filled shadow behind its transparent grid,
    /// so the field keeps the focus and a click on the calendar hands it straight back.
    pub fn open_calendar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form() else { return };
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
            cx.subscribe_in(&calendar, window, |this, _, event: &CalendarEvent, window, cx| {
                if let CalendarEvent::Selected(day) = event
                    && let (Some(form), Some(day)) = (this.form(), forms::from_calendar_date(*day))
                {
                    let input = &form.date().input;
                    input.update(cx, |input, cx| input.set_content_silent(day.to_string(), cx));
                    // The click focused the calendar, which is about to go.
                    window.focus(&input.read(cx).focus_handle(cx), cx);
                    this.close_calendar(cx);
                }
            }),
            cx.subscribe_in(&input, window, |this, _, event: &InputStateEvent, _, cx| {
                if let (InputStateEvent::TextChanged, Some(form)) = (event, this.form())
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
        if let Some(form) = self.form_mut() {
            form.date_mut().picker = Some(forms::DatePicker { calendar, _subscriptions: subscriptions });
        }
        cx.notify();
    }

    pub fn close_calendar(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.form_mut()
            && form.date_mut().picker.take().is_some()
        {
            cx.notify();
        }
    }

    pub fn calendar_down(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.form() {
            Some(form) if form.date().picker.is_none() => self.open_calendar(window, cx),
            _ => self.move_calendar(7, 0, cx),
        }
    }

    pub fn move_calendar(&mut self, days: i64, months: i32, cx: &mut Context<Self>) {
        let Some(form) = self.form() else { return cx.propagate() };
        let field = form.date();
        let Some(picker) = &field.picker else { return cx.propagate() };
        let current = field.parsed(cx).unwrap_or(self.session.day());
        let Some(date) = domain::time::shift_date(current, days, months) else { return };
        field.input.update(cx, |input, cx| input.set_content_silent(date.to_string(), cx));
        picker.calendar.update(cx, |c, cx| c.set_selected(Some(forms::to_calendar_date(date)), cx));
        cx.notify();
    }

    pub fn dismiss_calendar(&mut self, cx: &mut Context<Self>) {
        match self.form() {
            Some(form) if form.date().picker.is_some() => self.close_calendar(cx),
            _ => cx.propagate(),
        }
    }
}
