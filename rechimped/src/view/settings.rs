use gpui::{
    Context, ElementId, FontWeight, ScrollHandle, Window, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, StatefulInteractiveElement,
    Styled, div, prelude::FluentBuilder, px,
};
use gpuikit::a11y::FocusNavigation;
use gpuikit::elements::button::button;
use gpuikit::elements::icon_button::icon_button;
use gpuikit::elements::tooltip::tooltip;
use gpuikit::icons::Icons;
use gpuikit::elements::checkbox::{CheckState, checkbox_box};
use gpuikit::elements::text_field::{TextField, text_field};
use gpuikit::layout::{h_stack, v_stack};
use gpuikit::theme::{ControlSize, Themeable};

use crate::app::{Cancel, Confirm, TimeChimpApp};
use crate::session;
use crate::forms::{Column, ListNav, PresetsForm, ProjectRow, shift, ProjectTagsForm, ProjectVisibility, SettingsForm, SettingsPage, TaskVisibility};

use super::widgets::*;

impl TimeChimpApp {
    pub(super) fn render_settings(&self, form: &SettingsForm, window: &Window, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = form.focus.is_focused(window);
        let categories = SettingsPage::ALL.iter().map(|&page| {
            let selected = page == form.page;
            div()
                .id(ElementId::Name(format!("settings-page-{page:?}").into()))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(selected, |d| d.bg(theme.accent_bg()).font_weight(FontWeight::SEMIBOLD))
                .when(!selected, |d| d.hover(|s| s.bg(theme.surface())))
                .on_click(cx.listener(move |this, _, w, cx| this.show_settings_page(page, w, cx)))
                .child(page.title())
        });
        let sidebar = v_stack()
            .w(px(190.))
            .flex_none()
            .p(px(12.))
            .gap(px(2.))
            .bg(theme.surface_secondary())
            .border_r_1()
            .border_color(theme.border())
            // The card's own corner radius, which a child's background would otherwise poke out of.
            .rounded_l(px(10.))
            .child(
                div()
                    .px(px(10.))
                    .pt(px(8.))
                    .pb(px(12.))
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.accent())
                    .child("Settings"),
            )
            .children(categories);

