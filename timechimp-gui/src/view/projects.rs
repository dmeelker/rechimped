//! The Projects tab.

use gpui::{IntoElement, ParentElement, Styled, prelude::FluentBuilder, px};
use gpuikit::layout::h_stack;
use gpuikit::theme::Themeable;

use crate::app::TimeChimpApp;

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_projects(&self, theme: &T) -> impl IntoElement {
        let projects = panel(theme, format!("My projects ({})", self.session.projects.len()), None)
            .flex_1()
            .child(header_row(
                theme,
                [cell(None, "Customer"), cell(None, "Project"), right_cell(60., "Tasks"), cell(Some(70.), "Active")],
            ))
            .child(table_rows("project-rows", self.session.projects.iter().map(|p| {
                let tasks = self.session.project_tasks.get(&p.id).map_or(0, |t| t.len());
                h_stack()
                    .w_full()
                    .py(px(6.))
                    .px(px(4.))
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .when(!p.active, |d| d.text_color(theme.fg_muted()))
                    .child(cell(None, p.customer_name.clone()))
                    .child(cell(None, p.name.clone()))
                    .child(right_cell(60., tasks.to_string()))
                    .child(cell(Some(70.), if p.active { "yes" } else { "no" }))
            })));
        let tasks = panel(theme, format!("Tasks ({})", self.session.tasks.len()), None)
            .w(px(360.))
            .flex_none()
            .child(header_row(theme, [cell(None, "Task"), cell(Some(80.), "Billable")]))
            .child(table_rows("task-rows", self.session.tasks.iter().map(|t| {
                h_stack()
                    .w_full()
                    .py(px(6.))
                    .px(px(4.))
                    .border_b_1()
                    .border_color(theme.border_subtle())
                    .when(!t.billable, |d| d.text_color(theme.fg_muted()))
                    .child(cell(None, t.name.clone()))
                    .child(cell(Some(80.), if t.billable { "yes" } else { "no" }))
            })));
        h_stack()
            .flex_1()
            .min_h_0()
            .items_start()
            .gap(px(12.))
            .child(projects.max_h_full())
            .child(tasks.max_h_full())
    }
}
