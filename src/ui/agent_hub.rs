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
    detail_runtime: gtk::Label,
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
    let summary_icon = icons::icon(icons::Icon::Users, 18);
    summary_icon.add_css_class("agent-hub-summary-icon");
    summary.append(&summary_icon);
    let summary_copy = gtk::Box::new(gtk::Orientation::Vertical, 1);
    summary_copy.set_hexpand(true);
    let summary_title = gtk::Label::new(Some("Agent activity"));
    summary_title.set_xalign(0.0);
    summary_title.add_css_class("agent-hub-summary-title");
    let summary_subtitle = gtk::Label::new(Some("Live work and recent agent transcripts"));
    summary_subtitle.set_xalign(0.0);
    summary_subtitle.add_css_class("agent-hub-summary-subtitle");
    summary_copy.append(&summary_title);
    summary_copy.append(&summary_subtitle);
    let active_count = gtk::Label::new(Some("0 active"));
    active_count.add_css_class("agent-hub-active-count");
    let total_count = gtk::Label::new(Some("0 agents"));
    total_count.add_css_class("agent-hub-total-count");
    summary.append(&summary_copy);
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
    let placeholder_title = gtk::Label::new(Some("Choose an agent"));
    placeholder_title.add_css_class("agent-hub-empty-title");
    let placeholder_help = gtk::Label::new(Some(
        "Select an agent to review its current task, runtime activity, and transcript.",
    ));
    placeholder_help.set_wrap(true);
    placeholder_help.set_justify(gtk::Justification::Center);
    placeholder_help.set_max_width_chars(42);
    placeholder_help.add_css_class("agent-hub-empty-help");
    placeholder.append(&placeholder_title);
    placeholder.append(&placeholder_help);

    let detail = gtk::Box::new(gtk::Orientation::Vertical, 0);
    detail.add_css_class("agent-hub-detail");
    let detail_header = gtk::Box::new(gtk::Orientation::Vertical, 11);
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

    let context = gtk::Grid::new();
    context.set_column_spacing(18);
    context.set_row_spacing(9);
    context.set_column_homogeneous(true);
    context.add_css_class("agent-hub-detail-context");
    let (task_field, detail_task) = detail_field("CURRENT TASK", "agent-hub-detail-task");
    let (activity_field, detail_activity) =
        detail_field("LATEST ACTIVITY", "agent-hub-detail-activity");
    let (runtime_field, detail_runtime) = detail_field("RUNTIME", "agent-hub-detail-runtime");
    let (metrics_field, detail_metrics) = detail_field("USAGE", "agent-hub-detail-metrics");
    context.attach(&task_field, 0, 0, 1, 1);
    context.attach(&activity_field, 1, 0, 1, 1);
    context.attach(&runtime_field, 0, 1, 1, 1);
    context.attach(&metrics_field, 1, 1, 1, 1);
    detail_header.append(&heading);
    detail_header.append(&context);

    let transcript_heading = gtk::Label::new(Some("TRANSCRIPT"));
    transcript_heading.set_xalign(0.0);
    transcript_heading.add_css_class("agent-hub-transcript-heading");
    let transcript = ConversationView::transcript();
    detail.append(&detail_header);
    detail.append(&transcript_heading);
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
        detail_runtime,
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
        if active == 0 {
            self.active_count.add_css_class("inactive");
        } else {
            self.active_count.remove_css_class("inactive");
        }
        self.total_count
            .set_text(&format!("{total} {}", plural(total, "agent", "agents")));
        self.root
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Agent Hub, {active} active, {total} total"
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
        let task = agent.current_task().unwrap_or_else(|| {
            if agent.is_active() {
                "Waiting for task details"
            } else {
                "Task details were not reported"
            }
        });
        set_detail_value(&self.detail_task, task);
        set_detail_value(&self.detail_activity, &activity_summary(agent));
        set_detail_value(&self.detail_runtime, &runtime_summary(agent));
        set_detail_value(&self.detail_metrics, &usage_summary(agent));
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
        let branch = gtk::Label::new(Some("↳"));
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

    let task = gtk::Label::new(Some(agent.current_task().unwrap_or_else(|| {
        if agent.is_active() {
            "Waiting for task details"
        } else {
            "Task details were not reported"
        }
    })));
    task.set_xalign(0.0);
    task.set_ellipsize(gtk::pango::EllipsizeMode::End);
    task.add_css_class("agent-hub-agent-task");
    let metadata_text = row_metadata(agent);
    let metadata = gtk::Label::new(Some(&metadata_text));
    metadata.set_xalign(0.0);
    metadata.set_ellipsize(gtk::pango::EllipsizeMode::End);
    metadata.set_tooltip_text(Some(&metadata_text));
    metadata.add_css_class("agent-hub-agent-metadata");
    text.append(&heading);
    text.append(&task);
    text.append(&metadata);
    content.append(&text);
    root.set_child(Some(&content));

    let hierarchy = if row.parent_id.is_some() {
        format!(", nested agent at level {}", row.depth + 1)
    } else {
        String::new()
    };
    root.update_property(&[gtk::accessible::Property::Label(&format!(
        "{}, {}{}; task: {}; {}",
        agent.display_name(),
        status_label(&agent.status),
        hierarchy,
        agent.current_task().unwrap_or("not reported"),
        metadata_text
    ))]);

    AgentHubRow {
        root,
        id: agent.id.clone(),
    }
}

