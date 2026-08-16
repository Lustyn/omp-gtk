use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gtk4::glib;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bridge::protocol::{SubagentProgress, SubagentSnapshot, message_role, message_text};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEntry {
    pub path: Option<PathBuf>,
    pub title: String,
    pub subtitle: String,
    pub cwd: Option<PathBuf>,
    pub(crate) created_at: SystemTime,
    pub current: bool,
    pub running: bool,
    pub runtime_id: Option<u64>,
}

#[derive(Default, Deserialize, Serialize)]
struct PersistedRecentSessions {
    #[serde(default)]
    sessions: Vec<PathBuf>,
}

pub fn load_recent_sessions() -> Vec<SessionEntry> {
    let path = recent_sessions_path();
    match load_recent_sessions_from(&path) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!(
                "Could not load recent sessions from {}: {error}",
                path.display()
            );
            Vec::new()
        }
    }
}

pub fn save_recent_sessions(entries: &[SessionEntry]) -> Result<(), String> {
    save_recent_sessions_to(&recent_sessions_path(), entries)
}

fn recent_sessions_path() -> PathBuf {
    glib::user_config_dir()
        .join("omp-gtk")
        .join("recent-sessions.json")
}

fn load_recent_sessions_from(path: &Path) -> Result<Vec<SessionEntry>, String> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
    };
    let persisted = serde_json::from_slice::<PersistedRecentSessions>(&contents)
        .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;
    let mut seen = HashSet::new();
    Ok(persisted
        .sessions
        .into_iter()
        .filter(|session| session.is_file() && seen.insert(session.clone()))
        .map(|session| session_entry(Some(&session), "", false, None))
        .collect())
}

fn save_recent_sessions_to(path: &Path, entries: &[SessionEntry]) -> Result<(), String> {
    let mut seen = HashSet::new();
    let sessions = entries
        .iter()
        .filter_map(|entry| entry.path.as_ref())
        .filter(|session| seen.insert((*session).clone()))
        .cloned()
        .collect();
    let persisted = PersistedRecentSessions { sessions };
    let parent = path
        .parent()
        .ok_or_else(|| "Recent sessions path has no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let contents = serde_json::to_vec_pretty(&persisted)
        .map_err(|error| format!("Could not encode recent sessions: {error}"))?;
    fs::write(&temporary, contents)
        .map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("Could not replace {}: {error}", path.display()))
}

pub fn session_entry(
    path: Option<&Path>,
    current_title: &str,
    current: bool,
    fallback_cwd: Option<&Path>,
) -> SessionEntry {
    let Some(path) = path else {
        return SessionEntry {
            path: None,
            title: authoritative_title(Some(current_title), None),
            subtitle: "Unsaved conversation".to_owned(),
            created_at: SystemTime::now(),
            cwd: fallback_cwd.map(Path::to_owned),
            current,
            running: false,
            runtime_id: None,
        };
    };
    let metadata = read_session_metadata(path);
    let file_metadata = fs::metadata(path).ok();
    let modified = file_metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .unwrap_or_else(SystemTime::now);
    let created_at = file_metadata
        .as_ref()
        .and_then(|metadata| metadata.created().ok())
        .unwrap_or(modified);
    let cwd = metadata.cwd.or_else(|| fallback_cwd.map(Path::to_owned));
    let title = resolved_title(
        metadata.title.as_deref(),
        current.then_some(current_title),
        metadata.first_message.as_deref(),
    );
    SessionEntry {
        path: Some(path.to_owned()),
        title,
        subtitle: session_subtitle(metadata.message_count, modified, cwd.as_deref()),
        cwd,
        created_at,
        current,
        running: false,
        runtime_id: None,
    }
}

