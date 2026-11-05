//! Desktop GUI for TimeChimp, built with gpui + gpuikit: log in, browse your time entries per
//! day or week, register/edit/copy/delete time and mileage, and see a monthly report.
//!
//! The session is stored in `%APPDATA%\timechimp-tui\refresh_token`, so you only log in once.
//! `TIMECHIMP_REFRESH_TOKEN` overrides it.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod forms;
mod logic;
mod password;
mod view;

use std::sync::Arc;

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, Rems, SharedString, TitlebarOptions, WindowBounds, WindowOptions,
    px, size,
};
use gpuikit::a11y::{FocusNext, FocusPrevious};
use gpuikit::input::INPUT_CONTEXT;
use gpuikit::theme::{ControlMetrics, GlobalTheme, Theme, parse_hex};

use app::{CalculateDistance, Cancel, Confirm, CycleAddress, PickRecent, RecentDown, RecentUp, TimeChimpApp};

fn main() {
    // Start the tokio runtime up front so the first request doesn't pay for it.
    let _ = logic::runtime();

    Application::with_platform(gpui_platform::current_platform(false)).with_assets(gpuikit::assets()).run(
        |cx: &mut App| {
            gpuikit::init(cx);
            cx.set_global(GlobalTheme(Arc::new(theme())));
            bind_keys(cx);

            let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions { title: Some(SharedString::from("TimeChimp")), ..Default::default() }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| TimeChimpApp::new(window, cx)),
            )
            .expect("failed to open window");
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        },
    );
}

/// Catppuccin Latte (light) with TimeChimp's orange as the accent.
fn theme() -> Theme {
    let mut theme = Theme::catppuccin_latte();
    theme.name = "TimeChimp".into();
    theme.accent_color = parse_hex("#f36f21");
    theme.input_border_focused_color = Some(parse_hex("#f36f21"));
    // Text fields and selects a soft off-white, so they stand out from the dialog surface
    // without glaring.
    theme.input_bg_color = Some(parse_hex("#f6f7fa"));
    theme.input_cursor_color = Some(parse_hex("#f36f21"));
    theme.accent_bg_color = Some(parse_hex("#f36f21").opacity(0.14));
    theme.accent_bg_hover_color = Some(parse_hex("#f36f21").opacity(0.22));
    // Buttons: white on the grey-blue background (the default fill is the surface colour, which
    // barely differs from it), warming towards the accent on hover/press.
    theme.button_bg_color = Some(parse_hex("#ffffff"));
    theme.button_bg_hover_color = Some(parse_hex("#fdeee6"));
    theme.button_bg_active_color = Some(parse_hex("#fbdccb"));
    theme.button_border_color = Some(parse_hex("#bcc0cc"));
    // gpuikit's default control rungs (20px medium, 12px text) feel cramped in a desktop window;
    // every input, select, button, tab and checkbox sizes itself from these.
    theme.controls.medium = ControlMetrics {
        height: Rems(1.75),        // 28px
        padding_x: Rems(0.75),     // 12
        gap: Rems(0.375),          // 6
        radius: Rems(0.375),       // 6
        text_size: Rems(0.8125),   // 13
        line_height: Rems(1.125),  // 18
        ink: Rems(1.0),            // 16
    };
    theme.controls.large = ControlMetrics {
        height: Rems(2.0),         // 32
        padding_x: Rems(0.875),    // 14
        gap: Rems(0.5),            // 8
        radius: Rems(0.4375),      // 7
        text_size: Rems(0.875),    // 14
        line_height: Rems(1.25),   // 20
        ink: Rems(1.125),          // 18
    };
    theme
}

/// Registered after `gpuikit::init`, so at equal context depth these win over gpuikit's own
/// `Input` bindings: in our single-line forms Tab moves to the next field instead of typing a
/// tab character, Enter submits and Escape cancels the dialog instead of just blurring.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(INPUT_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(INPUT_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(INPUT_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some("Modal")),
        KeyBinding::new("ctrl-enter", Confirm, Some("Modal")),
        KeyBinding::new("ctrl-d", CalculateDistance, Some("Modal")),
        KeyBinding::new("f2", CycleAddress, Some("Modal")),
        // The time form's Recent filter: ↑/↓ move through the list, Enter picks.
        KeyBinding::new("up", RecentUp, Some("RecentPicker > Input")),
        KeyBinding::new("down", RecentDown, Some("RecentPicker > Input")),
        KeyBinding::new("enter", PickRecent, Some("RecentPicker > Input")),
    ]);
}
