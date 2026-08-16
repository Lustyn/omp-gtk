use std::collections::{HashMap, HashSet};

use crate::bridge::protocol::{
    SubagentProgress, SubagentSnapshot, SubagentUpdate, SubagentUpdateKind,
};

#[derive(Debug, Clone)]
pub(crate) struct AgentRecord {
    pub(crate) id: String,
    pub(crate) index: usize,
    pub(crate) agent: String,
    pub(crate) agent_source: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) status: String,
    pub(crate) task: Option<String>,
    pub(crate) assignment: Option<String>,
    pub(crate) session_file: Option<String>,
    pub(crate) parent_tool_call_id: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) historical: bool,
    pub(crate) last_update: u64,
    pub(crate) progress: Option<SubagentProgress>,
    order: usize,
}

impl AgentRecord {
    pub(crate) fn is_active(&self) -> bool {
        matches!(self.status.as_str(), "pending" | "running" | "started")
    }

    pub(crate) fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "aborted")
    }

    pub(crate) fn display_name(&self) -> String {
        if looks_like_uuid(&self.id) {
            self.description
                .clone()
                .or_else(|| self.task.clone())
                .unwrap_or_else(|| friendly_agent_name(&self.agent))
        } else {
            friendly_agent_name(&self.id)
        }
    }

    pub(crate) fn current_task(&self) -> Option<&str> {
        nonempty(self.task.as_deref())
            .or_else(|| nonempty(self.assignment.as_deref()))
            .or_else(|| nonempty(self.description.as_deref()))
    }

    pub(crate) fn current_activity(&self) -> Option<String> {
        let progress = self.progress.as_ref()?;
        if let Some(retry) = progress.retry_state.as_ref() {
            return Some(format!(
                "Retrying · attempt {} of {} · {}",
                retry.attempt, retry.max_attempts, retry.error_message
            ));
        }
        if let Some(failure) = progress.retry_failure.as_ref() {
            return Some(format!("Retry failed · {}", failure.error_message));
        }
        if let Some(tool) = nonempty(progress.current_tool.as_deref()) {
            return Some(match nonempty(progress.last_intent.as_deref()) {
                Some(intent) => format!("{intent} · using {tool}"),
                None => format!("Using {tool}"),
            });
        }
        nonempty(progress.last_intent.as_deref()).map(ToOwned::to_owned)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentTreeRow {
    pub(crate) agent: AgentRecord,
    pub(crate) depth: usize,
    pub(crate) parent_id: Option<String>,
    pub(crate) has_children: bool,
}

#[derive(Debug, Default)]
pub(crate) struct AgentHubState {
    agents: HashMap<String, AgentRecord>,
    parent_by_id: HashMap<String, String>,
    next_order: usize,
}

impl AgentHubState {
    pub(crate) fn clear(&mut self) {
        self.agents.clear();
        self.parent_by_id.clear();
        self.next_order = 0;
    }

    pub(crate) fn len(&self) -> usize {
        self.agents.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    pub(crate) fn active_count(&self) -> usize {
        self.agents
            .values()
            .filter(|agent| agent.is_active())
            .count()
    }

    pub(crate) fn get(&self, id: &str) -> Option<&AgentRecord> {
        self.agents.get(id)
    }

    pub(crate) fn apply_snapshot(&mut self, snapshots: Vec<SubagentSnapshot>) {
        let present = snapshots
            .iter()
            .map(|snapshot| snapshot.id.clone())
            .collect::<HashSet<_>>();
        self.agents
            .retain(|id, agent| present.contains(id) || agent.is_terminal());

        for snapshot in snapshots {
            let order = self
                .agents
                .get(&snapshot.id)
                .map(|agent| agent.order)
                .unwrap_or_else(|| self.take_order());
            self.agents.insert(
                snapshot.id.clone(),
                AgentRecord {
                    id: snapshot.id,
                    index: snapshot.index,
                    agent: snapshot.agent,
                    agent_source: snapshot.agent_source,
                    description: snapshot.description,
                    status: normalize_status(&snapshot.status),
                    task: snapshot.task,
                    assignment: snapshot.assignment,
                    session_file: snapshot.session_file,
                    parent_tool_call_id: snapshot.parent_tool_call_id,
                    parent_id: snapshot.parent_id,
                    historical: snapshot.historical,
                    last_update: snapshot.last_update,
                    progress: snapshot.progress,
                    order,
                },
            );
        }
        self.refresh_parent_links();
    }

    /// Merge one lifecycle/progress/event frame. Returns its stable agent id when usable.
    pub(crate) fn apply_update(&mut self, update: SubagentUpdate) -> Option<String> {
        let id = update.id.clone()?;
        if update.kind == SubagentUpdateKind::Event {
            update.activity_event.as_ref()?;
            return self.agents.contains_key(&id).then_some(id);
        }

        let order = self
            .agents
            .get(&id)
            .map(|agent| agent.order)
            .unwrap_or_else(|| self.take_order());
        let existing = self.agents.get(&id).cloned();
        let progress = update
            .progress
            .clone()
            .or_else(|| existing.as_ref().and_then(|agent| agent.progress.clone()));
        let status = update
            .status
            .as_deref()
            .map(normalize_status)
            .or_else(|| existing.as_ref().map(|agent| agent.status.clone()))
            .unwrap_or_else(|| {
                if update.kind == SubagentUpdateKind::Progress {
                    "running".to_owned()
                } else {
                    "pending".to_owned()
                }
            });
        let record = AgentRecord {
            id: id.clone(),
            index: update
                .index
                .or_else(|| existing.as_ref().map(|agent| agent.index))
                .unwrap_or(usize::MAX),
            agent: update
                .agent
                .or_else(|| existing.as_ref().map(|agent| agent.agent.clone()))
                .unwrap_or_else(|| "subagent".to_owned()),
            agent_source: update.agent_source.or_else(|| {
                existing
                    .as_ref()
                    .and_then(|agent| agent.agent_source.clone())
            }),
            description: update.description.or_else(|| {
                existing
                    .as_ref()
                    .and_then(|agent| agent.description.clone())
            }),
            status,
            task: update
                .task
                .or_else(|| existing.as_ref().and_then(|agent| agent.task.clone())),
            assignment: update
                .assignment
                .or_else(|| existing.as_ref().and_then(|agent| agent.assignment.clone())),
            session_file: update.session_file.or_else(|| {
                existing
                    .as_ref()
                    .and_then(|agent| agent.session_file.clone())
            }),
            parent_tool_call_id: update.parent_tool_call_id.or_else(|| {
                existing
                    .as_ref()
                    .and_then(|agent| agent.parent_tool_call_id.clone())
            }),
            parent_id: existing.as_ref().and_then(|agent| agent.parent_id.clone()),
            historical: false,
            last_update: existing.as_ref().map_or(0, |agent| agent.last_update),
            progress,
            order,
        };
        self.agents.insert(id.clone(), record);
        self.refresh_parent_links();
        Some(id)
    }

    pub(crate) fn visible_rows(&self, collapsed: &HashSet<String>) -> Vec<AgentTreeRow> {
        self.project_rows(collapsed)
    }

    fn project_rows(&self, collapsed: &HashSet<String>) -> Vec<AgentTreeRow> {
        let mut children = HashMap::<Option<String>, Vec<&AgentRecord>>::new();
        for agent in self.agents.values() {
            let parent = self
                .parent_by_id
                .get(&agent.id)
                .filter(|parent| self.agents.contains_key(*parent))
                .cloned();
            children.entry(parent).or_default().push(agent);
        }
        for siblings in children.values_mut() {
            siblings.sort_by_key(|agent| (agent.index, agent.order));
        }

        let roots = children.remove(&None).unwrap_or_default();
        let mut projection = TreeProjection {
            children: &children,
            collapsed,
            visited: HashSet::new(),
            rows: Vec::with_capacity(self.agents.len()),
        };
        for root in roots {
            projection.append(root, 0, None, true);
        }

        let mut leftovers = self.agents.values().collect::<Vec<_>>();
        leftovers.sort_by_key(|agent| (agent.index, agent.order));
        for agent in leftovers {
            if !projection.visited.contains(&agent.id) {
                projection.append(agent, 0, None, true);
            }
        }
        projection.rows
    }

    fn take_order(&mut self) -> usize {
        let order = self.next_order;
        self.next_order += 1;
        order
    }

    fn refresh_parent_links(&mut self) {
        self.parent_by_id.retain(|child, parent| {
            self.agents.contains_key(child) && self.agents.contains_key(parent) && child != parent
        });
        for agent in self.agents.values() {
            if let Some(parent) = agent
                .parent_id
                .as_ref()
                .filter(|parent| self.agents.contains_key(*parent) && *parent != &agent.id)
            {
                self.parent_by_id.insert(agent.id.clone(), parent.clone());
            }
        }
        for parent in self.agents.values() {
            let Some(details) = parent
                .progress
                .as_ref()
                .and_then(|progress| progress.inflight_task_details.as_ref())
            else {
                continue;
            };
            for child in &details.progress {
                if child.id != parent.id && self.agents.contains_key(&child.id) {
                    self.parent_by_id
                        .insert(child.id.clone(), parent.id.clone());
                }
            }
        }
    }
}

struct TreeProjection<'a> {
    children: &'a HashMap<Option<String>, Vec<&'a AgentRecord>>,
    collapsed: &'a HashSet<String>,
    visited: HashSet<String>,
    rows: Vec<AgentTreeRow>,
}

impl TreeProjection<'_> {
    fn append(
        &mut self,
        agent: &AgentRecord,
        depth: usize,
        parent_id: Option<String>,
        visible: bool,
    ) {
        if !self.visited.insert(agent.id.clone()) {
            return;
        }
        let descendants = self.children.get(&Some(agent.id.clone()));
        if visible {
            self.rows.push(AgentTreeRow {
                agent: agent.clone(),
                depth,
                parent_id,
                has_children: descendants.is_some_and(|descendants| !descendants.is_empty()),
            });
        }
        let descendants_visible = visible && !self.collapsed.contains(&agent.id);
        if let Some(descendants) = descendants {
            for child in descendants {
                self.append(
                    child,
                    depth + 1,
                    Some(agent.id.clone()),
                    descendants_visible,
                );
            }
        }
    }
}

