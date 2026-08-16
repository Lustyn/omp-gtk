use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk::prelude::*;
use gtk4 as gtk;

use super::icons;
use crate::agent_hub::{AgentRecord, AgentTreeRow};

#[derive(Clone)]
pub(crate) struct AgentHubView {
    root: gtk::Box,
    roster: gtk::ListBox,
    launcher: gtk::ToggleButton,
    launcher_count: gtk::Label,
    active_count: gtk::Label,
    total_count: gtk::Label,
}

#[derive(Clone)]
pub(crate) struct AgentHubRow {
    pub(crate) root: gtk::ListBoxRow,
    pub(crate) open: gtk::Button,
    pub(crate) expander: Option<gtk::ToggleButton>,
    pub(crate) id: String,
}

pub(crate) fn build() -> AgentHubView {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    root.set_halign(gtk::Align::End);
    root.add_css_class("agent-hub-surface");
    root.set_visible(false);

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.set_size_request(360, -1);
    panel.add_css_class("agent-hub-panel");
    panel.update_property(&[gtk::accessible::Property::Label("Runtime agent hub")]);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    header.add_css_class("agent-hub-header");
    let header_icon = icons::icon(icons::Icon::Network, 16);
    header_icon.add_css_class("agent-hub-header-icon");
    header.append(&header_icon);

    let header_copy = gtk::Box::new(gtk::Orientation::Vertical, 1);
    header_copy.set_hexpand(true);
    let title = gtk::Label::new(Some("Agent hub"));
    title.set_xalign(0.0);
    title.add_css_class("agent-hub-title");
    let subtitle = gtk::Label::new(Some("Select a session to open its transcript"));
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    subtitle.add_css_class("agent-hub-subtitle");
    header_copy.append(&title);
    header_copy.append(&subtitle);
    header.append(&header_copy);

    let counts = gtk::Box::new(gtk::Orientation::Vertical, 1);
    let active_count = gtk::Label::new(Some("0 running"));
    active_count.add_css_class("agent-hub-active-count");
    let total_count = gtk::Label::new(Some("0 total"));
    total_count.add_css_class("agent-hub-total-count");
    counts.append(&active_count);
    counts.append(&total_count);
    header.append(&counts);
    panel.append(&header);

    let roster = gtk::ListBox::new();
    roster.set_selection_mode(gtk::SelectionMode::Single);
    roster.set_activate_on_single_click(true);
    roster.add_css_class("agent-hub-roster");
    roster.update_property(&[gtk::accessible::Property::Label("Agent sessions")]);

    let empty = gtk::Box::new(gtk::Orientation::Vertical, 7);
    empty.set_valign(gtk::Align::Center);
    empty.set_halign(gtk::Align::Center);
    empty.set_margin_top(30);
    empty.set_margin_bottom(30);
    empty.append(&icons::icon(icons::Icon::Users, 25));
    let empty_title = gtk::Label::new(Some("No agent sessions"));
    empty_title.add_css_class("agent-hub-empty-title");
    let empty_help = gtk::Label::new(Some(
        "Agents spawned by this conversation will appear here.",
    ));
    empty_help.set_wrap(true);
    empty_help.set_justify(gtk::Justification::Center);
    empty_help.set_max_width_chars(32);
    empty_help.add_css_class("agent-hub-empty-help");
    empty.append(&empty_title);
    empty.append(&empty_help);
    roster.set_placeholder(Some(&empty));

    let roster_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .max_content_height(430)
        .propagate_natural_height(true)
        .child(&roster)
        .build();
    roster_scroll.add_css_class("agent-hub-roster-scroll");
    panel.append(&roster_scroll);

    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideLeft);
    revealer.set_transition_duration(180);
    revealer.set_child(Some(&panel));
    root.append(&revealer);

    let launcher_content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    launcher_content.append(&icons::icon(icons::Icon::Users, 15));
    let launcher_count = gtk::Label::new(Some("0 running"));
    launcher_count.add_css_class("agent-hub-launcher-count");
    launcher_content.append(&launcher_count);
    let launcher_icon = gtk::Stack::new();
    launcher_icon.add_named(&icons::icon(icons::Icon::ChevronLeft, 12), Some("closed"));
    launcher_icon.add_named(&icons::icon(icons::Icon::ChevronRight, 12), Some("open"));
    launcher_icon.set_visible_child_name("closed");
    launcher_content.append(&launcher_icon);

    let launcher = gtk::ToggleButton::new();
    launcher.set_valign(gtk::Align::End);
    launcher.set_child(Some(&launcher_content));
    launcher.set_tooltip_text(Some("Open Agent Hub"));
    launcher.add_css_class("agent-hub-launcher");
    root.append(&launcher);

    let revealer_for_toggle = revealer.clone();
    let icon_for_toggle = launcher_icon.clone();
    launcher.connect_toggled(move |button| {
        revealer_for_toggle.set_reveal_child(button.is_active());
        icon_for_toggle.set_visible_child_name(if button.is_active() { "open" } else { "closed" });
    });

    AgentHubView {
        root,
        roster,
        launcher,
        launcher_count,
        active_count,
        total_count,
    }
}

