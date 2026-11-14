//! The scrollbar's state and maths, shared by every scrolling list (drawn by
//! `view::widgets::with_scrollbar`).

use std::cell::Cell;
use std::rc::Rc;

use gpui::{ListState, ScrollHandle, point, px};

/// What a vertical scrollbar needs of a scrolled area, in window pixels: the viewport's top and
/// height, how far it can scroll and how far it has (both ≥ 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollMetrics {
    pub top: f32,
    pub viewport: f32,
    pub max_offset: f32,
    pub offset: f32,
}

pub const SCROLLBAR_MIN_THUMB: f32 = 24.;

/// Where a click on the bar landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BarHit {
    /// On the thumb, this far below its top.
    Thumb(f32),
    /// On the track; the offset a page up or down from here.
    Page(f32),
}

impl ScrollMetrics {
    /// The thumb's top (from the viewport's) and height; None when everything fits.
    pub fn thumb(&self) -> Option<(f32, f32)> {
        if self.viewport <= 0. || self.max_offset < 0.5 {
            return None;
        }
        let height =
            (self.viewport * self.viewport / (self.viewport + self.max_offset)).max(SCROLLBAR_MIN_THUMB).min(self.viewport);
        let fraction = (self.offset / self.max_offset).clamp(0., 1.);
        Some(((self.viewport - height) * fraction, height))
    }

    /// The offset that puts the thumb's top `thumb_top` below the viewport's.
    pub fn offset_for_thumb(&self, thumb_top: f32) -> f32 {
        let Some((_, height)) = self.thumb() else { return 0. };
        let travel = self.viewport - height;
        if travel <= 0. {
            return 0.;
        }
        thumb_top.clamp(0., travel) / travel * self.max_offset
    }

    /// What a press at window y `y` on the bar means.
    pub fn hit(&self, y: f32) -> Option<BarHit> {
        let (top, height) = self.thumb()?;
        let y = y - self.top;
        Some(if y < top {
            BarHit::Page((self.offset - self.viewport).max(0.))
        } else if y >= top + height {
            BarHit::Page((self.offset + self.viewport).min(self.max_offset))
        } else {
            BarHit::Thumb(y - top)
        })
    }
}

/// A scrolled area as the scrollbar sees it, read from last frame's layout. gpui reports both
/// kinds of offset negative (content moved up); `ScrollMetrics` has them positive.
pub trait Scrollable: Clone + 'static {
    fn metrics(&self) -> ScrollMetrics;
    /// Scrolls to `offset` pixels from the top.
    fn scroll_to(&self, offset: f32);
    fn drag_started(&self) {}
    fn drag_ended(&self) {}
}

/// A virtualized `list`.
impl Scrollable for ListState {
    fn metrics(&self) -> ScrollMetrics {
        let bounds = self.viewport_bounds();
        ScrollMetrics {
            top: bounds.origin.y.as_f32(),
            viewport: bounds.size.height.as_f32(),
            max_offset: self.max_offset_for_scrollbar().y.as_f32(),
            offset: -self.scroll_px_offset_for_scrollbar().y.as_f32(),
        }
    }

    fn scroll_to(&self, offset: f32) {
        self.set_offset_from_scrollbar(point(px(0.), px(-offset)));
    }

    fn drag_started(&self) {
        self.scrollbar_drag_started();
    }

    fn drag_ended(&self) {
        self.scrollbar_drag_ended();
    }
}

/// A div with `overflow_y_scroll` and `track_scroll` of this handle.
impl Scrollable for ScrollHandle {
    fn metrics(&self) -> ScrollMetrics {
        metrics_of(self.bounds(), self.max_offset(), self.offset())
    }

    /// Clamped by the div when it is next laid out.
    fn scroll_to(&self, offset: f32) {
        self.set_offset(point(self.offset().x, px(-offset)));
    }
}

