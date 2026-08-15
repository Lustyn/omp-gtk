use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::{chat::shimmer_markup, icons};
use adw::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use libadwaita as adw;
use serde_json::Value;

#[cfg(test)]
pub const BUILTIN_TOOL_NAMES: &[&str] = &[
    "read",
    "bash",
    "edit",
    "ast_grep",
    "ast_edit",
    "ask",
    "debug",
    "eval",
    "github",
    "glob",
    "grep",
    "lsp",
    "inspect_image",
    "browser",
    "computer",
    "checkpoint",
    "rewind",
    "security_scan",
    "task",
    "hub",
    "todo",
    "web_search",
    "write",
    "memory_edit",
    "retain",
    "recall",
    "reflect",
    "learn",
    "manage_skill",
    "yield",
    "goal",
    "think",
];

#[derive(Clone)]
pub struct ToolCard {
    pub root: gtk::Box,
    pub title: gtk::Label,
    pub summary: gtk::Label,
    pub status: gtk::Label,
    pub details: gtk::Label,
    pub spinner: gtk::Spinner,
    pub expander: gtk::Expander,
    name: String,
    args: Rc<RefCell<Value>>,
    intent: Rc<RefCell<Option<String>>>,
    active: Rc<Cell<bool>>,
    title_text: Rc<RefCell<String>>,
    summary_text: Rc<RefCell<String>>,
}

pub struct ToolPresentation {
    pub title: String,
    pub summary: String,
    pub details: String,
    pub icon: icons::Icon,
}

impl ToolCard {
    pub fn new(name: &str, args: &Value, intent: Option<&str>) -> Self {
        let presentation = presentation(name, args, None, intent);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("tool-card");
        root.add_css_class("activity-running");

        let expander = gtk::Expander::new(None);
        expander.set_tooltip_text(Some("Click to expand tool details"));
        expander.add_css_class("tool-expander");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        header.set_margin_top(10);
        header.set_margin_bottom(10);
        header.set_margin_start(12);
        header.set_margin_end(12);

        let icon = icons::icon(presentation.icon, 16);
        icon.add_css_class("activity-icon");
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        let title = gtk::Label::new(Some(&presentation.title));
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.add_css_class("activity-title");
        let summary = gtk::Label::new(Some(&presentation.summary));
        summary.set_xalign(0.0);
        summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        summary.add_css_class("tool-summary");
        text.append(&title);
        text.append(&summary);

        let spinner = gtk::Spinner::new();
        spinner.start();
        let status = gtk::Label::new(Some("Running"));
        status.add_css_class("activity-status");
        header.append(&icon);
        header.append(&text);
        header.append(&spinner);
        header.append(&status);
        expander.set_label_widget(Some(&header));

        let detail_box = gtk::Box::new(gtk::Orientation::Vertical, 7);
        detail_box.set_margin_bottom(12);
        detail_box.set_margin_start(14);
        detail_box.set_margin_end(14);
        let details = read_only_label(&presentation.details, "tool-details");
        detail_box.append(&details);
        expander.set_child(Some(&detail_box));
        root.append(&expander);
        wire_context_menu(&root, &details);

        let card = Self {
            root,
            title,
            summary,
            status,
            details,
            spinner,
            expander,
            name: name.to_owned(),
            args: Rc::new(RefCell::new(args.clone())),
            intent: Rc::new(RefCell::new(intent.map(ToOwned::to_owned))),
            active: Rc::new(Cell::new(true)),
            title_text: Rc::new(RefCell::new(presentation.title)),
            summary_text: Rc::new(RefCell::new(presentation.summary)),
        };
        card.start_animation();
        card
    }

    pub fn update_partial(&self, partial: &Value) {
        let presentation = presentation(
            &self.name,
            &self.args.borrow(),
            Some(partial),
            self.intent.borrow().as_deref(),
        );
        self.apply(&presentation);
    }

