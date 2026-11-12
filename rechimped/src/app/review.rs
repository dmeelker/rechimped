use gpui::{Context, Window};

use crate::domain::review::{Line, lines};
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

    pub(super) fn scroll_to_review_selection(&self) {
        let Some(rows) = self.session.review_rows() else { return };
        let Some(index) = rows.iter().position(|r| Some(r.id) == self.session.selected_review_id()) else { return };
        if let Some(line) = lines(rows).iter().position(|l| *l == Line::Entry(index)) {
            self.review_scroll.scroll_to_item(line);
        }
    }
}