impl AgentHubView {
    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn launcher(&self) -> gtk::ToggleButton {
        self.launcher.clone()
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
        let running = format!("{active} running");
        self.launcher_count.set_text(&running);
        self.active_count.set_text(&running);
        self.total_count.set_text(&format!("{total} total"));
        self.root.set_visible(total > 0);
        if active > 0 {
            self.launcher.add_css_class("active");
            self.active_count.remove_css_class("inactive");
        } else {
            self.launcher.remove_css_class("active");
            self.active_count.add_css_class("inactive");
        }
        if total == 0 {
            self.launcher.set_active(false);
        }
        let agent_word = plural(total, "agent", "agents");
        let label = format!("Agent Hub, {active} running, {total} {agent_word} total");
        self.launcher.set_tooltip_text(Some(&label));
        self.launcher
            .update_property(&[gtk::accessible::Property::Label(&label)]);
        self.root
            .update_property(&[gtk::accessible::Property::Label(&label)]);
    }

    pub(crate) fn set_revealed(&self, revealed: bool) {
        self.launcher.set_active(revealed);
    }

    pub(crate) fn unselect_all(&self) {
        self.roster.unselect_all();
    }

    pub(crate) fn select_id(&self, id: &str, rows: &[AgentHubRow]) {
        if let Some(row) = rows.iter().find(|row| row.id == id) {
            self.roster.select_row(Some(&row.root));
        }
    }
}

pub(crate) fn agent_row(row: &AgentTreeRow, expanded: bool) -> AgentHubRow {
    let agent = &row.agent;
    let root = gtk::ListBoxRow::new();
    root.set_activatable(false);
    root.set_selectable(true);
    root.add_css_class("agent-hub-row");
    if row.depth == 0 {
        root.add_css_class("agent-hub-session-row");
    }

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    content.set_margin_start(6 + i32::try_from(row.depth.min(8)).unwrap_or(8) * 17);
    content.set_margin_end(6);

    let expander = row.has_children.then(|| {
        let icons = gtk::Stack::new();
        icons.add_named(
            &super::icons::icon(icons::Icon::ChevronRight, 12),
            Some("collapsed"),
        );
        icons.add_named(
            &super::icons::icon(icons::Icon::ChevronDown, 12),
            Some("expanded"),
        );
        icons.set_visible_child_name(if expanded { "expanded" } else { "collapsed" });
        let button = gtk::ToggleButton::new();
        button.set_child(Some(&icons));
        button.set_active(expanded);
        button.add_css_class("agent-hub-expander");
        let name = agent.display_name();
        button.set_tooltip_text(Some(if expanded {
            "Collapse spawned agents"
        } else {
            "Expand spawned agents"
        }));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "{} agents spawned by {name}",
            if expanded { "Collapse" } else { "Expand" }
        ))]);
        let icons_for_toggle = icons.clone();
        button.connect_toggled(move |button| {
            icons_for_toggle.set_visible_child_name(if button.is_active() {
                "expanded"
            } else {
                "collapsed"
            });
            button.set_tooltip_text(Some(if button.is_active() {
                "Collapse spawned agents"
            } else {
                "Expand spawned agents"
            }));
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "{} agents spawned by {name}",
                if button.is_active() {
                    "Collapse"
                } else {
                    "Expand"
                }
            ))]);
        });
        button
    });
    if let Some(expander) = &expander {
        content.append(expander);
    } else {
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_size_request(26, -1);
        content.append(&spacer);
    }

    let open = gtk::Button::new();
    open.set_hexpand(true);
    open.add_css_class("agent-hub-row-action");
    let action_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let status_dot = gtk::Label::new(Some("●"));
    status_dot.set_accessible_role(gtk::AccessibleRole::Presentation);
    status_dot.add_css_class("agent-hub-status-dot");
    status_dot.add_css_class(status_class(&agent.status));
    action_content.append(&status_dot);

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
    action_content.append(&text);
    open.set_child(Some(&action_content));
    content.append(&open);
    root.set_child(Some(&content));

    let hierarchy = if row.parent_id.is_some() {
        format!(", nested agent at level {}", row.depth + 1)
    } else {
        ", top-level agent session".to_owned()
    };
    open.update_property(&[gtk::accessible::Property::Label(&format!(
        "Open {} session, {}{}; task: {}; {}",
        agent.display_name(),
        status_label(&agent.status),
        hierarchy,
        agent.current_task().unwrap_or("not reported"),
        metadata_text
    ))]);

    AgentHubRow {
        root,
        open,
        expander,
        id: agent.id.clone(),
    }
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
    if agent.historical {
        parts.push("Saved session".to_owned());
    }
    if let Some(activity) = agent.current_activity() {
        parts.push(activity);
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
            let words = role.replace(['-', '_'], " ");
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
