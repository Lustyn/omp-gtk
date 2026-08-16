use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use super::icons;
use crate::bridge::protocol::{TodoItem, TodoPhase, TodoStatus};

const ACTIVE_TASK_CAP: usize = 5;
const FOLLOWING_PHASE_CAP: usize = 4;

pub(crate) fn validate_phases(phases: &[TodoPhase]) -> Result<(), String> {
    let mut phase_names = HashSet::new();
    let mut task_contents = HashSet::new();
    for phase in phases {
        if phase.name.trim().is_empty() {
            return Err("A todo phase name cannot be empty.".to_owned());
        }
        if !phase_names.insert(phase.name.as_str()) {
            return Err(format!(
                "A todo phase named “{}” already exists.",
                phase.name
            ));
        }
        for task in &phase.tasks {
            if task.content.trim().is_empty() {
                return Err("A todo task cannot be empty.".to_owned());
            }
            if !task_contents.insert(task.content.as_str()) {
                return Err(format!(
                    "A todo task named “{}” already exists.",
                    task.content
                ));
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct TodoPanel {
    pub(crate) root: gtk::Box,
    list: gtk::Box,
    status: gtk::Label,
    rail: gtk::ToggleButton,
    progress: gtk::ProgressBar,
    count: gtk::Label,
}

impl TodoPanel {
    pub(crate) fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.set_halign(gtk::Align::End);
        root.set_valign(gtk::Align::Center);
        root.set_margin_end(12);
        root.add_css_class("todo-surface");

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.set_size_request(350, -1);
        card.add_css_class("todo-panel");

        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        heading.add_css_class("todo-panel-header");
        heading.append(&icons::icon(icons::Icon::ListTodo, 14));
        let title = gtk::Label::new(Some("Plan"));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.add_css_class("todo-panel-heading");
        heading.append(&title);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 7);
        list.set_margin_bottom(9);
        list.set_margin_start(10);
        list.set_margin_end(10);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .max_content_height(340)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        scroll.add_css_class("todo-panel-scroll");

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_visible(false);
        status.set_accessible_role(gtk::AccessibleRole::Alert);
        status.add_css_class("todo-panel-status");

        card.append(&heading);
        card.append(&scroll);
        card.append(&status);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
        revealer.set_transition_duration(180);
        revealer.set_child(Some(&card));
        root.append(&revealer);

        let progress = gtk::ProgressBar::new();
        progress.set_orientation(gtk::Orientation::Vertical);
        progress.set_inverted(true);
        progress.set_valign(gtk::Align::Fill);
        progress.set_vexpand(true);
        progress.add_css_class("todo-rail-progress");
        let count = gtk::Label::new(None);
        count.add_css_class("todo-rail-count");
        let rail_content = gtk::Box::new(gtk::Orientation::Vertical, 5);
        rail_content.append(&icons::icon(icons::Icon::ListTodo, 13));
        rail_content.append(&progress);
        rail_content.append(&count);

        let rail = gtk::ToggleButton::new();
        rail.set_child(Some(&rail_content));
        rail.set_tooltip_text(Some("Hover to show the work plan"));
        rail.add_css_class("todo-rail");
        root.append(&rail);

        let pinned = Rc::new(Cell::new(false));
        let revealer_for_toggle = revealer.clone();
        let pinned_for_toggle = pinned.clone();
        rail.connect_toggled(move |button| {
            pinned_for_toggle.set(button.is_active());
            revealer_for_toggle.set_reveal_child(button.is_active());
        });
        let revealer_for_focus = revealer.clone();
        rail.connect_has_focus_notify(move |button| {
            if button.has_focus() {
                revealer_for_focus.set_reveal_child(true);
            }
        });

        let motion = gtk::EventControllerMotion::new();
        let revealer_for_enter = revealer.clone();
        motion.connect_enter(move |_, _, _| revealer_for_enter.set_reveal_child(true));
        let revealer_for_leave = revealer.clone();
        motion.connect_leave(move |_| {
            if !pinned.get() {
                revealer_for_leave.set_reveal_child(false);
            }
        });
        root.add_controller(motion);

        let panel = Self {
            root,
            list,
            status,
            rail,
            progress,
            count,
        };
        panel.set_phases(&[]);
        panel
    }

    pub(crate) fn set_phases(&self, phases: &[TodoPhase]) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let phases = phases
            .iter()
            .filter(|phase| !phase.tasks.is_empty())
            .collect::<Vec<_>>();
        let total = phases.iter().map(|phase| phase.tasks.len()).sum::<usize>();
        let closed = phases
            .iter()
            .flat_map(|phase| &phase.tasks)
            .filter(|task| task.status.is_closed())
            .count();
        self.root.set_visible(total > 0);
        if total == 0 {
            return;
        }

        self.progress.set_fraction(closed as f64 / total as f64);
        self.count.set_text(&format!("{closed}/{total}"));
        let label = format!("Show work plan, {closed} of {total} complete");
        self.rail.set_tooltip_text(Some(&label));
        self.rail
            .update_property(&[gtk::accessible::Property::Label(&label)]);

        let active_index = phases
            .iter()
            .position(|phase| phase.tasks.iter().any(|task| !task.status.is_closed()));
        let Some(active_index) = active_index else {
            let complete = gtk::Label::new(Some("All tasks complete"));
            complete.set_xalign(0.0);
            complete.add_css_class("todo-complete");
            self.list.append(&complete);
            return;
        };

        let visible_phases = phases
            .iter()
            .skip(active_index)
            .take(1 + FOLLOWING_PHASE_CAP)
            .copied()
            .collect::<Vec<_>>();
        for (offset, phase) in visible_phases.iter().enumerate() {
            self.list.append(&phase_view(phase, offset == 0));
        }
        let hidden_phases = phases
            .len()
            .saturating_sub(active_index + visible_phases.len());
        if hidden_phases > 0 {
            let summary = gtk::Label::new(Some(&format!("… {hidden_phases} later phases")));
            summary.set_xalign(0.0);
            summary.add_css_class("todo-more");
            self.list.append(&summary);
        }
    }

    #[cfg(feature = "ui-stories")]
    pub(crate) fn set_revealed(&self, revealed: bool) {
        self.rail.set_active(revealed);
    }

    pub(crate) fn set_error(&self, message: Option<&str>) {
        if let Some(message) = message {
            self.status.set_text(message);
            self.status.set_visible(true);
        } else {
            self.status.set_visible(false);
        }
    }
}

fn phase_view(phase: &TodoPhase, show_tasks: bool) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.add_css_class("todo-phase");

    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some(&phase.name));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class(if show_tasks {
        "todo-phase-title-active"
    } else {
        "todo-phase-title"
    });
    heading.append(&title);
    let closed = phase
        .tasks
        .iter()
        .filter(|task| task.status.is_closed())
        .count();
    let progress = gtk::Label::new(Some(&format!("{closed}/{}", phase.tasks.len())));
    progress.add_css_class("todo-phase-progress");
    heading.append(&progress);
    root.append(&heading);

    if show_tasks {
        let (tasks, hidden) = collapsed_tasks(&phase.tasks, ACTIVE_TASK_CAP);
        for task in tasks {
            root.append(&task_view(task));
        }
        if hidden > 0 {
            let summary = gtk::Label::new(Some(&format!("… {hidden} more open tasks")));
            summary.set_xalign(0.0);
            summary.add_css_class("todo-more");
            root.append(&summary);
        }
    }

    root.upcast()
}

