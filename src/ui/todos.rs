use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use crate::bridge::protocol::{TodoItem, TodoPhase, TodoStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TodoEdit {
    Initialize(Vec<TodoPhase>),
    Append {
        phase: String,
        content: String,
    },
    Start {
        phase: usize,
        task: usize,
    },
    Complete {
        phase: usize,
        task: usize,
    },
    Drop {
        phase: usize,
        task: usize,
    },
    Block {
        phase: usize,
        task: usize,
        blocker: Option<String>,
    },
    Unblock {
        phase: usize,
        task: usize,
    },
    Remove {
        phase: usize,
        task: usize,
    },
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TodoAction {
    AddPhase,
    AddTask { phase: usize },
    Start { phase: usize, task: usize },
    Complete { phase: usize, task: usize },
    Drop { phase: usize, task: usize },
    Block { phase: usize, task: usize },
    Unblock { phase: usize, task: usize },
    Remove { phase: usize, task: usize },
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TodoSummary {
    pub(crate) total: usize,
    pub(crate) completed: usize,
    pub(crate) abandoned: usize,
    pub(crate) blocked: usize,
    pub(crate) active: Option<String>,
}

pub(crate) fn summarize(phases: &[TodoPhase]) -> TodoSummary {
    let tasks = phases.iter().flat_map(|phase| phase.tasks.iter());
    let mut summary = TodoSummary {
        total: 0,
        completed: 0,
        abandoned: 0,
        blocked: 0,
        active: None,
    };
    for task in tasks {
        summary.total += 1;
        match task.status {
            TodoStatus::Completed => summary.completed += 1,
            TodoStatus::Abandoned => summary.abandoned += 1,
            TodoStatus::Blocked => summary.blocked += 1,
            TodoStatus::InProgress => {
                if summary.active.is_none() {
                    summary.active = Some(task.content.clone());
                }
            }
            TodoStatus::Pending => {}
        }
    }
    summary
}

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

pub(crate) fn apply_edit(phases: &[TodoPhase], edit: TodoEdit) -> Result<Vec<TodoPhase>, String> {
    let mut next = phases.to_vec();
    match edit {
        TodoEdit::Initialize(initial) => {
            if !phases.is_empty() {
                return Err("Initialize is only available when the todo list is empty.".to_owned());
            }
            if initial.is_empty() || initial.iter().all(|phase| phase.tasks.is_empty()) {
                return Err("Initialize requires at least one task.".to_owned());
            }
            next = initial;
        }
        TodoEdit::Append { phase, content } => {
            let phase = phase.trim();
            let content = content.trim();
            if phase.is_empty() {
                return Err("A phase name is required.".to_owned());
            }
            if content.is_empty() {
                return Err("A task description is required.".to_owned());
            }
            if next
                .iter()
                .flat_map(|phase| phase.tasks.iter())
                .any(|task| task.content == content)
            {
                return Err(format!("A todo task named “{content}” already exists."));
            }
            let task = TodoItem {
                content: content.to_owned(),
                status: TodoStatus::Pending,
                blocker: None,
            };
            if let Some(existing) = next.iter_mut().find(|candidate| candidate.name == phase) {
                existing.tasks.push(task);
            } else {
                next.push(TodoPhase {
                    name: phase.to_owned(),
                    tasks: vec![task],
                });
            }
        }
        TodoEdit::Start { phase, task } => {
            ensure_task(&next, phase, task)?;
            for candidate in next.iter_mut().flat_map(|phase| phase.tasks.iter_mut()) {
                if candidate.status == TodoStatus::InProgress {
                    candidate.status = TodoStatus::Pending;
                    candidate.blocker = None;
                }
            }
            let target = task_mut(&mut next, phase, task)?;
            target.status = TodoStatus::InProgress;
            target.blocker = None;
        }
        TodoEdit::Complete { phase, task } => {
            let target = task_mut(&mut next, phase, task)?;
            target.status = TodoStatus::Completed;
            target.blocker = None;
        }
        TodoEdit::Drop { phase, task } => {
            let target = task_mut(&mut next, phase, task)?;
            target.status = TodoStatus::Abandoned;
            target.blocker = None;
        }
        TodoEdit::Block {
            phase,
            task,
            blocker,
        } => {
            let target = task_mut(&mut next, phase, task)?;
            if !matches!(
                target.status,
                TodoStatus::Pending | TodoStatus::InProgress | TodoStatus::Blocked
            ) {
                return Err("Only open tasks can be blocked.".to_owned());
            }
            target.status = TodoStatus::Blocked;
            target.blocker = blocker.and_then(|value| normalize_optional_text(&value));
        }
        TodoEdit::Unblock { phase, task } => {
            let target = task_mut(&mut next, phase, task)?;
            if target.status != TodoStatus::Blocked {
                return Err("Only blocked tasks can be unblocked.".to_owned());
            }
            target.status = TodoStatus::Pending;
            target.blocker = None;
        }
        TodoEdit::Remove { phase, task } => {
            ensure_task(&next, phase, task)?;
            next[phase].tasks.remove(task);
        }
        TodoEdit::Clear => next.clear(),
    }
    validate_phases(&next)?;
    Ok(next)
}

fn normalize_optional_text(value: &str) -> Option<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

fn ensure_task(phases: &[TodoPhase], phase: usize, task: usize) -> Result<(), String> {
    phases
        .get(phase)
        .and_then(|phase| phase.tasks.get(task))
        .map(|_| ())
        .ok_or_else(|| "That todo no longer exists. Refresh and try again.".to_owned())
}

fn task_mut(phases: &mut [TodoPhase], phase: usize, task: usize) -> Result<&mut TodoItem, String> {
    phases
        .get_mut(phase)
        .and_then(|phase| phase.tasks.get_mut(task))
        .ok_or_else(|| "That todo no longer exists. Refresh and try again.".to_owned())
}

type ActionHandler = Rc<RefCell<Option<Box<dyn Fn(TodoAction)>>>>;

#[derive(Clone)]
pub(crate) struct TodoPanel {
    pub(crate) root: gtk::Box,
    summary: gtk::ToggleButton,
    summary_context: gtk::Label,
    summary_current: gtk::Label,
    summary_count: gtk::Label,
    summary_progress: gtk::ProgressBar,
    summary_blocked: gtk::Label,
    summary_disclosure: gtk::Label,
    summary_description: Rc<RefCell<String>>,
    details: gtk::Revealer,
    list: gtk::Box,
    status: gtk::Label,
    phases: Rc<RefCell<Vec<TodoPhase>>>,
    action_handler: ActionHandler,
    action_controls: Rc<RefCell<Vec<gtk::Widget>>>,
    pending: Rc<Cell<bool>>,
}

impl TodoPanel {
    pub(crate) fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("todo-panel");

        let summary_context = gtk::Label::new(Some("PROGRESS"));
        summary_context.set_xalign(0.0);
        summary_context.add_css_class("todo-summary-context");
        let summary_current = gtk::Label::new(None);
        summary_current.set_xalign(0.0);
        summary_current.set_ellipsize(gtk::pango::EllipsizeMode::End);
        summary_current.set_hexpand(true);
        summary_current.add_css_class("todo-summary-current");
        let summary_copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
        summary_copy.set_hexpand(true);
        summary_copy.append(&summary_context);
        summary_copy.append(&summary_current);

        let summary_count = gtk::Label::new(None);
        summary_count.add_css_class("todo-summary-count");
        let summary_progress = gtk::ProgressBar::new();
        summary_progress.set_valign(gtk::Align::Center);
        summary_progress.add_css_class("todo-summary-progress");
        let summary_blocked = gtk::Label::new(None);
        summary_blocked.add_css_class("todo-summary-blocked");
        let summary_disclosure = gtk::Label::new(Some("Show"));
        summary_disclosure.add_css_class("todo-summary-disclosure");
        let summary_meta = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        summary_meta.append(&summary_count);
        summary_meta.append(&summary_progress);
        summary_meta.append(&summary_blocked);
        summary_meta.append(&summary_disclosure);

        let summary_content = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        summary_content.append(&summary_copy);
        summary_content.append(&summary_meta);
        let summary = gtk::ToggleButton::new();
        summary.set_child(Some(&summary_content));
        summary.add_css_class("todo-summary");
        summary.set_hexpand(true);
        summary.set_halign(gtk::Align::Fill);
        summary.set_tooltip_text(Some("Expand work plan"));

        let list = gtk::Box::new(gtk::Orientation::Vertical, 12);
        list.set_margin_top(12);
        list.set_margin_bottom(12);
        list.set_margin_start(12);
        list.set_margin_end(12);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .max_content_height(400)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        let details = gtk::Revealer::new();
        details.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        details.set_transition_duration(160);
        details.set_child(Some(&scroll));

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_visible(false);
        status.set_accessible_role(gtk::AccessibleRole::Alert);
        status.add_css_class("todo-panel-status");

        root.append(&summary);
        root.append(&details);
        root.append(&status);

        let summary_description = Rc::new(RefCell::new(String::new()));
        summary.connect_toggled({
            let details = details.clone();
            let disclosure = summary_disclosure.clone();
            let description = summary_description.clone();
            move |button| {
                let expanded = button.is_active();
                details.set_reveal_child(expanded);
                disclosure.set_text(if expanded { "Hide" } else { "Show" });
                let action = if expanded {
                    "Collapse work plan"
                } else {
                    "Expand work plan"
                };
                button.set_tooltip_text(Some(action));
                let accessible = format!("{action}. {}", description.borrow());
                button.update_property(&[gtk::accessible::Property::Label(&accessible)]);
            }
        });

        let panel = Self {
            root,
            summary,
            summary_context,
            summary_current,
            summary_count,
            summary_progress,
            summary_blocked,
            summary_disclosure,
            summary_description,
            details,
            list,
            status,
            phases: Rc::new(RefCell::new(Vec::new())),
            action_handler: Rc::new(RefCell::new(None)),
            action_controls: Rc::new(RefCell::new(Vec::new())),
            pending: Rc::new(Cell::new(false)),
        };
        panel.render();
        panel
    }

    pub(crate) fn connect_action(&self, handler: impl Fn(TodoAction) + 'static) {
        self.action_handler.replace(Some(Box::new(handler)));
    }

    pub(crate) fn set_phases(&self, phases: &[TodoPhase]) {
        self.phases.replace(phases.to_vec());
        self.render();
    }

    pub(crate) fn set_expanded(&self, expanded: bool) {
        self.summary.set_active(expanded);
        self.details.set_reveal_child(expanded);
        self.summary_disclosure
            .set_text(if expanded { "Hide" } else { "Show" });
    }

    pub(crate) fn set_pending(&self, pending: bool) {
        self.pending.set(pending);
        for control in self.action_controls.borrow().iter() {
            control.set_sensitive(!pending);
        }
        if pending {
            self.status.remove_css_class("error");
            self.status.set_text("Saving plan changes…");
            self.status.set_visible(true);
        } else if !self.status.has_css_class("error") {
            self.status.set_visible(false);
        }
    }

    pub(crate) fn set_error(&self, message: Option<&str>) {
        self.status.remove_css_class("error");
        if let Some(message) = message {
            self.status.add_css_class("error");
            self.status.set_text(message);
            self.status.set_visible(true);
        } else if !self.pending.get() {
            self.status.set_visible(false);
        }
    }

    fn render(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.action_controls.borrow_mut().clear();

        let phases = self.phases.borrow();
        let summary = summarize(&phases);
        self.root.set_visible(summary.total > 0);
        if summary.total == 0 {
            self.details.set_reveal_child(false);
            return;
        }

        let (context, current) = if let Some(active) = summary.active.as_deref() {
            ("CURRENT TASK", active)
        } else if summary.blocked > 0 {
            ("NEEDS ATTENTION", "Resolve blocked work to continue")
        } else if summary.completed + summary.abandoned == summary.total {
            ("PLAN COMPLETE", "All planned work is resolved")
        } else {
            ("UP NEXT", "Choose the next task from the work plan")
        };
        self.summary_context.set_text(context);
        self.summary_current.set_text(current);
        self.summary_count
            .set_text(&format!("{} of {} done", summary.completed, summary.total));
        self.summary_progress
            .set_fraction(summary.completed as f64 / summary.total as f64);
        self.summary_progress.set_tooltip_text(Some(&format!(
            "{} of {} tasks complete",
            summary.completed, summary.total
        )));
        self.summary_blocked.set_visible(summary.blocked > 0);
        self.summary_blocked
            .set_text(&format!("{} blocked", summary.blocked));
        self.summary_disclosure
            .set_text(if self.summary.is_active() {
                "Hide"
            } else {
                "Show"
            });

        let mut description = format!(
            "{current}. {} of {} tasks complete",
            summary.completed, summary.total
        );
        if summary.blocked > 0 {
            description.push_str(&format!(", {} blocked", summary.blocked));
        }
        if summary.abandoned > 0 {
            description.push_str(&format!(", {} abandoned", summary.abandoned));
        }
        description.push('.');
        self.summary_description.replace(description.clone());
        let action = if self.summary.is_active() {
            "Collapse work plan"
        } else {
            "Expand work plan"
        };
        let accessible = format!("{action}. {description}");
        self.summary
            .update_property(&[gtk::accessible::Property::Label(&accessible)]);

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let heading = gtk::Label::new(Some("Work plan"));
        heading.set_xalign(0.0);
        heading.set_hexpand(true);
        heading.add_css_class("todo-panel-heading");
        toolbar.append(&heading);
        toolbar.append(&self.action_button(
            "Add phase",
            "Add a phase to the work plan",
            TodoAction::AddPhase,
            false,
        ));
        toolbar.append(&self.action_button(
            "Clear",
            "Clear the entire work plan",
            TodoAction::Clear,
            true,
        ));
        self.list.append(&toolbar);

        for (phase_index, phase) in phases.iter().enumerate() {
            self.list.append(&self.phase_view(phase_index, phase));
        }
    }

    fn phase_view(&self, phase_index: usize, phase: &TodoPhase) -> gtk::Widget {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("todo-phase");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
        heading.set_hexpand(true);
        let index = gtk::Label::new(Some(&format!("PHASE {}", phase_index + 1)));
        index.set_xalign(0.0);
        index.add_css_class("todo-phase-index");
        let title = gtk::Label::new(Some(&phase.name));
        title.set_xalign(0.0);
        title.set_wrap(true);
        title.add_css_class("todo-phase-title");
        heading.append(&index);
        heading.append(&title);
        header.append(&heading);

        let completed = phase
            .tasks
            .iter()
            .filter(|task| task.status == TodoStatus::Completed)
            .count();
        let progress = gtk::Label::new(Some(&format!("{} / {}", completed, phase.tasks.len())));
        progress.set_tooltip_text(Some(&format!(
            "{} of {} phase tasks complete",
            completed,
            phase.tasks.len()
        )));
        progress.add_css_class("todo-phase-progress");
        header.append(&progress);
        header.append(&self.action_button(
            "Add task",
            &format!("Add a task to phase {}", phase.name),
            TodoAction::AddTask { phase: phase_index },
            false,
        ));
        root.append(&header);

        if phase.tasks.is_empty() {
            let empty = gtk::Label::new(Some("This phase has no tasks yet."));
            empty.set_xalign(0.0);
            empty.add_css_class("todo-empty");
            root.append(&empty);
        } else {
            for (task_index, task) in phase.tasks.iter().enumerate() {
                root.append(&self.task_view(phase_index, task_index, task));
            }
        }
        root.upcast()
    }

    fn task_view(&self, phase: usize, task_index: usize, task: &TodoItem) -> gtk::Widget {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("todo-task");
        row.add_css_class(task.status.css_class());

        let marker = gtk::Label::new(Some(task.status.marker()));
        marker.set_valign(gtk::Align::Start);
        marker.set_tooltip_text(Some(task.status.label()));
        marker.set_accessible_role(gtk::AccessibleRole::Presentation);
        marker.add_css_class("todo-task-marker");
        row.append(&marker);

        let copy = gtk::Box::new(gtk::Orientation::Vertical, 4);
        copy.set_hexpand(true);
        let content = gtk::Label::new(Some(&task.content));
        content.set_xalign(0.0);
        content.set_wrap(true);
        content.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        content.add_css_class("todo-task-content");
        copy.append(&content);
        let state = gtk::Label::new(Some(task.status.label()));
        state.set_xalign(0.0);
        state.add_css_class("todo-task-state");
        copy.append(&state);
        if let Some(blocker) = task.blocker.as_deref() {
            let blocker = gtk::Label::new(Some(&format!("Blocked by: {blocker}")));
            blocker.set_xalign(0.0);
            blocker.set_wrap(true);
            blocker.add_css_class("todo-blocker");
            copy.append(&blocker);
        }
        row.append(&copy);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.set_valign(gtk::Align::Start);
        let (primary_label, primary_action, secondary) = match task.status {
            TodoStatus::Pending => (
                "Start",
                TodoAction::Start {
                    phase,
                    task: task_index,
                },
                vec![
                    (
                        "Mark complete",
                        TodoAction::Complete {
                            phase,
                            task: task_index,
                        },
                        false,
                    ),
                    (
                        "Mark blocked",
                        TodoAction::Block {
                            phase,
                            task: task_index,
                        },
                        false,
                    ),
                    (
                        "Abandon",
                        TodoAction::Drop {
                            phase,
                            task: task_index,
                        },
                        false,
                    ),
                    (
                        "Remove",
                        TodoAction::Remove {
                            phase,
                            task: task_index,
                        },
                        true,
                    ),
                ],
            ),
            TodoStatus::InProgress => (
                "Complete",
                TodoAction::Complete {
                    phase,
                    task: task_index,
                },
                vec![
                    (
                        "Mark blocked",
                        TodoAction::Block {
                            phase,
                            task: task_index,
                        },
                        false,
                    ),
                    (
                        "Abandon",
                        TodoAction::Drop {
                            phase,
                            task: task_index,
                        },
                        false,
                    ),
                    (
                        "Remove",
                        TodoAction::Remove {
                            phase,
                            task: task_index,
                        },
                        true,
                    ),
                ],
            ),
            TodoStatus::Blocked => (
                "Unblock",
                TodoAction::Unblock {
                    phase,
                    task: task_index,
                },
                vec![
                    (
                        "Mark complete",
                        TodoAction::Complete {
                            phase,
                            task: task_index,
                        },
                        false,
                    ),
                    (
                        "Abandon",
                        TodoAction::Drop {
                            phase,
                            task: task_index,
                        },
                        false,
                    ),
                    (
                        "Remove",
                        TodoAction::Remove {
                            phase,
                            task: task_index,
                        },
                        true,
                    ),
                ],
            ),
            TodoStatus::Completed | TodoStatus::Abandoned => (
                "Reopen",
                TodoAction::Start {
                    phase,
                    task: task_index,
                },
                vec![(
                    "Remove",
                    TodoAction::Remove {
                        phase,
                        task: task_index,
                    },
                    true,
                )],
            ),
        };
        let primary = self.task_action_button(primary_label, &task.content, primary_action, false);
        primary.add_css_class("todo-primary-action");
        actions.append(&primary);
        actions.append(&self.task_action_menu(&task.content, secondary));
        row.append(&actions);
        row.upcast()
    }

    fn task_action_button(
        &self,
        label: &str,
        task: &str,
        action: TodoAction,
        destructive: bool,
    ) -> gtk::Button {
        self.action_button(label, &format!("{label} task: {task}"), action, destructive)
    }

    fn task_action_menu(
        &self,
        task: &str,
        actions: Vec<(&'static str, TodoAction, bool)>,
    ) -> gtk::MenuButton {
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
        menu.add_css_class("todo-actions-menu");
        let popover = gtk::Popover::builder()
            .has_arrow(true)
            .autohide(true)
            .child(&menu)
            .build();
        popover.add_css_class("todo-actions-popover");
        for (label, action, destructive) in actions {
            let button = self.task_action_button(label, task, action, destructive);
            button.add_css_class("todo-menu-action");
            let popover_for_click = popover.clone();
            button.connect_clicked(move |_| popover_for_click.popdown());
            menu.append(&button);
        }

        let button = gtk::MenuButton::new();
        button.set_label("More");
        button.set_popover(Some(&popover));
        button.set_tooltip_text(Some(&format!("More actions for {task}")));
        let accessible = format!("More actions for task: {task}");
        button.update_property(&[gtk::accessible::Property::Label(&accessible)]);
        button.add_css_class("todo-action");
        button.add_css_class("todo-more-action");
        button.set_sensitive(!self.pending.get());
        self.action_controls
            .borrow_mut()
            .push(button.clone().upcast());
        button
    }

    fn action_button(
        &self,
        label: &str,
        accessible_label: &str,
        action: TodoAction,
        destructive: bool,
    ) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        button.add_css_class("todo-action");
        if destructive {
            button.add_css_class("destructive-action");
        }
        button.set_tooltip_text(Some(accessible_label));
        button.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
        button.set_sensitive(!self.pending.get());
        let handler = self.action_handler.clone();
        button.connect_clicked(move |_| {
            if let Some(handler) = handler.borrow().as_ref() {
                handler(action.clone());
            }
        });
        self.action_controls
            .borrow_mut()
            .push(button.clone().upcast());
        button
    }
}

impl TodoStatus {
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
    use super::{TodoEdit, apply_edit, summarize, validate_phases};
    use crate::bridge::protocol::{TodoItem, TodoPhase, TodoStatus};

    fn phases() -> Vec<TodoPhase> {
        vec![
            TodoPhase {
                name: "Research".to_owned(),
                tasks: vec![
                    TodoItem {
                        content: "Inspect protocol".to_owned(),
                        status: TodoStatus::InProgress,
                        blocker: None,
                    },
                    TodoItem {
                        content: "Map UI".to_owned(),
                        status: TodoStatus::Pending,
                        blocker: None,
                    },
                ],
            },
            TodoPhase {
                name: "Ship".to_owned(),
                tasks: vec![TodoItem {
                    content: "Verify behavior".to_owned(),
                    status: TodoStatus::Blocked,
                    blocker: Some("Needs fixture".to_owned()),
                }],
            },
        ]
    }

    #[test]
    fn operations_preserve_order_and_move_the_active_task() {
        let initial = phases();
        let appended = apply_edit(
            &initial,
            TodoEdit::Append {
                phase: "Research".to_owned(),
                content: "Write model".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(
            appended[0]
                .tasks
                .iter()
                .map(|task| task.content.as_str())
                .collect::<Vec<_>>(),
            vec!["Inspect protocol", "Map UI", "Write model"]
        );
        assert_eq!(appended[1].name, "Ship");

        let started = apply_edit(&appended, TodoEdit::Start { phase: 0, task: 1 }).unwrap();
        assert_eq!(started[0].tasks[0].status, TodoStatus::Pending);
        assert_eq!(started[0].tasks[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn every_state_transition_and_removal_is_explicit() {
        let initial = phases();
        let completed = apply_edit(&initial, TodoEdit::Complete { phase: 0, task: 0 }).unwrap();
        assert_eq!(completed[0].tasks[0].status, TodoStatus::Completed);
        let dropped = apply_edit(&completed, TodoEdit::Drop { phase: 0, task: 1 }).unwrap();
        assert_eq!(dropped[0].tasks[1].status, TodoStatus::Abandoned);
        let blocked = apply_edit(
            &dropped,
            TodoEdit::Block {
                phase: 1,
                task: 0,
                blocker: Some("  Waiting\nfor owner  ".to_owned()),
            },
        )
        .unwrap();
        assert_eq!(blocked[1].tasks[0].status, TodoStatus::Blocked);
        assert_eq!(
            blocked[1].tasks[0].blocker.as_deref(),
            Some("Waiting for owner")
        );
        let unblocked = apply_edit(&blocked, TodoEdit::Unblock { phase: 1, task: 0 }).unwrap();
        assert_eq!(unblocked[1].tasks[0].status, TodoStatus::Pending);
        assert!(unblocked[1].tasks[0].blocker.is_none());
        let removed = apply_edit(&unblocked, TodoEdit::Remove { phase: 0, task: 1 }).unwrap();
        assert_eq!(removed[0].tasks.len(), 1);
        assert!(apply_edit(&removed, TodoEdit::Clear).unwrap().is_empty());
    }

    #[test]
    fn initialize_and_append_reject_invalid_state_without_mutating_confirmation() {
        let confirmed = phases();
        assert!(apply_edit(&confirmed, TodoEdit::Initialize(Vec::new())).is_err());
        assert_eq!(confirmed, phases());
        assert!(
            apply_edit(
                &confirmed,
                TodoEdit::Append {
                    phase: "Ship".to_owned(),
                    content: "Inspect protocol".to_owned(),
                }
            )
            .is_err()
        );
        assert_eq!(confirmed, phases());

        let initialized = apply_edit(
            &[],
            TodoEdit::Initialize(vec![TodoPhase {
                name: "Build".to_owned(),
                tasks: vec![TodoItem {
                    content: "Implement".to_owned(),
                    status: TodoStatus::Pending,
                    blocker: None,
                }],
            }]),
        )
        .unwrap();
        assert_eq!(initialized[0].name, "Build");
    }

    #[test]
    fn validation_rejects_globally_ambiguous_task_content() {
        let mut duplicate = phases();
        duplicate[1].tasks[0].content = "Map UI".to_owned();
        assert!(validate_phases(&duplicate).is_err());
    }

    #[test]
    fn summary_counts_terminal_and_blocked_states_and_finds_active_task() {
        let mut state = phases();
        state[0].tasks[1].status = TodoStatus::Completed;
        let summary = summarize(&state);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.blocked, 1);
        assert_eq!(summary.active.as_deref(), Some("Inspect protocol"));
    }
}