pub fn discover_all_sessions(current_file: Option<&Path>) -> Vec<SessionEntry> {
    let mut roots = Vec::<(PathBuf, bool)>::new();
    if let Some(parent) = current_file.and_then(Path::parent) {
        roots.push((parent.to_owned(), false));
        if let Some(root) = parent.parent() {
            roots.push((root.to_owned(), true));
        }
    }
    if let Some(agent_dir) = env::var_os("PI_CODING_AGENT_DIR") {
        roots.push((PathBuf::from(agent_dir).join("sessions"), true));
    } else if let Some(home) = env::var_os("HOME") {
        roots.push((PathBuf::from(home).join(".omp/agent/sessions"), true));
    }

    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for (root, include_children) in roots {
        collect_session_files(&root, include_children, &mut seen, &mut files);
    }
    files.sort_by(|(left_path, left_created), (right_path, right_created)| {
        right_created
            .cmp(left_created)
            .then_with(|| right_path.cmp(left_path))
    });
    files
        .into_iter()
        .take(500)
        .map(|(path, _)| {
            let current = current_file == Some(path.as_path());
            session_entry(Some(&path), "", current, None)
        })
        .collect()
}

pub fn recent_workspaces(
    sessions: &[SessionEntry],
    current_workspace: Option<&Path>,
    limit: usize,
) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    sessions
        .iter()
        .filter_map(|entry| entry.cwd.as_ref())
        .filter(|workspace| {
            current_workspace != Some(workspace.as_path())
                && workspace.is_dir()
                && seen.insert((*workspace).clone())
        })
        .take(limit)
        .cloned()
        .collect()
}

fn collect_session_files(
    root: &Path,
    include_children: bool,
    seen: &mut HashSet<PathBuf>,
    output: &mut Vec<(PathBuf, SystemTime)>,
) {
    collect_jsonl_files(root, seen, output);
    if !include_children {
        return;
    }
    for child in fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
    {
        collect_jsonl_files(&child.path(), seen, output);
    }
}

fn collect_jsonl_files(
    directory: &Path,
    seen: &mut HashSet<PathBuf>,
    output: &mut Vec<(PathBuf, SystemTime)>,
) {
    for entry in fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl")
            || !seen.insert(path.clone())
        {
            continue;
        }
        let created = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        output.push((path, created));
    }
}

#[derive(Default)]
struct SessionMetadata {
    title: Option<String>,
    message_count: usize,
    first_message: Option<String>,
    cwd: Option<PathBuf>,
}

pub fn read_session_title(path: &Path) -> Option<String> {
    let metadata = read_session_metadata(path);
    let title = resolved_title(
        metadata.title.as_deref(),
        None,
        metadata.first_message.as_deref(),
    );
    (title != "New conversation").then_some(title)
}

pub fn read_session_messages(path: &Path) -> io::Result<Vec<Value>> {
    Ok(read_session_branch(path)?
        .into_iter()
        .filter_map(|entry| {
            (entry.get("type").and_then(Value::as_str) == Some("message"))
                .then(|| entry.get("message").cloned())
                .flatten()
        })
        .collect())
}

pub fn read_session_subagents(path: &Path) -> io::Result<Vec<SubagentSnapshot>> {
    let mut snapshots = Vec::new();
    let mut seen_files = HashSet::new();
    let mut seen_agents = HashSet::new();
    read_persisted_subagents(
        path,
        None,
        &mut seen_files,
        &mut seen_agents,
        &mut snapshots,
    )?;
    Ok(snapshots)
}

fn read_session_branch(path: &Path) -> io::Result<Vec<Value>> {
    struct BranchEntry {
        parent_id: Option<String>,
        entry: Value,
    }

    let file = File::open(path)?;
    let mut entries = HashMap::<String, BranchEntry>::new();
    let mut leaf_id = None;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) == Some("session") {
            continue;
        }
        let Some(id) = entry
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        let parent_id = entry
            .get("parentId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        entries.insert(id.clone(), BranchEntry { parent_id, entry });
        leaf_id = Some(id);
    }

    let mut branch = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = leaf_id;
    while let Some(id) = cursor.filter(|id| seen.insert(id.clone())) {
        let Some(entry) = entries.remove(&id) else {
            break;
        };
        branch.push(entry.entry);
        cursor = entry.parent_id;
    }
    branch.reverse();
    Ok(branch)
}

