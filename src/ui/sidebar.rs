use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use adw::prelude::*;
use gtk::{gdk, gio};
use gtk4 as gtk;
use libadwaita as adw;
use serde_json::Value;

use super::icons;
use crate::bridge::protocol::{message_role, message_text};

#[derive(Clone)]
pub struct SidebarWidgets {
    pub root: gtk::Box,
    pub list: gtk::ListBox,
    pub new_chat: gtk::Button,
    pub history: gtk::Button,
    pub collapse: gtk::Button,
    pub active_count: gtk::Label,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEntry {
    pub path: Option<PathBuf>,
    pub title: String,
    pub subtitle: String,
    pub cwd: Option<PathBuf>,
    pub current: bool,
}

#[derive(Clone)]
pub struct SessionRow {
    pub row: gtk::ListBoxRow,
    pub badge: gtk::Label,
    pub open_action: gtk::Button,
    pub rename_action: gtk::Button,
    pub close_action: gtk::Button,
    pub delete_action: gtk::Button,
    pub entry: SessionEntry,
}

pub fn build() -> SidebarWidgets {
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.set_size_request(286, -1);
    sidebar.add_css_class("sidebar");

    let brand_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    brand_row.set_margin_top(12);
    brand_row.set_margin_bottom(10);
    brand_row.set_margin_start(14);
    brand_row.set_margin_end(10);
    brand_row.append(&icons::omp_logo(29));
    let brand_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    brand_spacer.set_hexpand(true);
    let active_count = gtk::Label::new(None);
    active_count.set_visible(false);
    active_count.add_css_class("sidebar-activity-count");
    let collapse = icons::icon_button(icons::Icon::PanelLeftClose, "Collapse sidebar");
    collapse.add_css_class("sidebar-toggle");
    brand_row.append(&brand_spacer);
    brand_row.append(&active_count);
    brand_row.append(&collapse);
    let brand_handle = gtk::WindowHandle::new();
    brand_handle.set_child(Some(&brand_row));

    let new_chat = gtk::Button::new();
    let new_chat_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    new_chat_content.append(&icons::icon(icons::Icon::Plus, 16));
    let label = gtk::Label::new(Some("New conversation"));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    new_chat_content.append(&label);
    new_chat_content.append(&gtk::Label::new(Some("Ctrl+N")));
    new_chat.set_child(Some(&new_chat_content));
    new_chat.set_margin_start(12);
    new_chat.set_margin_end(12);
    new_chat.set_tooltip_text(Some("Start a new omp conversation"));
    new_chat.add_css_class("new-chat-button");

    let section_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    section_row.set_margin_top(20);
    section_row.set_margin_bottom(7);
    section_row.set_margin_start(16);
    section_row.set_margin_end(10);
    let section = gtk::Label::new(Some("RECENT"));
    section.set_xalign(0.0);
    section.set_hexpand(true);
    section.add_css_class("section-label");
    let history = icons::icon_button(icons::Icon::History, "Search conversation history");
    history.add_css_class("history-button");
    section_row.append(&section);
    section_row.append(&history);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("session-list");
    list.set_margin_start(8);
    list.set_margin_end(8);
    let empty = gtk::Label::new(Some("No open conversations"));
    empty.add_css_class("session-list-empty");
    list.set_placeholder(Some(&empty));
    let session_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    sidebar.append(&brand_handle);
    sidebar.append(&new_chat);
    sidebar.append(&section_row);
    sidebar.append(&session_scroll);

    SidebarWidgets {
        root: sidebar,
        list,
        new_chat,
        history,
        collapse,
        active_count,
    }
}

pub fn session_row(entry: SessionEntry) -> SessionRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    row.set_selectable(true);
    if entry.current {
        row.add_css_class("current-session");
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(9);
    content.set_margin_end(4);
    let indicator = gtk::Box::new(gtk::Orientation::Vertical, 0);
    indicator.add_css_class("session-indicator");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(&entry.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("session-title");
    let subtitle = gtk::Label::new(Some(&entry.subtitle));
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    subtitle.add_css_class("session-subtitle");
    if let Some(cwd) = entry.cwd.as_deref() {
        subtitle.set_tooltip_text(Some(&cwd.to_string_lossy()));
    }
    text.append(&title);
    text.append(&subtitle);
    let badge = gtk::Label::new(None);
    badge.set_visible(false);
    badge.add_css_class("session-badge");
    let close_action = icons::icon_button(icons::Icon::X, "Close conversation");
    close_action.add_css_class("session-close");
    content.append(&indicator);
    content.append(&text);
    content.append(&badge);
    content.append(&close_action);
    row.set_child(Some(&content));

    let popover = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.add_css_class("context-menu");
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let open_action = context_button(icons::Icon::FolderOpen, "Open conversation");
    let rename_action = context_button(icons::Icon::Pencil, "Rename conversation");
    let close_menu_action = context_button(icons::Icon::X, "Close conversation");
    let copy_action = context_button(icons::Icon::Copy, "Copy title");
    let reveal_action = context_button(icons::Icon::Folder, "Reveal transcript");
    let delete_action = context_button(icons::Icon::Trash2, "Delete conversation");
    delete_action.add_css_class("destructive-action");
    menu.append(&open_action);
    menu.append(&rename_action);
    menu.append(&close_menu_action);
    menu.append(&copy_action);
    menu.append(&reveal_action);
    menu.append(&delete_action);
    popover.set_child(Some(&menu));
    popover.set_parent(&row);

    let close_from_menu = close_action.clone();
    let popover_for_close = popover.clone();
    close_menu_action.connect_clicked(move |_| {
        close_from_menu.emit_clicked();
        popover_for_close.popdown();
    });
    let title_for_copy = entry.title.clone();
    let popover_for_copy = popover.clone();
    copy_action.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(&title_for_copy);
        }
        popover_for_copy.popdown();
    });
    if let Some(path) = entry.path.clone() {
        reveal_action.connect_clicked(move |_| {
            let uri = gio::File::for_path(&path).uri();
            let _ = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE);
        });
    } else {
        reveal_action.set_sensitive(false);
        delete_action.set_sensitive(false);
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    let popover_for_click = popover.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        popover_for_click.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover_for_click.popup();
    });
    row.add_controller(gesture);
    row.connect_unrealize(move |_| popover.unparent());

    SessionRow {
        row,
        badge,
        open_action,
        rename_action,
        close_action,
        delete_action,
        entry,
    }
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