    pub fn complete(&self, result: &Value, is_error: bool) {
        self.active.set(false);
        self.spinner.stop();
        self.spinner.set_visible(false);
        self.status
            .set_text(if is_error { "Failed" } else { "Done" });
        self.root.remove_css_class("activity-running");
        self.root.add_css_class(if is_error {
            "activity-error"
        } else {
            "activity-done"
        });
        let presentation = presentation(
            &self.name,
            &self.args.borrow(),
            Some(result),
            self.intent.borrow().as_deref(),
        );
        self.apply(&presentation);
        if is_error {
            self.expander.set_expanded(true);
        }
    }

    fn apply(&self, presentation: &ToolPresentation) {
        self.title_text.replace(presentation.title.clone());
        self.summary_text.replace(presentation.summary.clone());
        self.title.set_text(&presentation.title);
        self.summary.set_text(&presentation.summary);
        self.details.set_text(&presentation.details);
    }

    fn start_animation(&self) {
        let title = self.title.downgrade();
        let summary = self.summary.downgrade();
        let title_text = self.title_text.clone();
        let summary_text = self.summary_text.clone();
        let active = self.active.clone();
        let started = Instant::now();
        self.title
            .set_markup(&shimmer_markup(&title_text.borrow(), Duration::ZERO));
        self.summary
            .set_markup(&shimmer_markup(&summary_text.borrow(), Duration::ZERO));
        glib::timeout_add_local(Duration::from_millis(33), move || {
            let (Some(title), Some(summary)) = (title.upgrade(), summary.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if !active.get() {
                return glib::ControlFlow::Break;
            }
            let elapsed = started.elapsed();
            title.set_markup(&shimmer_markup(&title_text.borrow(), elapsed));
            summary.set_markup(&shimmer_markup(&summary_text.borrow(), elapsed));
            glib::ControlFlow::Continue
        });
    }
}

pub fn presentation(
    name: &str,
    args: &Value,
    result: Option<&Value>,
    intent: Option<&str>,
) -> ToolPresentation {
    let normalized = name.to_ascii_lowercase();
    let summary = match normalized.as_str() {
        "read" => read_summary(args, result),
        "bash" => bash_summary(args, result),
        "edit" => edit_summary(args, result),
        "ast_grep" => ast_grep_summary(args, result),
        "ast_edit" => ast_edit_summary(args, result),
        "ask" => ask_summary(args, result),
        "debug" => action_target_summary(args, &["program", "file", "name"]),
        "eval" => eval_summary(args, result),
        "github" => action_target_summary(args, &["repo", "number", "query"]),
        "glob" => field_summary(args, "path", "Project files"),
        "grep" => grep_summary(args, result),
        "lsp" => action_target_summary(args, &["symbol", "file", "query"]),
        "inspect_image" => field_summary(args, "path", "Image"),
        "browser" => action_target_summary(args, &["url", "name"]),
        "computer" => action_target_summary(args, &["target", "application"]),
        "checkpoint" => action_target_summary(args, &["label", "name"]),
        "rewind" => action_target_summary(args, &["checkpoint", "label"]),
        "security_scan" => action_target_summary(args, &["path", "target"]),
        "task" => task_summary(args, result),
        "hub" => action_target_summary(args, &["name", "to"]),
        "todo" => todo_summary(args, result),
        "web_search" => field_summary(args, "query", "Web search"),
        "write" => write_summary(args, result),
        "memory_edit" => action_target_summary(args, &["path", "memory"]),
        "retain" => field_summary(args, "memory", "Save memory"),
        "recall" => field_summary(args, "query", "Recall memory"),
        "reflect" => field_summary(args, "topic", "Reflect on memory"),
        "learn" => field_summary(args, "topic", "Learn project convention"),
        "manage_skill" => action_target_summary(args, &["name", "skill"]),
        "yield" => field_summary(args, "message", "Return control"),
        "goal" => action_target_summary(args, &["objective"]),
        "think" => field_summary(args, "thought", "Reasoning"),
        _ if normalized.starts_with("mcp__") => mcp_summary(&normalized, args),
        _ => custom_summary(args),
    };
    let summary = intent
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(summary);
    ToolPresentation {
        title: tool_title(&normalized),
        summary: compact(&summary, 150),
        details: detail_text(args, result),
        icon: tool_icon(&normalized),
    }
}

fn read_summary(args: &Value, result: Option<&Value>) -> String {
    let path = string(args, "path").unwrap_or("File");
    let selector = path
        .rsplit_once(':')
        .filter(|(_, tail)| tail.chars().all(|ch| ch.is_ascii_digit() || ch == '-'))
        .map(|(_, range)| format!(" · lines {range}"))
        .unwrap_or_default();
    let count = result
        .and_then(line_count)
        .map(|lines| format!(" · {lines} lines"));
    format!(
        "{}{}{}",
        path_without_selector(path),
        selector,
        count.unwrap_or_default()
    )
}

fn bash_summary(args: &Value, result: Option<&Value>) -> String {
    let command = string(args, "command").unwrap_or("Shell command");
    let suffix = result
        .and_then(extract_text)
        .and_then(|text| text.lines().find(|line| !line.trim().is_empty()))
        .map(|line| format!(" · {}", compact(line, 60)))
        .unwrap_or_default();
    format!("{}{suffix}", compact(command, 100))
}

fn edit_summary(args: &Value, result: Option<&Value>) -> String {
    let patch = string(args, "input").unwrap_or_default();
    let files = patch
        .lines()
        .filter_map(|line| {
            line.strip_prefix('[')?
                .split_once('#')
                .map(|(path, _)| path)
        })
        .collect::<Vec<_>>();
    let label = count_targets(&files, "file");
    result_suffix(label, result)
}

fn ast_grep_summary(args: &Value, result: Option<&Value>) -> String {
    let pattern = string(args, "pattern")
        .or_else(|| string(args, "pat"))
        .unwrap_or("AST pattern");
    result_suffix(compact(pattern, 100), result)
}

fn ast_edit_summary(args: &Value, result: Option<&Value>) -> String {
    let operations = args
        .get("ops")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let paths = args
        .get("paths")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    result_suffix(format!("{operations} rewrites · {paths} targets"), result)
}

fn ask_summary(args: &Value, _result: Option<&Value>) -> String {
    let count = args
        .get("questions")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    format!("{count} {}", plural(count, "question", "questions"))
}

fn eval_summary(args: &Value, result: Option<&Value>) -> String {
    let language = string(args, "language").unwrap_or("code");
    let title = string(args, "title").unwrap_or("Evaluation");
    result_suffix(format!("{} · {title}", title_case(language)), result)
}

fn grep_summary(args: &Value, result: Option<&Value>) -> String {
    let pattern = string(args, "pattern").unwrap_or("Pattern");
    let path = string(args, "path").unwrap_or("workspace");
    result_suffix(format!("“{}” · {path}", compact(pattern, 70)), result)
}

fn task_summary(args: &Value, result: Option<&Value>) -> String {
    let count = args
        .get("tasks")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    result_suffix(
        format!("{count} {}", plural(count, "agent", "agents")),
        result,
    )
}

fn todo_summary(args: &Value, result: Option<&Value>) -> String {
    let operation = string(args, "op").unwrap_or("update");
    let target = string(args, "task")
        .or_else(|| string(args, "phase"))
        .unwrap_or("task list");
    result_suffix(format!("{} · {target}", title_case(operation)), result)
}

fn write_summary(args: &Value, result: Option<&Value>) -> String {
    let path = string(args, "path").unwrap_or("File");
    let bytes = string(args, "content").map(str::len).unwrap_or(0);
    result_suffix(format!("{path} · {bytes} bytes"), result)
}

fn action_target_summary(args: &Value, targets: &[&str]) -> String {
    let action = string(args, "action")
        .or_else(|| string(args, "op"))
        .unwrap_or("Run");
    let target = targets.iter().find_map(|key| string(args, key));
    match target {
        Some(target) => format!("{} · {}", title_case(action), compact(target, 90)),
        None => title_case(action),
    }
}

fn field_summary(args: &Value, field: &str, fallback: &str) -> String {
    string(args, field)
        .map(|value| compact(value, 120))
        .unwrap_or_else(|| fallback.to_owned())
}

fn mcp_summary(name: &str, args: &Value) -> String {
    let service = name
        .strip_prefix("mcp__")
        .and_then(|value| value.split_once('_'))
        .map(|(service, tool)| format!("{service} · {}", title_case(tool)))
        .unwrap_or_else(|| "MCP operation".to_owned());
    let target = ["path", "query", "name", "id"]
        .iter()
        .find_map(|key| string(args, key));
    target.map_or(service.clone(), |target| format!("{service} · {target}"))
}

fn custom_summary(args: &Value) -> String {
    ["path", "query", "command", "name", "message"]
        .iter()
        .find_map(|key| string(args, key))
        .map(|value| compact(value, 120))
        .unwrap_or_else(|| {
            let count = args.as_object().map_or(0, serde_json::Map::len);
            format!("{count} input {}", plural(count, "field", "fields"))
        })
}

fn detail_text(args: &Value, result: Option<&Value>) -> String {
    let mut sections = Vec::new();
    if !args.is_null() {
        sections.push(format!("INPUT\n{}", pretty(args)));
    }
    if let Some(result) = result.filter(|result| !result.is_null()) {
        sections.push(format!("OUTPUT\n{}", pretty(result)));
    }
    if sections.is_empty() {
        "No additional details".to_owned()
    } else {
        sections.join("\n\n")
    }
}

fn pretty(value: &Value) -> String {
    match extract_text(value) {
        Some(text) if value.is_string() => compact(text, 12_000),
        _ => compact(
            &serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
            12_000,
        ),
    }
}

fn result_suffix(summary: String, result: Option<&Value>) -> String {
    let Some(result) = result else {
        return summary;
    };
    let Some(text) = extract_text(result) else {
        return summary;
    };
    let first = text.lines().find(|line| !line.trim().is_empty());
    match first {
        Some(first) if !first.trim().is_empty() => format!("{summary} · {}", compact(first, 55)),
        _ => summary,
    }
}

fn tool_title(name: &str) -> String {
    match name {
        "read" => "Read file",
        "bash" => "Terminal",
        "edit" => "Edit files",
        "ast_grep" => "AST search",
        "ast_edit" => "AST rewrite",
        "ask" => "Question",
        "debug" => "Debugger",
        "eval" => "Code evaluation",
        "github" => "GitHub",
        "glob" => "Find files",
        "grep" => "Search text",
        "lsp" => "Language server",
        "inspect_image" => "Inspect image",
        "browser" => "Browser",
        "computer" => "Computer",
        "checkpoint" => "Checkpoint",
        "rewind" => "Rewind",
        "security_scan" => "Security scan",
        "task" => "Delegate tasks",
        "hub" => "Agent hub",
        "todo" => "Task list",
        "web_search" => "Web search",
        "write" => "Write file",
        "memory_edit" => "Edit memory",
        "retain" => "Retain memory",
        "recall" => "Recall memory",
        "reflect" => "Reflect",
        "learn" => "Learn",
        "manage_skill" => "Manage skill",
        "yield" => "Yield",
        "goal" => "Goal",
        "think" => "Reasoning",
        _ if name.starts_with("mcp__") => "MCP tool",
        _ => return title_case(&name.replace(['_', '-'], " ")),
    }
    .to_owned()
}

fn tool_icon(name: &str) -> icons::Icon {
    match name {
        "read" => icons::Icon::FileText,
        "bash" => icons::Icon::Terminal,
        "edit" => icons::Icon::Pencil,
        "ast_grep" => icons::Icon::SearchCode,
        "ast_edit" => icons::Icon::Braces,
        "ask" => icons::Icon::CircleHelp,
        "debug" => icons::Icon::Bug,
        "eval" => icons::Icon::SquareFunction,
        "github" => icons::Icon::GitPullRequest,
        "glob" => icons::Icon::FolderSearch,
        "grep" => icons::Icon::Search,
        "lsp" => icons::Icon::CodeXml,
        "inspect_image" => icons::Icon::Image,
        "browser" => icons::Icon::Globe,
        "computer" => icons::Icon::Monitor,
        "checkpoint" => icons::Icon::Save,
        "rewind" => icons::Icon::Undo2,
        "security_scan" => icons::Icon::ShieldCheck,
        "task" => icons::Icon::Users,
        "hub" => icons::Icon::Network,
        "todo" => icons::Icon::ListTodo,
        "web_search" => icons::Icon::Search,
        "write" => icons::Icon::FilePlus,
        "memory_edit" => icons::Icon::BrainCircuit,
        "retain" => icons::Icon::BookMarked,
        "recall" => icons::Icon::Brain,
        "reflect" => icons::Icon::Sparkles,
        "learn" => icons::Icon::GraduationCap,
        "manage_skill" => icons::Icon::Wrench,
        "goal" => icons::Icon::Target,
        "yield" => icons::Icon::Pause,
        "think" => icons::Icon::Brain,
        _ if name.starts_with("mcp__") => icons::Icon::Plug,
        _ => icons::Icon::Activity,
    }
}

fn read_only_label(text: &str, css_class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_yalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_focusable(false);
    label.add_css_class(css_class);
    label
}

fn wire_context_menu(root: &gtk::Box, details: &gtk::Label) {
    let popover = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.add_css_class("context-menu");
    let copy = icons::labeled_button(icons::Icon::Copy, "Copy details");
    copy.add_css_class("context-action");
    popover.set_child(Some(&copy));
    popover.set_parent(root);

    let details_for_copy = details.clone();
    let popover_for_copy = popover.clone();
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(&details_for_copy.text());
        }
        popover_for_copy.popdown();
    });
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    let popover_for_click = popover.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        popover_for_click.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover_for_click.popup();
    });
    root.add_controller(gesture);
    root.connect_unrealize(move |_| popover.unparent());
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn extract_text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("text").and_then(Value::as_str))
        .or_else(|| value.get("output").and_then(Value::as_str))
        .or_else(|| value.get("content").and_then(Value::as_str))
        .or_else(|| value.get("message").and_then(Value::as_str))
}

