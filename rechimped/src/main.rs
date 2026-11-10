#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod domain;
mod forms;
mod fx;
mod net;
mod password;
mod seed;
mod session;
mod store;
#[cfg(test)]
mod test_support;
mod validate;
mod view;

use std::sync::Arc;

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, Rems, SharedString, TitlebarOptions, WindowBounds, WindowOptions,
    px, size,
};
use gpuikit::a11y::{FocusNext, FocusPrevious};
use gpuikit::input::INPUT_CONTEXT;
use gpuikit::theme::{ControlMetrics, GlobalTheme, Theme, parse_hex};

use app::{
    AddressDown, AddressUp, CalculateDistance, Cancel, CloseAddresses, CloseCalendar, Confirm, NextDay, NextMonth, NextWeek, PickAddress, PickRecent, PreviousDay, PreviousMonth,
    PreviousWeek, RecentDown, RecentUp, TimeChimpApp, ToLists, ToggleShowcase,
};

fn main() {
    // Start the tokio runtime up front so the first request doesn't pay for it.
    let _ = net::runtime();

    Application::with_platform(gpui_platform::current_platform(false)).with_assets(gpuikit::assets()).run(
        |cx: &mut App| {
            gpuikit::init(cx);
            cx.set_global(GlobalTheme(Arc::new(theme())));
            bind_keys(cx);

            let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions { title: Some(SharedString::from("Rechimped")), ..Default::default() }),
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

fn theme() -> Theme {
    let mut theme = Theme::catppuccin_latte();
    theme.name = "Rechimped".into();
    theme.accent_color = parse_hex("#f36f21");
    theme.input_border_focused_color = Some(parse_hex("#f36f21"));
    theme.input_bg_color = Some(parse_hex("#f6f7fa"));
    theme.input_cursor_color = Some(parse_hex("#f36f21"));
    theme.accent_bg_color = Some(parse_hex("#f36f21").opacity(0.14));
    theme.accent_bg_hover_color = Some(parse_hex("#f36f21").opacity(0.22));
    // The default button fill is the surface colour, which barely differs from the background.
    theme.button_bg_color = Some(parse_hex("#ffffff"));
    theme.button_bg_hover_color = Some(parse_hex("#fdeee6"));
    theme.button_bg_active_color = Some(parse_hex("#fbdccb"));
    theme.button_border_color = Some(parse_hex("#bcc0cc"));
    theme.controls.medium = ControlMetrics {
        height: Rems(1.75),
        padding_x: Rems(0.75),
        gap: Rems(0.375),
        radius: Rems(0.375),
        text_size: Rems(0.8125),
        line_height: Rems(1.125),
        ink: Rems(1.0),
    };
    theme.controls.large = ControlMetrics {
        height: Rems(2.0),
        padding_x: Rems(0.875),
        gap: Rems(0.5),
        radius: Rems(0.4375),
        text_size: Rems(0.875),
        line_height: Rems(1.25),
        ink: Rems(1.125),
    };
    theme
}

/// Registered after `gpuikit::init`, so at equal context depth these win over gpuikit's own
/// `Input` bindings.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(INPUT_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(INPUT_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(INPUT_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some("Modal")),
        KeyBinding::new("ctrl-enter", Confirm, Some("Modal")),
        KeyBinding::new("ctrl-d", CalculateDistance, Some("Modal")),
        KeyBinding::new("up", RecentUp, Some("RecentPicker > Input")),
        KeyBinding::new("down", RecentDown, Some("RecentPicker > Input")),
        KeyBinding::new("enter", PickRecent, Some("RecentPicker > Input")),
        // With nothing to do these pass the key on, so Enter still saves and Escape still cancels.
        KeyBinding::new("up", AddressUp, Some("AddressField > Input")),
        KeyBinding::new("down", AddressDown, Some("AddressField > Input")),
        KeyBinding::new("enter", PickAddress, Some("AddressField > Input")),
        KeyBinding::new("escape", CloseAddresses, Some("AddressField > Input")),
        // With the calendar closed these pass the key on (caret movement, Enter saves, Escape cancels).
        KeyBinding::new("left", PreviousDay, Some("DateField > Input")),
        KeyBinding::new("right", NextDay, Some("DateField > Input")),
        KeyBinding::new("up", PreviousWeek, Some("DateField > Input")),
        KeyBinding::new("down", NextWeek, Some("DateField > Input")),
        KeyBinding::new("pageup", PreviousMonth, Some("DateField > Input")),
        KeyBinding::new("pagedown", NextMonth, Some("DateField > Input")),
        KeyBinding::new("enter", CloseCalendar, Some("DateField > Input")),
        KeyBinding::new("escape", CloseCalendar, Some("DateField > Input")),
        KeyBinding::new("down", ToLists, Some("BillableTarget > Input")),
        KeyBinding::new("f12", ToggleShowcase, None),
    ]);
}
