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
    AddTask {
        phase: usize,
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
            return Err(format!("A todo phase named “{}” already exists.", phase.name));
        }
        for task in &phase.tasks {
            if task.content.trim().is_empty() {
                return Err("A todo task cannot be empty.".to_owned());
            }
            if !task_contents.insert(task.content.as_str()) {
                return Err(format!("A todo task named “{}” already exists.", task.content));
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

fn task_mut(
    phases: &mut [TodoPhase],
    phase: usize,
    task: usize,
) -> Result<&mut TodoItem, String> {
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
    details: gtk::Revealer,
    list: gtk::Box,
    status: gtk::Label,
    phases: Rc<RefCell<Vec<TodoPhase>>>,
    action_handler: ActionHandler,
    action_buttons: Rc<RefCell<Vec<gtk::Button>>>,
    pending: Rc<Cell<bool>>,
}

impl TodoPanel {
    pub(crate) fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("todo-panel");

        let summary = gtk::ToggleButton::with_label("Todos · No tasks");
        summary.add_css_class("todo-summary");
        summary.set_hexpand(true);
        summary.set_halign(gtk::Align::Fill);
        summary.set_tooltip_text(Some("Expand todo details"));

        let list = gtk::Box::new(gtk::Orientation::Vertical, 10);
        list.set_margin_top(10);
        list.set_margin_bottom(10);
        list.set_margin_start(12);
        list.set_margin_end(12);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .max_content_height(320)
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

        summary.connect_toggled({
            let details = details.clone();
            move |button| {
                let expanded = button.is_active();
                details.set_reveal_child(expanded);
                button.set_tooltip_text(Some(if expanded {
                    "Collapse todo details"
                } else {
                    "Expand todo details"
                }));
            }
        });

        let panel = Self {
            root,
            summary,
            details,
            list,
            status,
            phases: Rc::new(RefCell::new(Vec::new())),
            action_handler: Rc::new(RefCell::new(None)),
            action_buttons: Rc::new(RefCell::new(Vec::new())),
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
    }

    pub(crate) fn set_pending(&self, pending: bool) {
        self.pending.set(pending);
        for button in self.action_buttons.borrow().iter() {
            button.set_sensitive(!pending);
        }
        if pending {
            self.status.remove_css_class("error");
            self.status.set_text("Saving todo changes…");
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
        self.action_buttons.borrow_mut().clear();

        let phases = self.phases.borrow();
        let summary = summarize(&phases);
        let mut summary_text = if summary.total == 0 {
            "Todos · No tasks".to_owned()
        } else {
            format!(
                "Todos · {} of {} complete",
                summary.completed, summary.total
            )
        };
        if summary.abandoned > 0 {
            summary_text.push_str(&format!(" · {} abandoned", summary.abandoned));
        }
        if summary.blocked > 0 {
            summary_text.push_str(&format!(" · {} blocked", summary.blocked));
        }
        if let Some(active) = summary.active.as_deref() {
            summary_text.push_str(&format!(" · Active: {active}"));
        }
        self.summary.set_label(&summary_text);
        self.summary.set_tooltip_text(Some(&format!(
            "{}. {}",
            summary_text,
            if self.summary.is_active() {
                "Collapse todo details"
            } else {
                "Expand todo details"
            }
        )));

        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let heading = gtk::Label::new(Some("Ordered todo phases"));
        heading.set_xalign(0.0);
        heading.set_hexpand(true);
        heading.add_css_class("todo-panel-heading");
        toolbar.append(&heading);
        toolbar.append(&self.action_button("Add phase", TodoAction::AddPhase, false));
        let clear = self.action_button("Clear", TodoAction::Clear, true);
        clear.set_sensitive(!phases.is_empty() && !self.pending.get());
        toolbar.append(&clear);
        self.list.append(&toolbar);

        if phases.is_empty() {
            let empty = gtk::Label::new(Some(
                "No todos yet. Add a phase and its first task to initialize the list.",
            ));
            empty.set_xalign(0.0);
            empty.set_wrap(true);
            empty.add_css_class("todo-empty");
            self.list.append(&empty);
            return;
        }

        for (phase_index, phase) in phases.iter().enumerate() {
            self.list.append(&self.phase_view(phase_index, phase));
        }
    }

    fn phase_view(&self, phase_index: usize, phase: &TodoPhase) -> gtk::Widget {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 7);
        root.add_css_class("todo-phase");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::new(Some(&format!("{}. {}", phase_index + 1, phase.name)));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_wrap(true);
        title.add_css_class("todo-phase-title");
        header.append(&title);
        header.append(&self.action_button(
            "Add task",
            TodoAction::AddTask { phase: phase_index },
            false,
        ));
        root.append(&header);

        if phase.tasks.is_empty() {
            let empty = gtk::Label::new(Some("No tasks in this phase."));
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
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.add_css_class("todo-task");
        row.add_css_class(task.status.css_class());

        let marker = gtk::Label::new(Some(task.status.marker()));
        marker.set_valign(gtk::Align::Start);
        marker.set_tooltip_text(Some(task.status.label()));
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
        match task.status {
            TodoStatus::Pending => {
                actions.append(&self.action_button(
                    "Start",
                    TodoAction::Start {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
                actions.append(&self.action_button(
                    "Complete",
                    TodoAction::Complete {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
                actions.append(&self.action_button(
                    "Block",
                    TodoAction::Block {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
                actions.append(&self.action_button(
                    "Drop",
                    TodoAction::Drop {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
            }
            TodoStatus::InProgress => {
                actions.append(&self.action_button(
                    "Complete",
                    TodoAction::Complete {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
                actions.append(&self.action_button(
                    "Block",
                    TodoAction::Block {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
                actions.append(&self.action_button(
                    "Drop",
                    TodoAction::Drop {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
            }
            TodoStatus::Blocked => {
                actions.append(&self.action_button(
                    "Unblock",
                    TodoAction::Unblock {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
                actions.append(&self.action_button(
                    "Complete",
                    TodoAction::Complete {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
                actions.append(&self.action_button(
                    "Drop",
                    TodoAction::Drop {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
            }
            TodoStatus::Completed | TodoStatus::Abandoned => {
                actions.append(&self.action_button(
                    "Start",
                    TodoAction::Start {
                        phase,
                        task: task_index,
                    },
                    false,
                ));
            }
        }
        actions.append(&self.action_button(
            "Remove",
            TodoAction::Remove {
                phase,
                task: task_index,
            },
            true,
        ));
        row.append(&actions);
        row.upcast()
    }

    fn action_button(&self, label: &str, action: TodoAction, destructive: bool) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        button.add_css_class("todo-action");
        if destructive {
            button.add_css_class("destructive-action");
        }
        button.set_tooltip_text(Some(label));
        button.set_sensitive(!self.pending.get());
        let handler = self.action_handler.clone();
        button.connect_clicked(move |_| {
            if let Some(handler) = handler.borrow().as_ref() {
                handler(action.clone());
            }
        });
        self.action_buttons.borrow_mut().push(button.clone());
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
        assert_eq!(blocked[1].tasks[0].blocker.as_deref(), Some("Waiting for owner"));
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
