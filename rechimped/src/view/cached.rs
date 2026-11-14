//! A gpui animation asks for its next frame by notifying the view it is drawn in, and a notified
//! view redraws with all its ancestors. Drawn straight in `TimeChimpApp`, the sweep bar would lay
//! out the whole window every frame for as long as it runs (each load, and 900 ms after one). So
//! it is a view of its own, and everything under it (toolbar, tab, footer) is a cached view, reused
//! while the app isn't notified: each sweep frame only redraws the header and the bar.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, Render, StyleRefinement, Styled, Subscription, WeakEntity, Window, div,
};
use gpuikit::theme::{ActiveTheme, Themeable};

use crate::app::TimeChimpApp;

pub struct CachedViews {
    body: Entity<TabBody>,
    sweep: Entity<SweepBar>,
}

impl CachedViews {
    pub fn new(cx: &mut Context<TimeChimpApp>) -> Self {
        let app = cx.entity();
        Self {
            body: cx.new(|cx| TabBody {
                app: app.downgrade(),
                // Not a sweep frame: those notify the sweep bar, which only marks this view's
                // ancestors for redrawing, not the app's observers.
                _app_changed: cx.observe(&app, |_, _, cx| cx.notify()),
            }),
            sweep: cx.new(|_| SweepBar { app: app.downgrade() }),
        }
    }

    pub fn body(&self) -> impl IntoElement + use<> {
        self.body.clone().cached(StyleRefinement::default().flex_1().min_h_0().flex().flex_col())
    }

    pub fn sweep(&self) -> Entity<SweepBar> {
        self.sweep.clone()
    }
}

pub struct TabBody {
    app: WeakEntity<TimeChimpApp>,
    _app_changed: Subscription,
}

impl Render for TabBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(app) = self.app.upgrade() else { return div().into_any_element() };
        app.update(cx, |app, cx| app.render_tab_body(window, cx).into_any_element())
    }
}

/// Not cached: it reads the app afresh whenever the app redraws.
pub struct SweepBar {
    app: WeakEntity<TimeChimpApp>,
}

impl Render for SweepBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (sweep, busy) =
            self.app.upgrade().map_or((0, false), |app| (app.read(cx).fx.sweep, app.read(cx).busy().is_some()));
        crate::fx::sweep_bar(sweep, busy, cx.theme().accent())
    }
}
