//! A minimal masked text field. gpuikit's `InputState` has no password mode, so this keeps the
//! secret in a plain `String` and only ever renders bullets.

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, Render, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::theme::{ActiveTheme, ControlSize, Themeable};

pub enum PasswordEvent {
    Submit,
}

pub struct PasswordField {
    value: String,
    focus: FocusHandle,
}

impl EventEmitter<PasswordEvent> for PasswordField {}

impl PasswordField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { value: String::new(), focus: cx.focus_handle().tab_stop(true) }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let m = ks.modifiers;
        match ks.key.as_str() {
            "enter" => cx.emit(PasswordEvent::Submit),
            "backspace" if m.control => self.value.clear(),
            "backspace" => {
                self.value.pop();
            }
            "v" if m.control || m.platform => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.value.push_str(text.trim_end_matches(['\r', '\n']));
                }
            }
            _ if (m.control || m.platform) && !event.prefer_character_input => return,
            _ => match &ks.key_char {
                Some(c) if !c.chars().any(char::is_control) => self.value.push_str(c),
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for PasswordField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PasswordField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.focus.is_focused(window);
        let focus = self.focus.clone();
        // Same box as gpuikit's text fields.
        let m = theme.control(ControlSize::Medium);
        div()
            .id("password")
            .track_focus(&self.focus)
            .moves_focus_on_tab()
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| window.focus(&focus, cx))
            .w_full()
            .h(m.height)
            .px(m.padding_x)
            .flex()
            .items_center()
            .rounded(m.radius)
            .text_size(m.text_size)
            .border_1()
            .bg(theme.input_bg())
            .border_color(if focused { theme.input_border_focused() } else { theme.input_border() })
            .text_color(theme.input_text())
            .cursor_text()
            .when(self.value.is_empty() && !focused, |d| {
                d.child(div().text_color(theme.input_placeholder()).child("Password"))
            })
            .child("•".repeat(self.value.chars().count()))
            .when(focused, |d| d.child(div().w(px(1.5)).h(m.line_height).ml(px(1.)).bg(theme.input_cursor())))
    }
}