        let content = match form.page {
            SettingsPage::General => self.render_general(form, theme, cx),
            SettingsPage::Projects => self.render_project_visibility(&form.projects, focused, theme, cx),
            SettingsPage::Tasks => self.render_task_visibility(&form.tasks, focused, theme, cx),
            SettingsPage::ProjectTags => self.render_project_tags(&form.tags, focused, theme, cx),
            SettingsPage::Presets => self.render_presets(&form.presets, theme, cx),
        };
        let footer = h_stack()
            .gap(px(8.))
            .items_center()
            .child(div().flex_1().min_w_0().map(|d| match form.error.clone() {
                Some(e) => d.p(px(8.)).rounded(px(6.)).bg(theme.danger().opacity(0.15)).text_color(theme.danger()).child(e),
                None => d.child(muted(theme, key_hints(form.page)).text_xs()),
            }))
            .child(pop(
                theme,
                button("settings-cancel", "Cancel").on_click(cx.listener(|this, _, w, cx| this.cancel_settings(w, cx))),
            ))
            .child(primary_button(
                theme,
                "settings-save",
                "Save",
                !form.is_dirty(cx),
                cx.listener(|this, _, w, cx| this.save_settings(w, cx)),
            ));
        let page = v_stack()
            .flex_1()
            .min_w_0()
            .p(px(24.))
            .gap(px(14.))
            .child(
                v_stack()
                    .gap(px(4.))
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(form.page.title()))
                    .child(muted(theme, form.page.description())),
            )
            .child(v_stack().flex_1().min_h_0().child(content))
            .child(footer);

        let card = h_stack()
            .id("settings-card")
            .key_context("Modal")
            .track_focus(&form.focus)
            .moves_focus_on_tab()
            .on_action(cx.listener(|this, _: &Confirm, w, cx| this.save_settings(w, cx)))
            .on_action(cx.listener(|this, _: &Cancel, w, cx| this.cancel_settings(w, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, w, cx| this.on_settings_key(event, w, cx)))
            .h(px(540.))
            .items_stretch()
            .child(sidebar)
            .child(page);
        Self::modal(theme, "settings", 880., card, None)
    }

    fn render_general(&self, form: &SettingsForm, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let values = form.values(cx);
        let work_days = values.work_days.iter().filter(|on| **on).count();
        let contract_hours = self.session.contract_hours();
        let contract = crate::domain::week::contract_summary(contract_hours, work_days);
        let goal = crate::validate::billable_goal(&values.billable_goal)
            .and_then(|goal| crate::domain::week::goal_summary(contract_hours, goal))
            .unwrap_or_default();
        let row = |label: &'static str, control: gpui::AnyElement| {
            h_stack()
                .gap(px(16.))
                .items_center()
                .child(div().w(px(130.)).flex_none().text_color(theme.fg_muted()).child(label))
                .child(div().flex_1().min_w_0().child(control))
        };
        v_stack()
            .gap(px(14.))
            .child(row("Work days", h_stack().gap(px(14.)).children(form.work_days.iter().cloned()).into_any_element()))
            .child(row("Contract", muted(theme, contract).into_any_element()))
            .child(row(
                "Billable target",
                number_field(
                    text_field(&form.billable_goal, cx).placeholder("80"),
                    h_stack().gap(px(12.)).child("%").child(muted(theme, goal)).text_color(theme.fg_muted()),
                ),
            ))
            .into_any_element()
    }

    fn render_project_visibility(&self, p: &ProjectVisibility, focused: bool, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows = p.projects.iter().enumerate().map(|(i, project)| {
            let shown = !p.hidden.contains(&project.id);
            check_row(theme, ElementId::NamedInteger("settings-project".into(), project.id as u64), shown, focused && i == p.nav.item)
                .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| f.projects.toggle_at(Some(i)))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when(!shown, |d| d.text_color(theme.fg_muted()))
                        .child(project.name.clone()),
                )
                .child(muted(theme, project.customer.clone()).text_xs().flex_none())
        });
        let list = list_box(theme, "settings-projects", &p.nav.items_scroll)
            .p(px(6.))
            .when(p.projects.is_empty(), |d| d.child(muted(theme, "No projects.")))
            .children(rows);
        let shown = p.projects.len() - p.hidden.len();
        let set_all = |id: &'static str, label: &'static str, show: bool, disabled: bool| {
            pop(
                theme,
                button(id, label)
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| f.projects.set_all(show)))),
            )
        };
        v_stack()
            .flex_1()
            .min_h_0()
            .gap(px(8.))
            .child(
                h_stack()
                    .gap(px(8.))
                    .items_center()
                    .child(muted(theme, format!("{shown} of {} offered in the forms", p.projects.len())).flex_1().min_w_0())
                    .child(set_all("settings-projects-all", "Select all", true, p.hidden.is_empty()))
                    .child(set_all("settings-projects-none", "Clear all", false, shown == 0)),
            )
            .child(list)
            .into_any_element()
    }

    fn render_task_visibility(&self, form: &TaskVisibility, focused: bool, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let projects = form.projects.iter().map(|(project, tasks)| {
            let hidden = form.hidden_count(project.id);
            let badge = Some(format!("{} of {}", tasks.len() - hidden, tasks.len()));
            (project, form.hidden_projects.contains(&project.id), badge)
        });
        let in_projects = focused && form.nav.column == Column::Projects;
        let projects = project_list(theme, "task-projects", projects, form.current, in_projects, &form.nav, |f, i| f.tasks.pick_project(i), cx);
        let current = form.projects.get(form.current);
        let cursor = (focused && form.nav.column == Column::Items).then_some(form.nav.item);
        let tasks = match current {
            None => list_box(theme, "task-choices", &form.nav.items_scroll).p(px(12.)).child(muted(theme, "No projects with tasks.")),
            Some((project, tasks)) => {
                list_box(theme, "task-choices", &form.nav.items_scroll).p(px(8.)).children(tasks.iter().enumerate().map(|(i, (task_id, name))| {
                    let shown = !form.is_hidden(project.id, *task_id);
                    check_row(theme, ElementId::NamedInteger("task-choice".into(), *task_id as u64), shown, cursor == Some(i))
                        .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| f.tasks.toggle_at(Some(i)))))
                        .child(div().when(!shown, |d| d.text_color(theme.fg_muted())).child(name.clone()))
                }))
            }
        };
        let hidden = current.map_or(0, |(p, _)| form.hidden_count(p.id));
        let total = current.map_or(0, |(_, tasks)| tasks.len());
        let set_all = |id: &'static str, label: &'static str, shown: bool, disabled: bool| {
            pop(
                theme,
                button(id, label)
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| f.tasks.set_all(shown)))),
            )
        };
        let title = current.map_or_else(String::new, |(p, _)| format!("Tasks of {}", p.name));
        let tasks = settings_column(
            theme,
            h_stack()
                .gap(px(8.))
                .child(muted(theme, title).flex_1().min_w_0().truncate())
                .child(set_all("tasks-all", "Select all", true, current.is_none() || hidden == 0))
                .child(set_all("tasks-none", "Clear all", false, current.is_none() || hidden == total)),
            tasks,
        )
        .flex_1()
        .min_w_0();
        h_stack().flex_1().min_h_0().items_stretch().gap(px(12.)).child(projects).child(tasks).into_any_element()
    }

    fn render_project_tags(&self, form: &ProjectTagsForm, focused: bool, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let projects = form.projects.iter().map(|project| {
            let count = form.count(project.id);
            (project, form.hidden_projects.contains(&project.id), Some(format!("{count} of {}", form.tags.len())))
        });
        let in_projects = focused && form.nav.column == Column::Projects;
        let projects = project_list(theme, "tag-projects", projects, form.current, in_projects, &form.nav, |f, i| f.tags.pick_project(i), cx);
        let cursor = (focused && form.nav.column == Column::Items).then_some(form.nav.item);
        let tags = match form.current_project() {
            None => list_box(theme, "tag-choices", &form.nav.items_scroll).p(px(12.)).child(muted(theme, "No projects.")),
            Some(project) => {
                list_box(theme, "tag-choices", &form.nav.items_scroll).p(px(8.)).children(form.tags.iter().enumerate().map(|(i, (tag_id, name))| {
                    let checked = form.is_checked(project, *tag_id);
                    check_row(theme, ElementId::NamedInteger("tag-choice".into(), *tag_id as u64), checked, cursor == Some(i))
                        .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| f.tags.toggle_at(Some(i)))))
                        .child(name.clone())
                }))
            }
        };
        let count = form.current_project().map_or(0, |p| form.count(p));
        let no_project = form.current_project().is_none();
        let set_all = |id: &'static str, label: &'static str, checked: bool, disabled: bool| {
            pop(
                theme,
                button(id, label)
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| f.tags.set_all(checked)))),
            )
        };
        let title = form.projects.get(form.current).map_or_else(String::new, |p| format!("Tags for {}", p.name));
        let note = (!form.configured)
            .then_some("No project has tags set yet, so each offers all of them. The first change sets them for every project.");
        let tags = settings_column(
            theme,
            h_stack()
                .gap(px(8.))
                .child(muted(theme, title).flex_1().min_w_0().truncate())
                .child(set_all("project-tags-all", "Select all", true, no_project || count == form.tags.len()))
                .child(set_all("project-tags-none", "Clear all", false, no_project || count == 0)),
            tags,
        )
        .when_some(note, |d, note| d.child(muted(theme, note).text_xs()))
        .flex_1()
        .min_w_0();
        h_stack().flex_1().min_h_0().items_stretch().gap(px(12.)).child(projects).child(tags).into_any_element()
    }
}