fn collapsed_tasks(tasks: &[TodoItem], cap: usize) -> (Vec<&TodoItem>, usize) {
    let open = tasks
        .iter()
        .filter(|task| !task.status.is_closed())
        .collect::<Vec<_>>();
    let mut visible = tasks
        .iter()
        .rfind(|task| task.status.is_closed())
        .into_iter()
        .collect::<Vec<_>>();
    visible.extend(open.iter().take(cap).copied());
    let hidden = open.len().saturating_sub(cap);
    (visible, hidden)
}

fn task_view(task: &TodoItem) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    row.add_css_class("todo-task");
    row.add_css_class(task.status.css_class());

    let marker = gtk::Label::new(Some(task.status.marker()));
    marker.set_valign(gtk::Align::Start);
    marker.set_tooltip_text(Some(task.status.label()));
    marker.update_property(&[gtk::accessible::Property::Label(task.status.label())]);
    marker.add_css_class("todo-task-marker");
    row.append(&marker);

    let copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
    copy.set_hexpand(true);
    let content = gtk::Label::new(Some(&task.content));
    content.set_xalign(0.0);
    content.set_wrap(true);
    content.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    content.add_css_class("todo-task-content");
    copy.append(&content);
    if let Some(blocker) = task.blocker.as_deref() {
        let blocker = gtk::Label::new(Some(blocker));
        blocker.set_xalign(0.0);
        blocker.set_wrap(true);
        blocker.add_css_class("todo-blocker");
        copy.append(&blocker);
    }
    row.append(&copy);
    row.upcast()
}