fn line_count(value: &Value) -> Option<usize> {
    extract_text(value).map(|text| text.lines().count())
}

fn path_without_selector(path: &str) -> &str {
    path.rsplit_once(':')
        .filter(|(_, tail)| tail.chars().all(|ch| ch.is_ascii_digit() || ch == '-'))
        .map_or(path, |(path, _)| path)
}

fn count_targets(values: &[&str], noun: &str) -> String {
    match values {
        [] => format!("{noun} changes"),
        [value] => (*value).to_owned(),
        many => format!("{} {noun}s", many.len()),
    }
}

fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn compact(text: &str, limit: usize) -> String {
    let flattened = text.replace(['\r', '\n', '\t'], " ");
    let mut output = flattened.split_whitespace().collect::<Vec<_>>().join(" ");
    if output.chars().count() > limit {
        output = output.chars().take(limit.saturating_sub(1)).collect();
        output.push('…');
    }
    output
}

fn title_case(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{BUILTIN_TOOL_NAMES, presentation};

    #[test]
    fn every_builtin_tool_has_a_compact_renderer() {
        let args = json!({
            "action": "inspect",
            "op": "view",
            "path": "src/main.rs:1-20",
            "query": "needle",
            "command": "cargo check",
            "input": "[src/main.rs#ABCD]",
            "content": "content",
            "questions": [{"question": "Proceed?"}],
            "tasks": [{"task": "Inspect"}],
            "ops": [{"pat": "$A", "out": "$A"}],
            "paths": ["src"],
            "language": "rust",
            "title": "Check",
            "objective": "Improve UI"
        });
        for name in BUILTIN_TOOL_NAMES {
            let rendered = presentation(name, &args, Some(&json!({"text": "done"})), None);
            assert!(!rendered.title.is_empty(), "missing title for {name}");
            assert!(!rendered.summary.is_empty(), "missing summary for {name}");
            assert!(
                rendered.details.contains("INPUT"),
                "missing details for {name}"
            );
            let _icon = rendered.icon;
        }
    }

    #[test]
    fn mcp_and_custom_tools_get_safe_catered_fallbacks() {
        let mcp = presentation("mcp__linear_issue", &json!({"id": "ENG-42"}), None, None);
        assert!(mcp.summary.contains("linear"));
        let custom = presentation("deploy_preview", &json!({"name": "staging"}), None, None);
        assert_eq!(custom.summary, "staging");
    }
}