impl TimeChimpApp {
    fn render_presets(&self, form: &PresetsForm, theme: &T, cx: &mut Context<Self>) -> gpui::AnyElement {
        let trips = form.trips.iter().enumerate().map(|(i, trip)| {
            let details = format!("{} → {} · {}", trip.from, trip.to, trip.project);
            let label = v_stack().child(div().truncate().child(trip.name.clone())).child(muted(theme, details).text_xs().truncate());
            preset_row(
                theme,
                ElementId::NamedInteger("preset-trip".into(), i as u64),
                label,
                (i, form.trips.len()),
                |f, i, delta| shift(&mut f.presets.trips, i, delta),
                |f, i| {
                    f.presets.trips.remove(i);
                },
                cx,
            )
        });
        let trips: Vec<_> = trips.collect();
        let times = form.times.iter().enumerate().map(|(i, time)| {
            let task = time.task.as_deref().map(|t| format!(" / {t}")).unwrap_or_default();
            let label = v_stack()
                .child(div().truncate().child(session::time_preset_label(time).to_owned()))
                .child(muted(theme, format!("{}{task}", time.project)).text_xs().truncate());
            preset_row(
                theme,
                ElementId::NamedInteger("preset-time".into(), i as u64),
                label,
                (i, form.times.len()),
                |f, i, delta| shift(&mut f.presets.times, i, delta),
                |f, i| {
                    f.presets.times.remove(i);
                },
                cx,
            )
        });
        let times: Vec<_> = times.collect();
        let column = |title: &'static str, empty: &'static str, scroll: &ScrollHandle, rows: Vec<gpui::Stateful<gpui::Div>>| {
            let list = list_box(theme, title, scroll).p(px(6.)).gap(px(4.)).when(rows.is_empty(), |d| d.child(muted(theme, empty).p(px(6.))));
            settings_column(theme, h_stack().child(muted(theme, title)), list.children(rows)).flex_1().min_w_0()
        };
        h_stack()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .gap(px(12.))
            .child(column("Trips", "No trip presets. Use \"Save as preset\" in the trip form.", &form.trips_scroll, trips))
            .child(column("Time entries", "No time presets. Use \"Save as preset\" in the time form.", &form.times_scroll, times))
            .into_any_element()
    }
}

