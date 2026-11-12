use gpui::{Context, ElementId, IntoElement, ParentElement, StatefulInteractiveElement, Styled, px};

use crate::app::TimeChimpApp;
use crate::domain::review::{Line, lines};

use super::week::{EntryLine, EntryRow};
use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_review(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let card = table_card(theme).child(Self::entry_header(theme).rounded_t(px(5.)));
        let Some(rows) = self.session.review_rows() else {
            let message = if self.busy().is_some() { "Loading review…" } else { NOT_LOADED };
            return card.child(empty_message(theme, message));
        };
        if rows.is_empty() {
            return card.child(empty_message(theme, "No time registered this month."));
        }
        let selected = self.session.selected_review_id();
        let lines: Vec<_> = lines(rows)
            .into_iter()
            .enumerate()
            .map(|(n, line)| match line {
                Line::Day { date, total } => {
                    day_header(theme, ElementId::NamedInteger("review-day".into(), n as u64), date, hh_mm(total))
                        .into_any_element()
                }
                Line::Entry(i) => {
                    let line = EntryLine::from(&rows[i]);
                    let row = EntryRow { selected: selected == Some(line.id), actions: false, wrap: true };
                    self.entry_row(theme, i, &line, row, cx).into_any_element()
                }
            })
            .collect();
        card.child(table_rows("review-rows", lines).track_scroll(&self.review_scroll))
    }
}
