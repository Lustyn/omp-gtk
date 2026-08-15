use std::time::{Duration, SystemTime, UNIX_EPOCH};

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use super::conversation::ConversationView;
use super::icons;
use crate::agent_hub::{AgentRecord, AgentTreeRow};

#[derive(Clone)]
pub(crate) struct AgentHubView {
    root: gtk::Box,
    roster: gtk::ListBox,
    active_count: gtk::Label,
    total_count: gtk::Label,
    detail_stack: gtk::Stack,
    detail_name: gtk::Label,
    detail_status: gtk::Label,
    detail_task: gtk::Label,
    detail_activity: gtk::Label,
    detail_metrics: gtk::Label,
    pub(crate) transcript: ConversationView,
}

#[derive(Clone)]
pub(crate) struct AgentHubRow {
    pub(crate) root: gtk::ListBoxRow,
    pub(crate) id: String,
}

pub(crate) fn build() -> AgentHubView {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("agent-hub");
    root.update_property(&[gtk::accessible::Property::Label("Runtime agent hub")]);

    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    summary.add_css_class("agent-hub-summary");
    summary.append(&icons::icon(icons::Icon::Users, 18));
    let summary_title = gtk::Label::new(Some("Runtime agents"));
    summary_title.set_hexpand(true);
    summary_title.set_xalign(0.0);
    summary_title.add_css_class("agent-hub-summary-title");
    let active_count = gtk::Label::new(Some("0 active"));
    active_count.add_css_class("agent-hub-active-count");
    let total_count = gtk::Label::new(Some("0 total"));
    total_count.add_css_class("agent-hub-total-count");
    summary.append(&summary_title);
    summary.append(&active_count);
    summary.append(&total_count);

    let roster = gtk::ListBox::new();
    roster.set_selection_mode(gtk::SelectionMode::Single);
    roster.add_css_class("agent-hub-roster");
    roster.update_property(&[gtk::accessible::Property::Label("Agent roster")]);
    let empty = gtk::Box::new(gtk::Orientation::Vertical, 8);
    empty.set_valign(gtk::Align::Center);
    empty.set_halign(gtk::Align::Center);
    empty.set_margin_top(36);
    empty.set_margin_bottom(36);
    empty.append(&icons::icon(icons::Icon::Users, 28));
    let empty_title = gtk::Label::new(Some("No runtime agents"));
    empty_title.add_css_class("agent-hub-empty-title");
    let empty_help = gtk::Label::new(Some(
        "Agents spawned by omp will appear here while this conversation is open.",
    ));
    empty_help.set_wrap(true);
    empty_help.set_justify(gtk::Justification::Center);
    empty_help.set_max_width_chars(34);
    empty_help.add_css_class("agent-hub-empty-help");
    empty.append(&empty_title);
    empty.append(&empty_help);
    roster.set_placeholder(Some(&empty));
    let roster_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&roster)
        .build();
    roster_scroll.set_size_request(350, -1);
    roster_scroll.add_css_class("agent-hub-roster-scroll");

    let placeholder = gtk::Box::new(gtk::Orientation::Vertical, 8);
    placeholder.set_valign(gtk::Align::Center);
    placeholder.set_halign(gtk::Align::Center);
    placeholder.append(&icons::icon(icons::Icon::MessageSquare, 28));
    let placeholder_title = gtk::Label::new(Some("Select an agent"));
    placeholder_title.add_css_class("agent-hub-empty-title");
    let placeholder_help = gtk::Label::new(Some(
        "Choose a roster row to inspect its authoritative live transcript.",
    ));
    placeholder_help.add_css_class("agent-hub-empty-help");
    placeholder.append(&placeholder_title);
    placeholder.append(&placeholder_help);

    let detail = gtk::Box::new(gtk::Orientation::Vertical, 0);
    detail.add_css_class("agent-hub-detail");
    let detail_header = gtk::Box::new(gtk::Orientation::Vertical, 5);
    detail_header.add_css_class("agent-hub-detail-header");
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let detail_name = gtk::Label::new(None);
    detail_name.set_xalign(0.0);
    detail_name.set_hexpand(true);
    detail_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    detail_name.add_css_class("agent-hub-detail-name");
    let detail_status = gtk::Label::new(None);
    detail_status.add_css_class("agent-hub-detail-status");
    heading.append(&detail_name);
    heading.append(&detail_status);
    let detail_task = detail_label("agent-hub-detail-task");
    let detail_activity = detail_label("agent-hub-detail-activity");
    let detail_metrics = detail_label("agent-hub-detail-metrics");
    detail_header.append(&heading);
    detail_header.append(&detail_task);
    detail_header.append(&detail_activity);
    detail_header.append(&detail_metrics);
    let transcript = ConversationView::transcript();
    detail.append(&detail_header);
    detail.append(transcript.widget());

    let detail_stack = gtk::Stack::new();
    detail_stack.set_hexpand(true);
    detail_stack.set_vexpand(true);
    detail_stack.add_named(&placeholder, Some("placeholder"));
    detail_stack.add_named(&detail, Some("transcript"));
    detail_stack.set_visible_child_name("placeholder");

    let split = gtk::Paned::new(gtk::Orientation::Horizontal);
    split.set_start_child(Some(&roster_scroll));
    split.set_end_child(Some(&detail_stack));
    split.set_position(350);
    split.set_resize_start_child(false);
    split.set_shrink_start_child(false);
    split.set_vexpand(true);

    root.append(&summary);
    root.append(&split);

    AgentHubView {
        root,
        roster,
        active_count,
        total_count,
        detail_stack,
        detail_name,
        detail_status,
        detail_task,
        detail_activity,
        detail_metrics,
        transcript,
    }
}