#[allow(clippy::too_many_arguments)]
fn preset_row(
    theme: &T,
    id: ElementId,
    label: gpui::Div,
    at: (usize, usize),
    move_by: fn(&mut SettingsForm, usize, isize),
    remove: fn(&mut SettingsForm, usize),
    cx: &mut Context<TimeChimpApp>,
) -> gpui::Stateful<gpui::Div> {
    let (i, len) = at;
    let action = |name: &'static str, icon: gpui::Svg, tip: &'static str, disabled: bool| {
        icon_button(ElementId::Name(format!("{id}-{name}").into()), icon)
            .box_size(gpui::rems(1.5))
            .icon_size(gpui::rems(0.875))
            .disabled(disabled)
            .tooltip(tooltip(tip))
    };
    h_stack()
        .id(id.clone())
        .flex_none()
        .w_full()
        .px(px(6.))
        .py(px(4.))
        .gap(px(6.))
        .items_center()
        .rounded(px(4.))
        .hover(|s| s.bg(theme.surface_secondary()))
        .child(label.flex_1().min_w_0())
        .child(
            action("up", Icons::arrow_up(), "Move up", i == 0)
                .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| move_by(f, i, -1)))),
        )
        .child(
            action("down", Icons::arrow_down(), "Move down", i + 1 == len)
                .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| move_by(f, i, 1)))),
        )
        .child(
            action("delete", Icons::trash(), "Delete (on Save)", false)
                .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| remove(f, i)))),
        )
}