fn read_persisted_subagents(
    path: &Path,
    parent_id: Option<&str>,
    seen_files: &mut HashSet<PathBuf>,
    seen_agents: &mut HashSet<String>,
    output: &mut Vec<SubagentSnapshot>,
) -> io::Result<()> {
    if !seen_files.insert(path.to_owned()) {
        return Ok(());
    }

    let branch = read_session_branch(path)?;
    let mut agents = HashMap::<String, SubagentSnapshot>::new();
    let mut order = Vec::new();
    for entry in &branch {
        let timestamp = entry
            .pointer("/message/timestamp")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        match entry.get("type").and_then(Value::as_str) {
            Some("message") => {
                let Some(message) = entry.get("message") else {
                    continue;
                };
                if message_role(message) != Some("toolResult") {
                    continue;
                }
                let tool_name = message
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let Some(details) = message.get("details") else {
                    continue;
                };
                if tool_name == "task" {
                    for progress in details
                        .get("progress")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let Ok(progress) =
                            serde_json::from_value::<SubagentProgress>(progress.clone())
                        else {
                            continue;
                        };
                        let id = progress.id.clone();
                        if !agents.contains_key(&id) {
                            order.push(id.clone());
                        }
                        agents.insert(
                            id.clone(),
                            snapshot_from_progress(
                                progress,
                                parent_id,
                                message
                                    .get("toolCallId")
                                    .and_then(Value::as_str)
                                    .map(ToOwned::to_owned),
                                subagent_session_file(path, &id),
                                timestamp,
                            ),
                        );
                    }
                } else if tool_name == "hub" {
                    for job in details
                        .get("jobs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        apply_persisted_job(
                            &mut agents,
                            &mut order,
                            job,
                            None,
                            None,
                            parent_id,
                            path,
                            timestamp,
                        );
                    }
                }
            }
            Some("custom_message")
                if entry.get("customType").and_then(Value::as_str) == Some("async-result") =>
            {
                let content = entry.get("content").and_then(Value::as_str);
                for job in entry
                    .pointer("/details/jobs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    apply_persisted_job(
                        &mut agents,
                        &mut order,
                        job,
                        Some("completed"),
                        content,
                        parent_id,
                        path,
                        timestamp,
                    );
                }
            }
            _ => {}
        }
    }

    for id in order {
        let Some(mut snapshot) = agents.remove(&id) else {
            continue;
        };
        if matches!(snapshot.status.as_str(), "pending" | "running" | "started") {
            snapshot.status = "completed".to_owned();
            if let Some(progress) = snapshot.progress.as_mut() {
                progress.status = snapshot.status.clone();
            }
        }
        let transcript = snapshot.session_file.clone().map(PathBuf::from);
        if seen_agents.insert(snapshot.id.clone()) {
            output.push(snapshot);
        }
        if let Some(transcript) = transcript {
            let _ =
                read_persisted_subagents(&transcript, Some(&id), seen_files, seen_agents, output);
        }
    }
    Ok(())
}

