use gpui::{Context, ElementId, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, px};
use gpuikit::layout::v_stack;
use gpuikit::theme::Themeable;

use crate::app::TimeChimpApp;
use crate::domain::review::{Findings, Line, lines, tag_mismatches};

use super::week::{EntryLine, EntryRow};
use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_review(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let card = table_card(theme).child(Self::entry_header(theme).rounded_t(px(5.)));
        let body = v_stack().flex_1().min_h_0().gap(px(8.));
        let Some(rows) = self.session.review_rows() else {
            let message = if self.busy().is_some() { "Loading review…" } else { NOT_LOADED };
            return body.child(card.child(empty_message(theme, message)));
        };
        if rows.is_empty() {
            return body.child(card.child(empty_message(theme, "No time registered this month.")));
        }
        let mismatches = tag_mismatches(rows);
        let findings = Findings::of(rows, &mismatches, &self.session.settings.excluded_tasks);
        let summary = div()
            .px(px(4.))
            .text_color(if findings.any() { theme.warning() } else { theme.success() })
            .child(findings.label());
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
                    let tags_differ = mismatches.get(&line.id).map(|m| m.label());
                    let row = EntryRow { selected: selected == Some(line.id), wrap: true, tags_differ };
                    self.entry_row(theme, i, &line, row, cx).into_any_element()
                }
            })
            .collect();
        body.child(summary).child(card.child(table_rows("review-rows", lines).track_scroll(&self.review_scroll)))
    }
}