#[allow(clippy::too_many_arguments)]
fn project_list<'a>(
    theme: &T,
    id: &'static str,
    projects: impl Iterator<Item = (&'a ProjectRow, bool, Option<String>)>,
    current: usize,
    active: bool,
    nav: &ListNav,
    pick: fn(&mut SettingsForm, usize),
    cx: &mut Context<TimeChimpApp>,
) -> gpui::Div {
    let mut in_hidden = false;
    let rows = projects.enumerate().map(|(i, (project, hidden, badge))| {
        let selected = i == current;
        let first_hidden = hidden && !in_hidden;
        in_hidden |= hidden;
        let subtitle = if project.active { project.customer.clone() } else { format!("{} · inactive", project.customer) };
        let row = h_stack()
            .id(ElementId::NamedInteger(format!("{id}-row").into(), i as u64))
            .relative()
            .flex_none()
            .px(px(10.))
            .py(px(6.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(theme.border_subtle())
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.accent_bg()))
            .when(!selected && hidden, |d| d.bg(theme.surface_secondary()))
            .when(!selected, |d| d.hover(|s| s.bg(theme.surface_secondary())))
            .when(selected && active, |d| d.child(div().absolute().left_0().top_0().bottom_0().w(px(3.)).bg(theme.accent())))
            .on_click(cx.listener(move |this, _, w, cx| this.edit_settings(w, cx, |f| pick(f, i))))
            .child(
                v_stack()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .when(selected, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .when(hidden || !project.active, |d| d.text_color(theme.fg_muted()))
                            .child(project.name.clone()),
                    )
                    .child(muted(theme, subtitle).text_xs().truncate()),
            )
            .when_some(badge, |d, badge| d.child(muted(theme, badge).flex_none()));
        // Wrapped rather than a row of its own, so row i stays child i for `scroll_to_item`.
        v_stack()
            .flex_none()
            .when(first_hidden, |d| {
                d.child(
                    h_stack()
                        .px(px(10.))
                        .pt(px(10.))
                        .pb(px(4.))
                        .items_center()
                        .border_b_1()
                        .border_color(theme.border())
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.fg_muted())
                        .child("Hidden on the Projects page"),
                )
            })
            .child(row)
    });
    let list = list_box(theme, id, &nav.projects_scroll).children(rows);
    settings_column(theme, h_stack().child(muted(theme, "Projects")), list).w(px(240.)).flex_none()
}

/// Its header row as tall as a button, so columns side by side line up.
fn settings_column(theme: &T, header: gpui::Div, list: gpui::Stateful<gpui::Div>) -> gpui::Div {
    v_stack().min_h_0().gap(px(8.)).child(header.h(theme.control(ControlSize::Medium).height).items_center()).child(list)
}

fn check_row(theme: &T, id: ElementId, checked: bool, cursor: bool) -> gpui::Stateful<gpui::Div> {
    h_stack()
        .id(id)
        .flex_none()
        .px(px(6.))
        .py(px(4.))
        .gap(px(10.))
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(if cursor { theme.accent() } else { gpui::transparent_black() })
        .cursor_pointer()
        .hover(|s| s.bg(theme.surface_secondary()))
        .child(checkbox_box(if checked { CheckState::Checked } else { CheckState::Unchecked }))
}

fn list_box(theme: &T, id: &'static str, scroll: &ScrollHandle) -> gpui::Stateful<gpui::Div> {
    v_stack()
        .id(id)
        .track_scroll(scroll)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .border_1()
        .border_color(theme.border())
        .rounded(px(6.))
        .bg(theme.surface())
}

fn key_hints(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::General => "Ctrl+Tab next page · Enter save · Esc cancel",
        SettingsPage::Projects => "↑↓ or type to move · Space toggle · Ctrl+Tab next page · Enter save",
        SettingsPage::Tasks | SettingsPage::ProjectTags => "↑↓ or type to move · ←→ switch list · Space toggle · Ctrl+Tab next page",
        SettingsPage::Presets => "Ctrl+Tab next page · Enter save · Esc cancel",
    }
}

fn number_field(field: TextField, unit: gpui::Div) -> gpui::AnyElement {
    h_stack().gap(px(8.)).items_center().child(div().w(px(100.)).child(field)).child(unit).into_any_element()
}

impl TimeChimpApp {
    pub(super) fn render_about(&self, theme: &T, cx: &mut Context<Self>) -> impl IntoElement {
        let card = v_stack()
            .id("about-card")
            .p(px(24.))
            .gap(px(6.))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(theme.accent()).child("Rechimped"))
            .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
            .child(muted(theme, env!("CARGO_PKG_DESCRIPTION")))
            .child(
                h_stack().mt(px(16.)).justify_end().child(primary_button(
                    theme,
                    "about-close",
                    "Close",
                    false,
                    cx.listener(|this, _, w, cx| this.close_overlay(w, cx)),
                )),
            );
        let close = cx.listener(|this, _: &gpui::MouseDownEvent, w, cx| this.close_overlay(w, cx));
        Self::modal(theme, "about", 400., card, Some(Box::new(close)))
    }
}