fn metrics_of(
    bounds: gpui::Bounds<gpui::Pixels>,
    max: gpui::Point<gpui::Pixels>,
    offset: gpui::Point<gpui::Pixels>,
) -> ScrollMetrics {
    ScrollMetrics {
        top: bounds.origin.y.as_f32(),
        viewport: bounds.size.height.as_f32(),
        max_offset: max.y.as_f32().max(0.),
        offset: (-offset.y.as_f32()).max(0.),
    }
}

/// A scrollbar's own state: while its thumb is dragged, where it was grabbed (below its top).
/// Shared with the window-wide drag listeners, hence the `Rc`.
#[derive(Clone, Default)]
pub struct Scrollbar(Rc<Cell<Option<f32>>>);

impl Scrollbar {
    pub fn grab(&self) -> Option<f32> {
        self.0.get()
    }

    pub fn start_drag(&self, grab: f32, scroll: &impl Scrollable) {
        self.0.set(Some(grab));
        scroll.drag_started();
    }

    pub fn end_drag(&self, scroll: &impl Scrollable) {
        if self.0.take().is_some() {
            scroll.drag_ended();
        }
    }
}

/// A scrolling div's handle together with its scrollbar.
#[derive(Clone, Default)]
pub struct ScrollArea {
    pub handle: ScrollHandle,
    pub bar: Scrollbar,
}

impl ScrollArea {
    pub fn scroll_to_item(&self, index: usize) {
        self.handle.scroll_to_item(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, size};

    fn m(viewport: f32, max_offset: f32, offset: f32) -> ScrollMetrics {
        ScrollMetrics { top: 100., viewport, max_offset, offset }
    }

    #[test]
    fn no_thumb_when_it_fits() {
        assert_eq!(m(400., 0., 0.).thumb(), None);
        assert_eq!(m(0., 300., 0.).thumb(), None);
    }

    #[test]
    fn thumb_follows_the_offset() {
        assert_eq!(m(400., 400., 0.).thumb(), Some((0., 200.)));
        assert_eq!(m(400., 400., 200.).thumb(), Some((100., 200.)));
        assert_eq!(m(400., 400., 400.).thumb(), Some((200., 200.)));
        // Past the end (before the list clamps) the thumb stays at the bottom.
        assert_eq!(m(400., 400., 500.).thumb(), Some((200., 200.)));
    }

    #[test]
    fn thumb_has_a_minimum_height() {
        assert_eq!(m(400., 100_000., 100_000.).thumb(), Some((400. - SCROLLBAR_MIN_THUMB, SCROLLBAR_MIN_THUMB)));
    }

    #[test]
    fn dragging_maps_back_to_the_offset() {
        let metrics = m(400., 400., 0.);
        assert_eq!(metrics.offset_for_thumb(100.), 200.);
        assert_eq!(metrics.offset_for_thumb(-50.), 0.);
        assert_eq!(metrics.offset_for_thumb(1000.), 400.);
    }

    #[test]
    fn presses_hit_the_thumb_or_page() {
        let metrics = m(400., 1000., 500.);
        let (top, height) = metrics.thumb().unwrap();
        assert_eq!(metrics.hit(100. + top + 5.), Some(BarHit::Thumb(5.)));
        assert_eq!(metrics.hit(100. + top - 1.), Some(BarHit::Page(100.)));
        assert_eq!(metrics.hit(100. + top + height + 1.), Some(BarHit::Page(900.)));
        assert_eq!(m(400., 0., 0.).hit(150.), None);
    }

    #[test]
    fn handle_offsets_turn_positive() {
        let bounds = Bounds::new(point(px(10.), px(50.)), size(px(300.), px(200.)));
        let metrics = metrics_of(bounds, point(px(0.), px(600.)), point(px(0.), px(-150.)));
        assert_eq!(metrics, ScrollMetrics { top: 50., viewport: 200., max_offset: 600., offset: 150. });
        // Before a scroll is clamped to a shrunk max, it never reads as negative.
        assert_eq!(metrics_of(bounds, point(px(0.), px(0.)), point(px(0.), px(20.))).offset, 0.);
    }

    #[test]
    fn a_handle_never_laid_out_has_no_thumb() {
        assert_eq!(ScrollHandle::new().metrics().thumb(), None);
    }
}
