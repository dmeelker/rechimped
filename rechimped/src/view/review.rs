use gpui::{AnyElement, Context, ElementId, IntoElement, ParentElement, Styled, Window, div, list, px};
use gpuikit::layout::v_stack;
use gpuikit::theme::{ActiveTheme, Themeable};

use crate::app::TimeChimpApp;
use crate::domain::review::Line;

use super::week::{EntryLine, EntryRow};
use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_review(&self, window: &Window, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let card = table_card(theme).child(Self::entry_header(theme).rounded_t(px(5.)));
        let body = v_stack().flex_1().min_h_0().gap(px(8.));
        let (Some(rows), Some(analysis)) = (self.session.review_rows(), self.session.review_analysis()) else {
            let message = if self.busy().is_some() { "Loading review…" } else { NOT_LOADED };
            return body.child(card.child(empty_message(theme, message)));
        };
        if rows.is_empty() {
            return body.child(card.child(empty_message(theme, "No time registered this month.")));
        }
        let findings = analysis.findings;
        let summary = div()
            .px(px(4.))
            .text_color(if findings.any() { theme.warning() } else { theme.success() })
            .child(findings.label());
        let selected = rows
            .iter()
            .position(|r| Some(r.id) == self.session.selected_review_id())
            .and_then(|index| analysis.line_of(index));
        self.review_list.sync(self.session.review_revision(), analysis.lines.len(), selected, window);
        // Only the lines in view are drawn, each frame; `sync` resets the list when they change.
        // The list has no height of its own, so the card fills the page.
        let lines = list(self.review_list.state.clone(), cx.processor(|this, line, _, cx| this.render_review_line(line, cx)))
            .flex_1()
            .min_h_0();
        body.child(summary).child(card.flex_1().child(lines))
    }

    fn render_review_line(&self, n: usize, cx: &mut Context<Self>) -> AnyElement {
        let (Some(rows), Some(analysis)) = (self.session.review_rows(), self.session.review_analysis()) else {
            return div().into_any_element();
        };
        let theme = cx.theme().clone();
        match analysis.lines.get(n) {
            Some(&Line::Day { date, total }) => {
                day_header(&theme, ElementId::NamedInteger("review-day".into(), n as u64), date, hh_mm(total))
                    .into_any_element()
            }
            Some(&Line::Entry(i)) => {
                let line = EntryLine::from(&rows[i]);
                let tags_differ = analysis.mismatches.get(&line.id).map(|m| m.label());
                let row = EntryRow { selected: self.session.selected_review_id() == Some(line.id), wrap: true, tags_differ };
                self.entry_row(&theme, i, &line, row, cx).into_any_element()
            }
            None => div().into_any_element(),
        }
    }
}