impl AgentHubView {
    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn clear_rows(&self) {
        while let Some(child) = self.roster.first_child() {
            self.roster.remove(&child);
        }
    }

    pub(crate) fn append_row(&self, row: &AgentHubRow) {
        self.roster.append(&row.root);
    }

    pub(crate) fn set_counts(&self, active: usize, total: usize) {
        self.active_count.set_text(&format!("{active} active"));
        self.total_count.set_text(&format!("{total} total"));
        self.root.update_property(&[gtk::accessible::Property::Label(&format!(
            "Runtime agent hub, {active} active, {total} total"
        ))]);
    }

    pub(crate) fn show_placeholder(&self) {
        self.detail_stack.set_visible_child_name("placeholder");
        self.roster.unselect_all();
    }

    pub(crate) fn show_agent(&self, agent: &AgentRecord) {
        let name = agent.display_name();
        self.detail_name.set_text(&name);
        self.detail_status.set_text(status_label(&agent.status));
        self.detail_status
            .set_css_classes(&["agent-hub-detail-status", status_class(&agent.status)]);
        self.detail_task.set_text(
            agent
                .current_task()
                .map(|task| format!("Task · {task}"))
                .as_deref()
                .unwrap_or("No task metadata reported"),
        );
        self.detail_activity.set_text(
            agent
                .current_activity()
                .map(|activity| format!("Activity · {activity}"))
                .as_deref()
                .unwrap_or("No current activity reported"),
        );
        self.detail_metrics.set_text(&detail_metrics(agent));
        self.detail_stack.set_visible_child_name("transcript");
    }

    pub(crate) fn select_id(&self, id: &str, rows: &[AgentHubRow]) {
        if let Some(row) = rows.iter().find(|row| row.id == id) {
            self.roster.select_row(Some(&row.root));
        }
    }
}

pub(crate) fn agent_row(row: &AgentTreeRow) -> AgentHubRow {
    let agent = &row.agent;
    let root = gtk::ListBoxRow::new();
    root.set_activatable(true);
    root.set_selectable(true);
    root.add_css_class("agent-hub-row");

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(12 + i32::try_from(row.depth.min(8)).unwrap_or(8) * 20);
    content.set_margin_end(10);
    if row.depth > 0 {
        let branch = gtk::Label::new(Some("└"));
        branch.set_accessible_role(gtk::AccessibleRole::Presentation);
        branch.add_css_class("agent-hub-tree-branch");
        content.append(&branch);
    }
    let status = gtk::Label::new(Some("●"));
    status.set_accessible_role(gtk::AccessibleRole::Presentation);
    status.add_css_class("agent-hub-status-dot");
    status.add_css_class(status_class(&agent.status));
    content.append(&status);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    let name = gtk::Label::new(Some(&agent.display_name()));
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("agent-hub-agent-name");
    let status = gtk::Label::new(Some(status_label(&agent.status)));
    status.add_css_class("agent-hub-agent-status");
    status.add_css_class(status_class(&agent.status));
    heading.append(&name);
    heading.append(&status);

    let task = gtk::Label::new(Some(agent.current_task().unwrap_or("No task metadata reported")));
    task.set_xalign(0.0);
    task.set_ellipsize(gtk::pango::EllipsizeMode::End);
    task.add_css_class("agent-hub-agent-task");
    let metadata = gtk::Label::new(Some(&row_metadata(agent)));
    metadata.set_xalign(0.0);
    metadata.set_ellipsize(gtk::pango::EllipsizeMode::End);
    metadata.add_css_class("agent-hub-agent-metadata");
    text.append(&heading);
    text.append(&task);
    text.append(&metadata);
    content.append(&text);
    root.set_child(Some(&content));

    let parent = row
        .parent_id
        .as_deref()
        .map(|parent| format!(", child of {parent}"))
        .unwrap_or_default();
    let activity = agent
        .current_activity()
        .map(|activity| format!(", {activity}"))
        .unwrap_or_default();
    root.update_property(&[gtk::accessible::Property::Label(&format!(
        "Agent {}, id {}, {}{}, task: {}{}",
        agent.display_name(),
        agent.id,
        status_label(&agent.status),
        parent,
        agent.current_task().unwrap_or("not reported"),
        activity
    ))]);

    AgentHubRow {
        root,
        id: agent.id.clone(),
    }
}