fn snapshot_from_progress(
    progress: SubagentProgress,
    parent_id: Option<&str>,
    parent_tool_call_id: Option<String>,
    session_file: Option<PathBuf>,
    last_update: u64,
) -> SubagentSnapshot {
    SubagentSnapshot {
        id: progress.id.clone(),
        index: progress.index,
        agent: progress.agent.clone(),
        agent_source: progress.agent_source.clone(),
        description: progress.description.clone(),
        status: progress.status.clone(),
        task: (!progress.task.trim().is_empty()).then(|| progress.task.clone()),
        assignment: progress.assignment.clone(),
        session_file: session_file.map(|path| path.to_string_lossy().into_owned()),
        last_update,
        progress: Some(progress),
        parent_tool_call_id,
        parent_id: parent_id.map(ToOwned::to_owned),
        historical: true,
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_persisted_job(
    agents: &mut HashMap<String, SubagentSnapshot>,
    order: &mut Vec<String>,
    job: &Value,
    default_status: Option<&str>,
    content: Option<&str>,
    parent_id: Option<&str>,
    path: &Path,
    last_update: u64,
) {
    let Some(id) = job
        .get("id")
        .or_else(|| job.get("jobId"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let status = job
        .get("status")
        .and_then(Value::as_str)
        .or_else(|| persisted_result_status(content, id))
        .or(default_status)
        .unwrap_or("completed");
    if let Some(snapshot) = agents.get_mut(id) {
        snapshot.status = status.to_owned();
        snapshot.last_update = snapshot.last_update.max(last_update);
        if let Some(progress) = snapshot.progress.as_mut() {
            progress.status = status.to_owned();
            if let Some(duration) = job.get("durationMs").and_then(Value::as_u64) {
                progress.duration_ms = duration;
            }
            if let Some(model) = job.get("resolvedModel").and_then(Value::as_str) {
                progress.resolved_model = Some(model.to_owned());
            }
        }
        return;
    }

    let index = order.len();
    let agent = job
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("subagent");
    let task = job.get("label").and_then(Value::as_str).unwrap_or(id);
    let progress = serde_json::from_value::<SubagentProgress>(serde_json::json!({
        "id": id,
        "index": index,
        "agent": agent,
        "status": status,
        "task": task,
        "durationMs": job.get("durationMs").and_then(Value::as_u64).unwrap_or_default(),
        "resolvedModel": job.get("resolvedModel").and_then(Value::as_str),
    }))
    .expect("persisted job projection is valid");
    order.push(id.to_owned());
    agents.insert(
        id.to_owned(),
        snapshot_from_progress(
            progress,
            parent_id,
            None,
            subagent_session_file(path, id),
            last_update,
        ),
    );
}

fn persisted_result_status<'a>(content: Option<&'a str>, id: &str) -> Option<&'a str> {
    let content = content?;
    let marker = format!("<task-result id=\"{id}\"");
    let result = content.split_once(&marker)?.1;
    let status = result.split_once("status=\"")?.1.split_once('"')?.0;
    Some(if status.starts_with("failed") {
        "failed"
    } else if status.starts_with("aborted") {
        "aborted"
    } else {
        "completed"
    })
}

fn subagent_session_file(parent: &Path, id: &str) -> Option<PathBuf> {
    fs::read_dir(parent.with_extension(""))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
                && path.file_stem().and_then(|stem| stem.to_str()) == Some(id)
        })
}

fn read_session_metadata(path: &Path) -> SessionMetadata {
    let Ok(file) = File::open(path) else {
        return SessionMetadata::default();
    };
    let mut metadata = SessionMetadata::default();
    for line in BufReader::new(file).lines().map_while(Result::ok).take(400) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match entry.get("type").and_then(Value::as_str) {
            Some("title") => {
                metadata.title = entry
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned);
            }
            Some("session") => {
                metadata.cwd = entry
                    .get("cwd")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(PathBuf::from);
            }
            Some("message") => {
                metadata.message_count += 1;
                if metadata.first_message.is_none()
                    && let Some(message) = entry.get("message")
                    && message_role(message) == Some("user")
                {
                    let text = message_text(message);
                    if !text.trim().is_empty() {
                        metadata.first_message = Some(text);
                    }
                }
            }
            _ => {}
        }
    }
    metadata
}

fn truncate_title(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title = text.chars().take(72).collect::<String>();
    if text.chars().count() > 72 {
        title.push('…');
    }
    title
}

pub fn authoritative_title(primary: Option<&str>, persisted: Option<&str>) -> String {
    resolved_title(primary, persisted, None)
}

fn resolved_title(
    persisted: Option<&str>,
    current: Option<&str>,
    first_message: Option<&str>,
) -> String {
    persisted
        .into_iter()
        .chain(current)
        .chain(first_message)
        .map(str::trim)
        .find(|title| {
            !title.is_empty()
                && !title.eq_ignore_ascii_case("omp session")
                && !matches!(*title, "New conversation" | "Current session")
        })
        .map(truncate_title)
        .unwrap_or_else(|| "New conversation".to_owned())
}

