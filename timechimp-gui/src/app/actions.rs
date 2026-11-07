//! The actions bound to keys (`main.rs::bind_keys`) and handled by the views.

use gpui::actions;

actions!(
    timechimp,
    [
        /// Enter in a form or the login screen.
        Confirm,
        /// Escape in a form.
        Cancel,
        /// Ctrl+D in the mileage form.
        CalculateDistance,
        /// ↑ in an address field of the trip form: previous suggestion (opens the list).
        AddressUp,
        /// ↓ in an address field of the trip form: next suggestion (opens the list).
        AddressDown,
        /// Enter in an address field: take the highlighted suggestion.
        PickAddress,
        /// Escape in an address field: close the suggestions.
        CloseAddresses,
        /// ← in a form's Date field with the calendar open: the day before.
        PreviousDay,
        /// → in the Date field with the calendar open: the day after.
        NextDay,
        /// ↑ in the Date field with the calendar open: a week earlier.
        PreviousWeek,
        /// ↓ in the Date field: opens the calendar, then a week later.
        NextWeek,
        /// PageUp in the Date field with the calendar open: a month earlier.
        PreviousMonth,
        /// PageDown in the Date field with the calendar open: a month later.
        NextMonth,
        /// Enter or Escape in the Date field: close the calendar.
        CloseCalendar,
        /// ↑ in the time form's Recent filter.
        RecentUp,
        /// ↓ in the time form's Recent filter.
        RecentDown,
        /// Enter in the time form's Recent filter: fill in the highlighted item.
        PickRecent,
        /// F12: showcase mode (aurora behind the app, frame-time HUD).
        ToggleShowcase,
    ]
);