fn detail_field(caption: &str, css_class: &str) -> (gtk::Box, gtk::Label) {
    let field = gtk::Box::new(gtk::Orientation::Vertical, 2);
    field.add_css_class("agent-hub-detail-field");
    let caption = gtk::Label::new(Some(caption));
    caption.set_xalign(0.0);
    caption.add_css_class("agent-hub-detail-caption");
    let value = gtk::Label::new(None);
    value.set_xalign(0.0);
    value.set_ellipsize(gtk::pango::EllipsizeMode::End);
    value.add_css_class(css_class);
    field.append(&caption);
    field.append(&value);
    (field, value)
}

fn set_detail_value(label: &gtk::Label, value: &str) {
    label.set_text(value);
    label.set_tooltip_text(Some(value));
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
    let mut parts = vec![role_label(&agent.agent)];
    if let Some(activity) = agent.current_activity() {
        parts.push(activity);
    }
    if let Some(updated) = relative_update(agent.last_update) {
        parts.push(updated);
    }
    parts.join(" · ")
}

fn activity_summary(agent: &AgentRecord) -> String {
    if let Some(activity) = agent.current_activity() {
        activity
    } else if agent.status == "pending" {
        "Waiting to start".to_owned()
    } else if agent.is_terminal() {
        "No final activity was reported".to_owned()
    } else {
        "Working · activity details not reported".to_owned()
    }
}

fn runtime_summary(agent: &AgentRecord) -> String {
    let role = role_label(&agent.agent);
    let model = agent
        .progress
        .as_ref()
        .and_then(|progress| {
            progress
                .resolved_model
                .as_deref()
                .map(|model| {
                    if progress.resolved_model_is_fallback {
                        format!("{model} (fallback)")
                    } else {
                        model.to_owned()
                    }
                })
                .or_else(|| {
                    progress
                        .model_role
                        .as_deref()
                        .map(|model_role| format!("{model_role} model role"))
                })
        })
        .unwrap_or_else(|| "Model not reported".to_owned());
    format!("{role} · {model}")
}

fn usage_summary(agent: &AgentRecord) -> String {
    let mut parts = Vec::new();
    if let Some(progress) = agent.progress.as_ref() {
        if progress.tokens > 0 {
            parts.push(format!("{} tokens", compact_number(progress.tokens)));
        }
        if let Some(context) = progress.context_tokens {
            match progress.context_window {
                Some(window) if window > 0 => parts.push(format!(
                    "{} / {} context",
                    compact_number(context),
                    compact_number(window)
                )),
                _ => parts.push(format!("{} context", compact_number(context))),
            }
        }
        if progress.cost > 0.0 {
            parts.push(format!("${:.3}", progress.cost));
        }
        if progress.duration_ms > 0 {
            parts.push(format_duration(progress.duration_ms));
        }
        if progress.requests > 0 {
            parts.push(format!(
                "{} {}",
                progress.requests,
                plural(progress.requests as usize, "request", "requests")
            ));
        }
        if progress.tool_count > 0 {
            parts.push(format!(
                "{} {}",
                progress.tool_count,
                plural(progress.tool_count as usize, "tool", "tools")
            ));
        }
    }
    if parts.is_empty() {
        parts.push("Usage metrics not reported".to_owned());
    }
    if let Some(updated) = relative_update(agent.last_update) {
        parts.push(updated);
    }
    parts.join(" · ")
}

fn role_label(role: &str) -> String {
    match role.trim() {
        "" | "subagent" => "General-purpose agent".to_owned(),
        "task" => "Task agent".to_owned(),
        "scout" => "Scout".to_owned(),
        "reviewer" => "Reviewer".to_owned(),
        "librarian" => "Researcher".to_owned(),
        role => {
            let words = role.replace(|character: char| character == '-' || character == '_', " ");
            let mut chars = words.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => "Agent".to_owned(),
            }
        }
    }
}

fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
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
