use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::bridge::protocol::{message_role, message_text};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEntry {
    pub path: Option<PathBuf>,
    pub title: String,
    pub subtitle: String,
    pub cwd: Option<PathBuf>,
    pub current: bool,
}

pub fn session_entry(path: Option<&Path>, current_title: &str, current: bool) -> SessionEntry {
    let Some(path) = path else {
        return SessionEntry {
            path: None,
            title: authoritative_title(Some(current_title), None),
            subtitle: "Unsaved conversation".to_owned(),
            cwd: None,
            current,
        };
    };
    let metadata = read_session_metadata(path);
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|_| SystemTime::now());
    let title = resolved_title(
        metadata.title.as_deref(),
        current.then_some(current_title),
        metadata.first_message.as_deref(),
    );
    SessionEntry {
        path: Some(path.to_owned()),
        title,
        subtitle: session_subtitle(metadata.message_count, modified, metadata.cwd.as_deref()),
        cwd: metadata.cwd,
        current,
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
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    files
        .into_iter()
        .take(500)
        .map(|(path, _)| {
            let current = current_file == Some(path.as_path());
            session_entry(Some(&path), "", current)
        })
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
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        output.push((path, modified));
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
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{authoritative_title, discover_all_sessions, read_session_title, session_entry};

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
        let entry = session_entry(Some(&path), "New conversation", true);
        assert_eq!(entry.title, "Generated release plan");
        assert_eq!(entry.cwd.as_deref(), Some(Path::new("/tmp/project-one")));
        assert!(entry.subtitle.starts_with("project-one ·"));

        write_session(
            &path,
            "Updated generated plan",
            "/tmp/project-two",
            "Prepare the release",
        );
        let entry = session_entry(Some(&path), "Generated release plan", true);
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

        let entry = session_entry(Some(&path), "New conversation", true);
        assert_eq!(entry.title, "Investigate why the release build is slow");

        fs::remove_dir_all(directory).expect("remove message title fixture directory");
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

    fn fixture_directory(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("omp-native-{name}-{nonce}"));
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