impl TodoStatus {
    fn is_closed(self) -> bool {
        matches!(self, Self::Completed | Self::Abandoned)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "In progress",
            Self::Completed => "Completed",
            Self::Abandoned => "Abandoned",
            Self::Blocked => "Blocked",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Pending => "○",
            Self::InProgress => "●",
            Self::Completed => "✓",
            Self::Abandoned => "—",
            Self::Blocked => "!",
        }
    }

    fn css_class(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in-progress",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
            Self::Blocked => "blocked",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{collapsed_tasks, validate_phases};
    use crate::bridge::protocol::{TodoItem, TodoPhase, TodoStatus};

    fn task(content: &str, status: TodoStatus) -> TodoItem {
        TodoItem {
            content: content.to_owned(),
            status,
            blocker: None,
        }
    }

    #[test]
    fn collapsed_plan_keeps_only_latest_closed_context_and_open_window() {
        let tasks = vec![
            task("Old completion", TodoStatus::Completed),
            task("Latest completion", TodoStatus::Completed),
            task("Current", TodoStatus::InProgress),
            task("Next", TodoStatus::Pending),
            task("Later", TodoStatus::Blocked),
        ];
        let (visible, hidden) = collapsed_tasks(&tasks, 2);
        assert_eq!(
            visible
                .iter()
                .map(|task| task.content.as_str())
                .collect::<Vec<_>>(),
            ["Latest completion", "Current", "Next"]
        );
        assert_eq!(hidden, 1);
    }

    #[test]
    fn validates_complete_ordered_plans() {
        let phases = vec![
            TodoPhase {
                name: "Research".to_owned(),
                tasks: vec![
                    task("Inspect protocol", TodoStatus::Pending),
                    task("Map UI", TodoStatus::Pending),
                ],
            },
            TodoPhase {
                name: "Ship".to_owned(),
                tasks: vec![task("Verify behavior", TodoStatus::Pending)],
            },
        ];
        assert_eq!(validate_phases(&phases), Ok(()));
    }

    #[test]
    fn rejects_duplicate_phase_and_task_names() {
        let duplicate_phase = vec![
            TodoPhase {
                name: "Build".to_owned(),
                tasks: vec![task("One", TodoStatus::Pending)],
            },
            TodoPhase {
                name: "Build".to_owned(),
                tasks: vec![task("Two", TodoStatus::Pending)],
            },
        ];
        assert!(validate_phases(&duplicate_phase).is_err());

        let duplicate_task = vec![
            TodoPhase {
                name: "Build".to_owned(),
                tasks: vec![task("Same", TodoStatus::Pending)],
            },
            TodoPhase {
                name: "Verify".to_owned(),
                tasks: vec![task("Same", TodoStatus::Pending)],
            },
        ];
        assert!(validate_phases(&duplicate_task).is_err());
    }
}