fn session_subtitle(message_count: usize, modified: SystemTime, cwd: Option<&Path>) -> String {
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    let time = if age < Duration::from_secs(60) {
        "Just now".to_owned()
    } else if age < Duration::from_secs(3_600) {
        format!("{}m ago", age.as_secs() / 60)
    } else if age < Duration::from_secs(86_400) {
        format!("{}h ago", age.as_secs() / 3_600)
    } else {
        format!("{}d ago", age.as_secs() / 86_400)
    };
    let workspace = cwd
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("No workspace");
    if message_count == 0 {
        format!("{workspace} · {time}")
    } else {
        format!("{workspace} · {message_count} messages · {time}")
    }
}

pub fn delete_session_files(path: &Path) -> io::Result<()> {
    let data_directory = path.with_extension("");
    if data_directory.is_dir() {
        fs::remove_dir_all(data_directory)?;
    }
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        authoritative_title, discover_all_sessions, load_recent_sessions_from,
        read_session_messages, read_session_subagents, read_session_title, recent_workspaces,
        save_recent_sessions_to, session_entry,
    };

    #[test]
    fn uses_generated_title_before_fallbacks() {
        assert_eq!(
            authoritative_title(Some("New conversation"), Some("Coordinate release work")),
            "Coordinate release work"
        );
        assert_eq!(
            authoritative_title(None, Some("Generated session title")),
            "Generated session title"
        );
        assert_eq!(authoritative_title(None, None), "New conversation");
    }

    #[test]
    fn reloads_generated_title_and_workspace_from_disk() {
        let directory = fixture_directory("title");
        let path = directory.join("session.jsonl");
        write_session(
            &path,
            "Generated release plan",
            "/tmp/project-one",
            "Prepare the release",
        );

        assert_eq!(
            read_session_title(&path).as_deref(),
            Some("Generated release plan")
        );
        let entry = session_entry(Some(&path), "New conversation", true, None);
        assert_eq!(entry.title, "Generated release plan");
        assert_eq!(entry.cwd.as_deref(), Some(Path::new("/tmp/project-one")));
        assert!(entry.subtitle.starts_with("project-one ·"));

        write_session(
            &path,
            "Updated generated plan",
            "/tmp/project-two",
            "Prepare the release",
        );
        let entry = session_entry(Some(&path), "Generated release plan", true, None);
        assert_eq!(entry.title, "Updated generated plan");
        assert_eq!(entry.cwd.as_deref(), Some(Path::new("/tmp/project-two")));

        fs::remove_dir_all(directory).expect("remove title fixture directory");
    }

    #[test]
    fn falls_back_to_first_user_message_when_title_is_missing() {
        let directory = fixture_directory("message-title");
        let path = directory.join("session.jsonl");
        write_session(
            &path,
            "",
            "/tmp/project-one",
            "Investigate why the release build is slow",
        );
        assert_eq!(
            read_session_title(&path).as_deref(),
            Some("Investigate why the release build is slow")
        );

        let entry = session_entry(Some(&path), "New conversation", true, None);
        assert_eq!(entry.title, "Investigate why the release build is slow");

        fs::remove_dir_all(directory).expect("remove message title fixture directory");
    }

    #[test]
    fn restores_every_message_on_the_active_persisted_branch() {
        let directory = fixture_directory("transcript");
        let path = directory.join("session.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"version\":3,\"id\":\"session\",\"cwd\":\"/tmp/project\"}\n",
                "{\"type\":\"message\",\"id\":\"a\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"First prompt\"}}\n",
                "{\"type\":\"message\",\"id\":\"b\",\"parentId\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"First answer\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"off-branch\",\"parentId\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Abandoned answer\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"c\",\"parentId\":\"b\",\"message\":{\"role\":\"user\",\"content\":\"Second prompt\"}}\n",
                "{\"type\":\"title_change\",\"id\":\"d\",\"parentId\":\"c\",\"title\":\"Current title\"}\n",
            ),
        )
        .expect("write transcript fixture");

        let messages = read_session_messages(&path).expect("read transcript");
        let texts = messages
            .iter()
            .map(crate::bridge::protocol::message_text)
            .collect::<Vec<_>>();
        assert_eq!(texts, ["First prompt", "First answer", "Second prompt"]);

        fs::remove_dir_all(directory).expect("remove transcript fixture directory");
    }
    #[test]
    fn restores_nested_agents_and_terminal_statuses_from_session_files() {
        let directory = fixture_directory("subagent-hydration");
        let path = directory.join("session.jsonl");
        let artifacts = path.with_extension("");
        fs::create_dir(&artifacts).expect("create session artifacts");
        let parent_transcript = artifacts.join("ParentAgent.jsonl");
        let parent_artifacts = parent_transcript.with_extension("");
        fs::create_dir(&parent_artifacts).expect("create parent agent artifacts");
        let child_transcript = parent_artifacts.join("ChildScout.jsonl");

        let parent_spawn = json!({
            "type": "message",
            "id": "spawn-parent",
            "parentId": null,
            "message": {
                "role": "toolResult",
                "toolCallId": "task-parent",
                "toolName": "task",
                "timestamp": 100,
                "details": {
                    "progress": [{
                        "id": "ParentAgent",
                        "index": 0,
                        "agent": "task",
                        "status": "pending",
                        "task": "Coordinate persisted work"
                    }]
                }
            }
        });
        let parent_done = json!({
            "type": "custom_message",
            "customType": "async-result",
            "id": "done-parent",
            "parentId": "spawn-parent",
            "content": "<task-result id=\"ParentAgent\" agent=\"task\" status=\"completed\">done</task-result>",
            "details": {"jobs": [{"jobId": "ParentAgent", "type": "task", "durationMs": 4200}]}
        });
        fs::write(&path, format!("{parent_spawn}\n{parent_done}\n")).expect("write parent session");

        let child_spawn = json!({
            "type": "message",
            "id": "spawn-child",
            "parentId": null,
            "message": {
                "role": "toolResult",
                "toolCallId": "task-child",
                "toolName": "task",
                "timestamp": 200,
                "details": {
                    "progress": [{
                        "id": "ChildScout",
                        "index": 0,
                        "agent": "scout",
                        "status": "running",
                        "task": "Inspect persisted metadata"
                    }]
                }
            }
        });
        let child_done = json!({
            "type": "custom_message",
            "customType": "async-result",
            "id": "done-child",
            "parentId": "spawn-child",
            "content": "<task-result id=\"ChildScout\" agent=\"scout\" status=\"failed (exit 1)\">failed</task-result>",
            "details": {"jobs": [{"jobId": "ChildScout", "type": "scout", "durationMs": 900}]}
        });
        fs::write(&parent_transcript, format!("{child_spawn}\n{child_done}\n"))
            .expect("write parent transcript");
        fs::write(
            &child_transcript,
            "{\"type\":\"message\",\"id\":\"child-message\",\"parentId\":null,\"message\":{\"role\":\"assistant\",\"content\":\"Persisted result\"}}\n",
        )
        .expect("write child transcript");

        let agents = read_session_subagents(&path).expect("restore persisted agents");
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].id, "ParentAgent");
        assert_eq!(agents[0].status, "completed");
        assert!(agents[0].historical);
        assert_eq!(
            agents[0].session_file.as_deref(),
            parent_transcript.to_str()
        );
        assert_eq!(agents[1].id, "ChildScout");
        assert_eq!(agents[1].status, "failed");
        assert_eq!(agents[1].parent_id.as_deref(), Some("ParentAgent"));
        assert_eq!(agents[1].session_file.as_deref(), child_transcript.to_str());

        fs::remove_dir_all(directory).expect("remove subagent hydration fixture");
    }

    #[test]
    fn keeps_runtime_workspace_for_an_unpersisted_moved_session() {
        let workspace = Path::new("/tmp/moved-project");
        let entry = session_entry(None, "New conversation", true, Some(workspace));

        assert_eq!(entry.cwd.as_deref(), Some(workspace));
        assert_eq!(entry.subtitle, "Unsaved conversation");
    }

    #[test]
    fn discovers_sessions_across_workspaces_without_subagent_transcripts() {
        let root = fixture_directory("history");
        let first_project = root.join("project-one");
        let second_project = root.join("project-two");
        fs::create_dir_all(&first_project).expect("create first project");
        fs::create_dir_all(&second_project).expect("create second project");
        let current = first_project.join("current.jsonl");
        let past = second_project.join("past.jsonl");
        write_session(&current, "Current work", "/work/one", "Current request");
        write_session(&past, "Past work", "/work/two", "Past request");
        let nested = first_project.join("current").join("Subagent.jsonl");
        fs::create_dir_all(nested.parent().expect("nested parent")).expect("create subagent dir");
        write_session(&nested, "Subagent work", "/work/one", "Subagent request");

        let sessions = discover_all_sessions(Some(&current));
        assert!(
            sessions
                .iter()
                .any(|entry| entry.path.as_deref() == Some(current.as_path()) && entry.current)
        );
        assert!(
            sessions
                .iter()
                .any(|entry| entry.path.as_deref() == Some(past.as_path()))
        );
        assert!(
            sessions
                .iter()
                .all(|entry| entry.path.as_deref() != Some(nested.as_path()))
        );

        fs::remove_dir_all(root).expect("remove history fixture directory");
    }

    #[test]
    fn recent_sessions_survive_restart_in_list_order() {
        let root = fixture_directory("recent-sessions");
        let first = root.join("first.jsonl");
        let second = root.join("second.jsonl");
        let store = root.join("config/recent-sessions.json");
        write_session(&first, "First session", "/work/one", "First request");
        write_session(&second, "Second session", "/work/two", "Second request");
        let entries = vec![
            session_entry(Some(&second), "", false, None),
            session_entry(Some(&first), "", false, None),
            session_entry(Some(&second), "", false, None),
            session_entry(None, "Unsaved", false, None),
        ];

        save_recent_sessions_to(&store, &entries).expect("save recent sessions");
        let restored = load_recent_sessions_from(&store).expect("restore recent sessions");

        assert_eq!(
            restored
                .iter()
                .filter_map(|entry| entry.path.as_deref())
                .collect::<Vec<_>>(),
            [second.as_path(), first.as_path()]
        );
        assert_eq!(
            restored
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            ["Second session", "First session"]
        );
        assert!(restored.iter().all(|entry| !entry.current));
        assert!(restored.iter().all(|entry| entry.runtime_id.is_none()));

        fs::remove_file(&second).expect("remove stale recent session");
        let restored = load_recent_sessions_from(&store).expect("restore existing sessions");
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].path.as_deref(), Some(first.as_path()));

        fs::remove_dir_all(root).expect("remove recent sessions fixture directory");
    }

    #[test]
    fn recent_workspaces_are_unique_existing_directories() {
        let root = fixture_directory("recent-workspaces");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).expect("create first workspace");
        fs::create_dir_all(&second).expect("create second workspace");
        let sessions = [
            session_entry(None, "First", false, Some(&first)),
            session_entry(None, "First again", false, Some(&first)),
            session_entry(None, "Current", false, Some(&second)),
            session_entry(None, "Missing", false, Some(&root.join("missing"))),
        ];

        assert_eq!(recent_workspaces(&sessions, Some(&second), 3), [first]);

        fs::remove_dir_all(root).expect("remove recent workspace fixture directory");
    }

    fn fixture_directory(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("omp-gtk-{name}-{nonce}"));
        fs::create_dir(&directory).expect("create fixture directory");
        directory
    }

    fn write_session(path: &Path, title: &str, cwd: &str, first_message: &str) {
        fs::write(
            path,
            format!(
                "{{\"type\":\"title\",\"v\":1,\"title\":\"{title}\"}}\n\
                 {{\"type\":\"session\",\"version\":3,\"id\":\"session\",\"cwd\":\"{cwd}\"}}\n\
                 {{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"{first_message}\"}}]}}}}\n"
            ),
        )
        .expect("write session fixture");
    }
}