pub fn present_history(
    parent: &adw::ApplicationWindow,
    sessions: Vec<SessionEntry>,
    active_paths: &HashSet<PathBuf>,
    on_open: impl Fn(SessionEntry) + 'static,
) {
    let sessions = sessions
        .into_iter()
        .filter(|entry| {
            entry
                .path
                .as_ref()
                .is_some_and(|path| !active_paths.contains(path))
        })
        .collect::<Vec<_>>();

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("history-picker");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("history-picker-header");
    let heading = gtk::Label::new(Some("Conversation history"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("history-picker-heading");
    let close = icons::icon_button(icons::Icon::X, "Close conversation history");
    close.add_css_class("history-picker-close");
    header.append(&heading);
    header.append(&close);
    root.append(&header);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search titles and workspaces"));
    search.add_css_class("history-picker-search");
    root.append(&search);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("history-picker-list");
    let empty = gtk::Label::new(Some("No closed conversations match."));
    empty.add_css_class("history-picker-empty");
    list.set_placeholder(Some(&empty));

    let rows = sessions
        .into_iter()
        .map(|entry| {
            let row = gtk::ListBoxRow::new();
            row.set_selectable(false);
            let button = gtk::Button::new();
            button.add_css_class("history-session");
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            content.append(&icons::icon(icons::Icon::MessageSquare, 16));
            let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
            text.set_hexpand(true);
            let title = gtk::Label::new(Some(&entry.title));
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.add_css_class("history-session-title");
            let subtitle = gtk::Label::new(Some(&entry.subtitle));
            subtitle.set_xalign(0.0);
            subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
            subtitle.add_css_class("history-session-subtitle");
            text.append(&title);
            text.append(&subtitle);
            content.append(&text);
            content.append(&icons::icon(icons::Icon::ArrowUpRight, 14));
            button.set_child(Some(&content));
            row.set_child(Some(&button));
            list.append(&row);
            (row, button, entry)
        })
        .collect::<Vec<_>>();
    let rows = Rc::new(rows);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    scroll.add_css_class("history-picker-scroll");
    root.append(&scroll);

    let dialog = adw::Dialog::builder()
        .title("Conversation history")
        .content_width(680)
        .content_height(620)
        .child(&root)
        .build();
    let weak_dialog = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });

    let rows_for_search = rows.clone();
    search.connect_search_changed(move |search| {
        let query = search.text().trim().to_ascii_lowercase();
        for (row, _, entry) in rows_for_search.iter() {
            let cwd = entry
                .cwd
                .as_deref()
                .map(|path| path.to_string_lossy())
                .unwrap_or_default();
            let matches = query.is_empty()
                || entry.title.to_ascii_lowercase().contains(&query)
                || entry.subtitle.to_ascii_lowercase().contains(&query)
                || cwd.to_ascii_lowercase().contains(&query);
            row.set_visible(matches);
        }
    });

    let on_open = Rc::new(on_open);
    for (_, button, entry) in rows.iter() {
        let entry = entry.clone();
        let on_open = on_open.clone();
        let weak_dialog = dialog.downgrade();
        button.connect_clicked(move |_| {
            on_open(entry.clone());
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
    }
    dialog.present(Some(parent));
    search.grab_focus();
}

fn context_button(icon: icons::Icon, text: &str) -> gtk::Button {
    let button = icons::labeled_button(icon, text);
    button.add_css_class("context-action");
    button
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