fn detail_label(css_class: &str) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.add_css_class(css_class);
    label
}

fn status_label(status: &str) -> &str {
    match status {
        "pending" => "Pending",
        "running" | "started" => "Running",
        "completed" => "Completed",
        "failed" => "Failed",
        "aborted" => "Aborted",
        _ => "Unknown",
    }
}

fn status_class(status: &str) -> &'static str {
    match status {
        "pending" => "agent-status-pending",
        "running" | "started" => "agent-status-running",
        "completed" => "agent-status-completed",
        "failed" => "agent-status-failed",
        "aborted" => "agent-status-aborted",
        _ => "agent-status-unknown",
    }
}

fn row_metadata(agent: &AgentRecord) -> String {
    let mut parts = vec![agent.agent.clone()];
    if let Some(source) = agent.agent_source.as_deref() {
        parts.push(source.to_owned());
    }
    if let Some(parent_tool_call_id) = agent.parent_tool_call_id.as_deref() {
        parts.push(format!("task call {parent_tool_call_id}"));
    }
    if let Some(activity) = agent.current_activity() {
        parts.push(activity);
    } else if let Some(updated) = relative_update(agent.last_update) {
        parts.push(updated);
    }
    parts.join(" · ")
}

fn detail_metrics(agent: &AgentRecord) -> String {
    let mut parts = Vec::new();
    parts.push(format!("ID {}", agent.id));
    if let Some(parent_tool_call_id) = agent.parent_tool_call_id.as_deref() {
        parts.push(format!("Parent task call {parent_tool_call_id}"));
    }
    if let Some(progress) = agent.progress.as_ref() {
        if let Some(model) = progress.resolved_model.as_deref() {
            let suffix = if progress.resolved_model_is_fallback {
                " (fallback)"
            } else {
                ""
            };
            parts.push(format!("Model {model}{suffix}"));
        } else if let Some(role) = progress.model_role.as_deref() {
            parts.push(format!("Role {role}"));
        }
        if progress.tokens > 0 {
            parts.push(format!("{} tokens", compact_number(progress.tokens)));
        }
        if let Some(context) = progress.context_tokens {
            match progress.context_window {
                Some(window) if window > 0 => parts.push(format!(
                    "context {}/{}",
                    compact_number(context),
                    compact_number(window)
                )),
                _ => parts.push(format!("context {}", compact_number(context))),
            }
        }
        if progress.cost > 0.0 {
            parts.push(format!("${:.3}", progress.cost));
        }
        if progress.duration_ms > 0 {
            parts.push(format_duration(progress.duration_ms));
        }
        if progress.requests > 0 {
            parts.push(format!("{} requests", progress.requests));
        }
        if progress.tool_count > 0 {
            parts.push(format!("{} tools", progress.tool_count));
        }
    }
    if let Some(updated) = relative_update(agent.last_update) {
        parts.push(updated);
    }
    if parts.is_empty() {
        "No model or usage metrics reported".to_owned()
    } else {
        parts.join(" · ")
    }
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_duration(duration_ms: u64) -> String {
    let seconds = duration_ms / 1_000;
    if seconds >= 3_600 {
        format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn relative_update(timestamp_ms: u64) -> Option<String> {
    if timestamp_ms == 0 {
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;
    let elapsed = now.saturating_sub(timestamp_ms) / 1_000;
    Some(if elapsed < 60 {
        "updated just now".to_owned()
    } else if elapsed < 3_600 {
        format!("updated {}m ago", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("updated {}h ago", elapsed / 3_600)
    } else {
        format!("updated {}d ago", elapsed / 86_400)
    })
}