fn normalize_status(status: &str) -> String {
    match status.to_ascii_lowercase().as_str() {
        "started" => "running".to_owned(),
        status => status.to_owned(),
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn friendly_agent_name(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_uuid(value: &str) -> bool {
    value.len() >= 24
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::collections::HashSet;

    use super::AgentHubState;
    use crate::bridge::protocol::RpcEvent;
    use crate::bridge::protocol::{
        SubagentSnapshot, SubagentUpdate, SubagentUpdateKind, decode_event,
    };

    fn snapshot(id: &str, index: usize, status: &str) -> SubagentSnapshot {
        serde_json::from_value(json!({
            "id": id,
            "index": index,
            "agent": "task",
            "agentSource": "bundled",
            "status": status,
            "lastUpdate": 42
        }))
        .unwrap()
    }

    #[test]
    fn snapshot_and_lifecycle_refreshes_never_duplicate_agents() {
        let mut hub = AgentHubState::default();
        hub.apply_snapshot(vec![snapshot("Researcher", 0, "running")]);
        hub.apply_snapshot(vec![snapshot("Researcher", 0, "running")]);
        hub.apply_update(SubagentUpdate {
            kind: SubagentUpdateKind::Lifecycle,
            id: Some("Researcher".to_owned()),
            index: Some(0),
            agent: Some("task".to_owned()),
            agent_source: Some("bundled".to_owned()),
            status: Some("started".to_owned()),
            description: None,
            task: None,
            assignment: None,
            session_file: Some("/tmp/researcher.jsonl".to_owned()),
            parent_tool_call_id: None,
            progress: None,
            activity_event: None,
        });

        assert_eq!(hub.len(), 1);
        assert_eq!(hub.active_count(), 1);
        assert_eq!(hub.visible_rows(&HashSet::new())[0].agent.id, "Researcher");
    }

    #[test]
    fn authoritative_snapshot_removes_stale_active_rows_but_keeps_terminal_events() {
        let mut hub = AgentHubState::default();
        hub.apply_snapshot(vec![
            snapshot("Running", 0, "running"),
            snapshot("Finishing", 1, "running"),
        ]);
        let mut terminal = lifecycle("Finishing", 1, "completed");
        terminal.session_file = Some("/tmp/finished.jsonl".to_owned());
        hub.apply_update(terminal);
        hub.apply_snapshot(Vec::new());

        assert_eq!(hub.len(), 1);
        assert_eq!(hub.active_count(), 0);
        assert_eq!(hub.get("Finishing").unwrap().status, "completed");
        assert_eq!(
            hub.get("Finishing").unwrap().session_file.as_deref(),
            Some("/tmp/finished.jsonl")
        );
    }

    #[test]
    fn full_subagent_events_do_not_replace_authoritative_lifecycle_status() {
        let mut hub = AgentHubState::default();
        hub.apply_snapshot(vec![snapshot("Worker", 0, "running")]);
        let RpcEvent::Subagent(event) = decode_event(json!({
            "type": "subagent_event",
            "payload": { "id": "Worker", "event": { "type": "tool_execution_start" } }
        })) else {
            panic!("expected subagent event");
        };
        assert_eq!(hub.apply_update(*event).as_deref(), Some("Worker"));
        assert_eq!(hub.get("Worker").unwrap().status, "running");
    }

    #[test]
    fn nested_progress_ids_project_parent_before_child() {
        let mut hub = AgentHubState::default();
        let parent = serde_json::from_value(json!({
            "id": "Parent",
            "index": 0,
            "agent": "task",
            "status": "running",
            "lastUpdate": 10,
            "progress": progress("Parent", 0, Some(json!({
                "progress": [progress("Child", 0, None)]
            })))
        }))
        .unwrap();
        let child = serde_json::from_value(json!({
            "id": "Child",
            "index": 0,
            "agent": "scout",
            "status": "running",
            "lastUpdate": 11,
            "progress": progress("Child", 0, None)
        }))
        .unwrap();
        hub.apply_snapshot(vec![child, parent]);

        let rows = hub.visible_rows(&HashSet::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].agent.id, "Parent");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].agent.id, "Child");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[1].parent_id.as_deref(), Some("Parent"));
        assert!(rows[0].has_children);
        assert!(!rows[1].has_children);

        let collapsed = HashSet::from(["Parent".to_owned()]);
        let visible_rows = hub.visible_rows(&collapsed);
        assert_eq!(visible_rows.len(), 1);
        assert_eq!(visible_rows[0].agent.id, "Parent");
    }

    #[test]
    fn progress_merges_activity_and_metrics_without_losing_lifecycle_metadata() {
        let mut hub = AgentHubState::default();
        let mut start = lifecycle("Builder", 0, "started");
        start.session_file = Some("/tmp/builder.jsonl".to_owned());
        hub.apply_update(start);
        let RpcEvent::Subagent(update) = decode_event(json!({
            "type": "subagent_progress",
            "payload": {
                "index": 0,
                "agent": "task",
                "agentSource": "project",
                "task": "Build runtime hub",
                "progress": progress("Builder", 0, None)
            }
        })) else {
            panic!("expected progress event");
        };
        hub.apply_update(*update);

        let agent = hub.get("Builder").unwrap();
        assert_eq!(agent.session_file.as_deref(), Some("/tmp/builder.jsonl"));
        assert_eq!(agent.current_task(), Some("Build runtime hub"));
        assert_eq!(
            agent.current_activity().as_deref(),
            Some("Inspecting RPC · using read")
        );
        assert_eq!(agent.progress.as_ref().unwrap().tokens, 12_000);
    }

    fn lifecycle(id: &str, index: usize, status: &str) -> SubagentUpdate {
        SubagentUpdate {
            kind: SubagentUpdateKind::Lifecycle,
            id: Some(id.to_owned()),
            index: Some(index),
            agent: Some("task".to_owned()),
            agent_source: Some("bundled".to_owned()),
            status: Some(status.to_owned()),
            description: None,
            task: None,
            assignment: None,
            session_file: None,
            parent_tool_call_id: None,
            progress: None,
            activity_event: None,
        }
    }

    fn progress(id: &str, index: usize, inflight: Option<serde_json::Value>) -> serde_json::Value {
        json!({
            "id": id,
            "index": index,
            "agent": "task",
            "agentSource": "bundled",
            "status": "running",
            "task": "Build runtime hub",
            "lastIntent": "Inspecting RPC",
            "currentTool": "read",
            "recentTools": [],
            "recentOutput": [],
            "toolCount": 3,
            "requests": 2,
            "tokens": 12000,
            "cost": 0.04,
            "durationMs": 65000,
            "inflightTaskDetails": inflight
        })
    }
}
