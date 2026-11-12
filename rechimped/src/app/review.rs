use std::cell::Cell;

use gpui::{Context, ListAlignment, ListOffset, ListState, Window, px};

use crate::domain::time::last_of_month;
use crate::forms::Origin;
use crate::net;

use super::{Tab, TimeChimpApp};

impl TimeChimpApp {
    pub(super) fn load_review(&mut self, select_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.session.client().cloned(), self.session.me()) else { return };
        let user_id = me.id;
        let (generation, month) = self.session.begin_review_load();
        self.run(
            "Loading review…",
            async move { net::load_time_rows(&client, user_id, month, last_of_month(month)).await },
            window,
            cx,
            move |this, result, window, cx| match result {
                Ok(rows) => {
                    if this.session.apply_review(generation, month, rows, select_id) {
                        this.error = None;
                        this.scroll_to_review_selection();
                    }
                }
                Err(e) => this.api_error(e, window, cx),
            },
        );
    }

    /// Edits or copies the selected row. A report row lacks what the form needs (tag ids, the
    /// project task, the pause), so the full entry is fetched first.
    pub(super) fn open_review_entry(&mut self, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(id)) = (self.session.client().cloned(), self.session.selected_review_id()) else { return };
        if self.overlay_open() {
            return;
        }
        self.run("Loading…", async move { client.time_entry(id).await }, window, cx, move |this, result, window, cx| {
            match result {
                Ok(entry) if this.tab == Tab::Review => {
                    let origin = Origin::Review { date: (!copy).then_some(entry.date) };
                    this.open_time_form_on(Some(&entry), copy, entry.date, origin, window, cx);
                }
                Ok(_) => {}
                Err(e) => this.api_error(e, window, cx),
            }
        });
    }

    /// Brings the selected row into view on the next frame the list is drawn.
    pub(super) fn scroll_to_review_selection(&self) {
        self.review_list.reveal.set(true);
    }
}

/// The Review tab's virtualized list: only the lines in view are laid out each frame.
pub struct ReviewList {
    pub state: ListState,
    /// The `Session::review_revision` the list was last reset for.
    revision: Cell<Option<(u64, u64)>>,
    /// The selected row is to be scrolled into view.
    reveal: Cell<bool>,
}

impl Default for ReviewList {
    fn default() -> Self {
        // Every line is measured once after a reset, so revealing a row far off is exact.
        let state = ListState::new(0, ListAlignment::Top, px(200.)).measure_all();
        Self { state, revision: Cell::new(None), reveal: Cell::new(false) }
    }
}

impl ReviewList {
    pub fn scroll_to_top(&self) {
        self.state.scroll_to(ListOffset::default());
    }

    /// Called while drawing, before the list: resets it when its lines changed, keeping the
    /// first line in view, then reveals `selected` (a line index) if asked to. The lines are only
    /// measured while the list is laid out, so after a reset the reveal waits for the next frame.
    pub fn sync(&self, revision: (u64, u64), count: usize, selected: Option<usize>, window: &Window) {
        if self.revision.get() != Some(revision) || self.state.item_count() != count {
            self.revision.set(Some(revision));
            let top = self.state.logical_scroll_top();
            self.state.reset(count);
            self.state.scroll_to(top);
            if self.reveal.get() {
                window.request_animation_frame();
            }
            return;
        }
        if self.reveal.take()
            && let Some(line) = selected
        {
            self.state.scroll_to_reveal_item(line);
        }
    }
}
