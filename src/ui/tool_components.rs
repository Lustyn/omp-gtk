use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::{
    chat::{self, shimmer_markup},
    icons,
};
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

const COLLAPSED_ACTIVITY_COUNT: usize = 3;
fn collapsed_activity_start(count: usize) -> usize {
    count.saturating_sub(COLLAPSED_ACTIVITY_COUNT)
}
fn activity_entry_is_active(index: usize, count: usize, working: bool, finished: bool) -> bool {
    working && !finished && count.checked_sub(1) == Some(index)
}

#[derive(Clone)]
struct ActivityPreview {
    root: gtk::Box,
    title: gtk::Label,
    summary: gtk::Label,
    active: Rc<Cell<bool>>,
    title_text: Rc<RefCell<String>>,
    summary_text: Rc<RefCell<String>>,
    animation_generation: Rc<Cell<u64>>,
}

impl ActivityPreview {
    fn new(icon: icons::Icon, title: &str, summary: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("activity-preview-row");
        let icon = icons::icon(icon, 14);
        icon.add_css_class("activity-icon");
        let title_label = gtk::Label::new(Some(title));
        title_label.set_xalign(0.0);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.add_css_class("activity-preview-title");
        title_label.set_visible(!title.is_empty());
        let summary_label = gtk::Label::new(Some(summary));
        summary_label.set_xalign(0.0);
        summary_label.set_hexpand(true);
        summary_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        summary_label.add_css_class("activity-summary");
        root.append(&icon);
        root.append(&title_label);
        root.append(&summary_label);
        Self {
            root,
            title: title_label,
            summary: summary_label,
            active: Rc::new(Cell::new(false)),
            title_text: Rc::new(RefCell::new(title.to_owned())),
            summary_text: Rc::new(RefCell::new(summary.to_owned())),
            animation_generation: Rc::new(Cell::new(0)),
        }
    }

    fn sync_text(&self, title: &str, summary: &str) {
        self.title_text.replace(title.to_owned());
        self.summary_text.replace(summary.to_owned());
        self.title.set_text(title);
        self.summary.set_text(summary);
    }

    fn set_active(&self, active: bool) {
        if self.active.replace(active) == active {
            return;
        }
        let generation = self.animation_generation.get().wrapping_add(1);
        self.animation_generation.set(generation);
        if !active {
            self.title.set_text(&self.title_text.borrow());
            self.summary.set_text(&self.summary_text.borrow());
            return;
        }

        let summary = self.summary.downgrade();
        let summary_text = self.summary_text.clone();
        let active = self.active.clone();
        let animation_generation = self.animation_generation.clone();
        let started = Instant::now();
        self.summary
            .set_markup(&shimmer_markup(&summary_text.borrow(), Duration::ZERO));
        glib::timeout_add_local(Duration::from_millis(33), move || {
            let Some(summary) = summary.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !active.get() || animation_generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            summary.set_markup(&shimmer_markup(&summary_text.borrow(), started.elapsed()));
            glib::ControlFlow::Continue
        });
    }
}

#[derive(Clone)]
enum ActivityEntry {
    Tool {
        card: ToolCard,
        preview: ActivityPreview,
    },
    Thinking {
        thinking: chat::ThinkingBlock,
        preview: ActivityPreview,
    },
    Notice {
        preview: ActivityPreview,
    },
}

impl ActivityEntry {
    fn preview(&self) -> &ActivityPreview {
        match self {
            Self::Tool { preview, .. }
            | Self::Thinking { preview, .. }
            | Self::Notice { preview, .. } => preview,
        }
    }

    fn sync(&self) {
        match self {
            Self::Tool { card, preview, .. } => {
                preview.sync_text(&card.title.text(), &card.summary.text());
            }
            Self::Thinking { thinking, preview } => {
                preview.sync_text("", &thinking.summary());
            }
            Self::Notice { .. } => {}
        }
    }
}

#[derive(Clone)]
pub struct ToolActivityGroup {
    pub root: gtk::Box,
    entries: Rc<RefCell<Vec<ActivityEntry>>>,
    index_by_id: Rc<RefCell<HashMap<String, usize>>>,
    history_toggle: gtk::Button,
    history_label: gtk::Label,
    expanded: Rc<Cell<bool>>,
    working: Rc<Cell<bool>>,
    finished: Rc<Cell<bool>>,
}

impl ToolActivityGroup {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.add_css_class("tool-activity-group");
        root.set_visible(false);

        let history_toggle = gtk::Button::new();
        history_toggle.add_css_class("activity-history-toggle");
        history_toggle.set_halign(gtk::Align::Start);
        history_toggle.set_visible(false);
        let history_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        history_content.append(&icons::icon(icons::Icon::History, 12));
        let history_label = gtk::Label::new(None);
        history_content.append(&history_label);
        history_toggle.set_child(Some(&history_content));
        root.append(&history_toggle);

        let entries = Rc::new(RefCell::new(Vec::new()));
        let expanded = Rc::new(Cell::new(false));
        let working = Rc::new(Cell::new(false));
        let finished = Rc::new(Cell::new(false));
        let group = Self {
            root,
            entries,
            index_by_id: Rc::new(RefCell::new(HashMap::new())),
            history_toggle,
            history_label,
            expanded,
            working,
            finished,
        };
        let root = group.root.downgrade();
        let toggle = group.history_toggle.downgrade();
        let label = group.history_label.downgrade();
        let entries = group.entries.clone();
        let expanded = group.expanded.clone();
        let working = group.working.clone();
        let finished = group.finished.clone();
        group.history_toggle.connect_clicked(move |_| {
            expanded.set(!expanded.get());
            let (Some(root), Some(toggle), Some(label)) =
                (root.upgrade(), toggle.upgrade(), label.upgrade())
            else {
                return;
            };
            refresh_activity_group(
                &root,
                &toggle,
                &label,
                &entries,
                expanded.get(),
                working.get(),
                finished.get(),
            );
        });
        group
    }

    pub fn ensure_card(
        &self,
        id: &str,
        name: &str,
        args: &Value,
        intent: Option<&str>,
    ) -> ToolCard {
        if let Some(index) = self.index_by_id.borrow().get(id).copied() {
            let entries = self.entries.borrow();
            let ActivityEntry::Tool { card, .. } = &entries[index] else {
                unreachable!("tool call index points to a non-tool activity");
            };
            return card.clone();
        }

        let presentation = presentation(name, args, None, intent);
        let card = ToolCard::new(name, args, intent);
        let preview = ActivityPreview::new(
            presentation.icon,
            &presentation.title,
            &presentation.summary,
        );
        self.root.append(&preview.root);
        let mut entries = self.entries.borrow_mut();
        let index = entries.len();
        entries.push(ActivityEntry::Tool {
            card: card.clone(),
            preview,
        });
        self.index_by_id.borrow_mut().insert(id.to_owned(), index);
        drop(entries);
        self.refresh();
        card
    }

    pub fn append_thinking(&self, text: &str, streaming: bool) -> chat::ThinkingBlock {
        let thinking = chat::ThinkingBlock::new(text, streaming);
        let preview = ActivityPreview::new(icons::Icon::BrainCircuit, "", &thinking.summary());
        self.root.append(&preview.root);
        self.entries.borrow_mut().push(ActivityEntry::Thinking {
            thinking: thinking.clone(),
            preview,
        });
        self.refresh();
        thinking
    }

    pub fn append_notice(&self, text: &str, is_error: bool) {
        let preview = ActivityPreview::new(
            if is_error {
                icons::Icon::TriangleAlert
            } else {
                icons::Icon::Info
            },
            if is_error { "Error" } else { "Update" },
            &compact_update(text, 150),
        );
        if is_error {
            preview.root.add_css_class("activity-error");
        }
        self.root.append(&preview.root);
        self.entries
            .borrow_mut()
            .push(ActivityEntry::Notice { preview });
        self.refresh();
    }

    pub fn update_partial(&self, id: &str, partial: &Value) {
        let Some(index) = self.index_by_id.borrow().get(id).copied() else {
            return;
        };
        let entries = self.entries.borrow();
        let ActivityEntry::Tool { card, .. } = &entries[index] else {
            return;
        };
        card.update_partial(partial);
        drop(entries);
        self.refresh();
    }

    pub fn complete(&self, id: &str, result: &Value, is_error: bool) {
        let Some(index) = self.index_by_id.borrow().get(id).copied() else {
            return;
        };
        {
            let mut entries = self.entries.borrow_mut();
            let ActivityEntry::Tool { card, preview } = &mut entries[index] else {
                return;
            };
            card.complete(result, is_error);
            preview.root.add_css_class(if is_error {
                "activity-error"
            } else {
                "activity-done"
            });
        }
        self.refresh();
    }

    pub fn set_working(&self, working: bool) {
        if self.working.replace(working) != working {
            self.refresh();
        }
    }

    pub fn refresh_summary(&self) {
        self.refresh();
    }

    pub fn finish(&self) {
        self.working.set(false);
        if self.finished.replace(true) {
            return;
        }
        let entries = self.entries.borrow();
        for entry in entries.iter() {
            if let ActivityEntry::Tool { card, .. } = entry {
                card.finish_incomplete();
            } else if let ActivityEntry::Thinking { thinking, .. } = entry
                && thinking.is_active()
            {
                thinking.finish(None);
            }
            entry.sync();
        }
        drop(entries);
        self.refresh();
    }

    fn refresh(&self) {
        refresh_activity_group(
            &self.root,
            &self.history_toggle,
            &self.history_label,
            &self.entries,
            self.expanded.get(),
            self.working.get(),
            self.finished.get(),
        );
    }
}

fn refresh_activity_group(
    root: &gtk::Box,
    history_toggle: &gtk::Button,
    history_label: &gtk::Label,
    entries: &Rc<RefCell<Vec<ActivityEntry>>>,
    expanded: bool,
    working: bool,
    finished: bool,
) {
    let entries = entries.borrow();
    if entries.is_empty() {
        root.set_visible(false);
        return;
    }
    let hidden_count = entries.len().saturating_sub(COLLAPSED_ACTIVITY_COUNT);
    let first_visible = if expanded {
        0
    } else {
        collapsed_activity_start(entries.len())
    };
    for (index, entry) in entries.iter().enumerate() {
        entry.sync();
        entry.preview().set_active(activity_entry_is_active(
            index,
            entries.len(),
            working,
            finished,
        ));
        entry.preview().root.set_visible(index >= first_visible);
    }
    history_toggle.set_visible(hidden_count > 0);
    if expanded {
        history_label.set_text(&format!("Show {COLLAPSED_ACTIVITY_COUNT} recent"));
        history_toggle.set_tooltip_text(Some("Collapse earlier tool activity"));
    } else {
        history_label.set_text(&format!(
            "{hidden_count} earlier {}",
            if hidden_count == 1 {
                "action"
            } else {
                "actions"
            }
        ));
        history_toggle.set_tooltip_text(Some("Show earlier tool activity"));
    }
    root.set_visible(true);
}

#[derive(Clone)]
pub struct ToolCard {
    pub root: gtk::Box,
    pub title: gtk::Label,
    pub summary: gtk::Label,
    pub status: gtk::Label,
    pub details: gtk::Label,
    pub spinner: gtk::Spinner,
    pub expander: gtk::Expander,
    image_box: gtk::Box,
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
        let image_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        image_box.add_css_class("tool-image-box");
        image_box.set_visible(false);
        root.append(&image_box);
        wire_context_menu(&root, &details);

        let card = Self {
            root,
            title,
            summary,
            status,
            details,
            spinner,
            expander,
            image_box,
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
        self.show_read_images(result, is_error);
        if is_error {
            self.expander.set_expanded(true);
        }
    }
    fn finish_incomplete(&self) {
        if !self.active.replace(false) {
            return;
        }
        self.spinner.stop();
        self.spinner.set_visible(false);
        self.status.set_text("Stopped");
        self.root.remove_css_class("activity-running");
    }

    fn apply(&self, presentation: &ToolPresentation) {
        self.title_text.replace(presentation.title.clone());
        self.summary_text.replace(presentation.summary.clone());
        self.title.set_text(&presentation.title);
        self.summary.set_text(&presentation.summary);
        self.details.set_text(&presentation.details);
    }

    fn show_read_images(&self, result: &Value, is_error: bool) {
        while let Some(child) = self.image_box.first_child() {
            self.image_box.remove(&child);
        }
        if is_error || !self.name.eq_ignore_ascii_case("read") {
            self.image_box.set_visible(false);
            return;
        }

        let path = string(&self.args.borrow(), "path")
            .unwrap_or("image")
            .to_owned();
        for data in read_image_data(result) {
            let Ok(decoded) = STANDARD.decode(data) else {
                continue;
            };
            let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(decoded)) else {
                continue;
            };
            let picture = gtk::Picture::new();
            picture.set_paintable(Some(&texture));
            picture.set_alternative_text(Some(&format!("Read image preview: {path}")));
            picture.set_tooltip_text(Some(&path));
            picture.set_content_fit(gtk::ContentFit::Contain);
            picture.set_can_shrink(true);
            picture.set_hexpand(true);
            picture.set_height_request(preview_height(texture.width(), texture.height()));
            picture.add_css_class("tool-image-preview");
            self.image_box.append(&picture);
        }
        self.image_box
            .set_visible(self.image_box.first_child().is_some());
    }

    fn start_animation(&self) {
        let summary = self.summary.downgrade();
        let summary_text = self.summary_text.clone();
        let active = self.active.clone();
        let started = Instant::now();
        self.summary
            .set_markup(&shimmer_markup(&summary_text.borrow(), Duration::ZERO));
        glib::timeout_add_local(Duration::from_millis(33), move || {
            let Some(summary) = summary.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !active.get() {
                return glib::ControlFlow::Break;
            }
            summary.set_markup(&shimmer_markup(&summary_text.borrow(), started.elapsed()));
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

fn read_image_data(result: &Value) -> Vec<&str> {
    result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("image"))
        .filter(|block| {
            block
                .get("mimeType")
                .and_then(Value::as_str)
                .is_some_and(|mime_type| mime_type.starts_with("image/"))
        })
        .filter_map(|block| block.get("data").and_then(Value::as_str))
        .collect()
}

fn preview_height(width: i32, height: i32) -> i32 {
    if width <= 0 || height <= 0 {
        return 320;
    }
    ((720.0 * f64::from(height) / f64::from(width)).round() as i32).clamp(160, 420)
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

#[derive(Clone, Copy)]
struct UpdateTag<'a> {
    start: usize,
    end: usize,
    name: &'a str,
    closing: bool,
    self_closing: bool,
}

fn compact_update(text: &str, limit: usize) -> String {
    let tags = update_tags(text);
    if !tags.iter().any(|tag| update_tag_is_removable(*tag, &tags)) {
        return compact(text, limit);
    }

    let mut visible = String::with_capacity(text.len());
    let mut cursor = 0;
    for tag in &tags {
        if update_tag_is_removable(*tag, &tags) {
            visible.push_str(&text[cursor..tag.start]);
            cursor = tag.end;
        }
    }
    visible.push_str(&text[cursor..]);
    compact(&visible, limit)
}

fn update_tag_is_removable(tag: UpdateTag<'_>, tags: &[UpdateTag<'_>]) -> bool {
    tag.self_closing
        || tags.iter().any(|candidate| {
            candidate.name == tag.name
                && candidate.closing != tag.closing
                && !candidate.self_closing
                && if tag.closing {
                    candidate.start < tag.start
                } else {
                    candidate.start > tag.start
                }
        })
}

fn update_tags(text: &str) -> Vec<UpdateTag<'_>> {
    let mut tags = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find('<') {
        let start = cursor + offset;
        if let Some(tag) = parse_update_tag(text, start) {
            cursor = tag.end;
            tags.push(tag);
        } else {
            cursor = start + 1;
        }
    }
    tags
}

fn parse_update_tag(text: &str, start: usize) -> Option<UpdateTag<'_>> {
    let bytes = text.as_bytes();
    let mut cursor = start.checked_add(1)?;
    let closing = bytes.get(cursor) == Some(&b'/');
    if closing {
        cursor += 1;
    }

    let name_start = cursor;
    if !bytes.get(cursor)?.is_ascii_alphabetic() {
        return None;
    }
    cursor += 1;
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        cursor += 1;
    }
    let name_end = cursor;
    if !bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>'))
    {
        return None;
    }

    let mut quote = None;
    while let Some(&byte) = bytes.get(cursor) {
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'<' => return None,
                b'>' => {
                    let mut content_end = cursor;
                    while content_end > name_end && bytes[content_end - 1].is_ascii_whitespace() {
                        content_end -= 1;
                    }
                    return Some(UpdateTag {
                        start,
                        end: cursor + 1,
                        name: &text[name_start..name_end],
                        closing,
                        self_closing: !closing
                            && content_end > name_end
                            && bytes[content_end - 1] == b'/',
                    });
                }
                _ => {}
            }
        }
        cursor += 1;
    }
    None
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

    use super::{
        BUILTIN_TOOL_NAMES, activity_entry_is_active, collapsed_activity_start, compact_update,
        presentation, preview_height, read_image_data,
    };

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
    fn collapsed_activity_shows_only_the_latest_three_actions() {
        assert_eq!(collapsed_activity_start(0), 0);
        assert_eq!(collapsed_activity_start(3), 0);
        assert_eq!(collapsed_activity_start(5), 2);
    }

    #[test]
    fn only_the_trailing_activity_animates_while_the_agent_is_working() {
        assert!(!activity_entry_is_active(0, 2, true, false));
        assert!(activity_entry_is_active(1, 2, true, false));
        assert!(!activity_entry_is_active(1, 2, false, false));
        assert!(!activity_entry_is_active(1, 2, true, true));
        assert!(!activity_entry_is_active(0, 0, true, false));
    }

    #[test]
    fn mcp_and_custom_tools_get_safe_catered_fallbacks() {
        let mcp = presentation("mcp__linear_issue", &json!({"id": "ENG-42"}), None, None);
        assert!(mcp.summary.contains("linear"));
        let custom = presentation("deploy_preview", &json!({"name": "staging"}), None, None);
        assert_eq!(custom.summary, "staging");
    }

    #[test]
    fn update_preview_hides_balanced_omp_tags() {
        assert_eq!(
            compact_update(
                "<system-reminder>\n2 todos remain. Continue working.\n</system-reminder>",
                150,
            ),
            "2 todos remain. Continue working."
        );
        assert_eq!(
            compact_update(
                concat!(
                    "<system-notice reason=\"manual_continue\">",
                    "<role>Continue.</role> Read memory://<memory-id>.",
                    "</system-notice>",
                ),
                150,
            ),
            "Continue. Read memory://<memory-id>."
        );
    }

    #[test]
    fn update_preview_preserves_plain_angle_brackets_and_unpaired_tags() {
        let text = "Keep x < y > z and the placeholder <memory-id>.";
        assert_eq!(compact_update(text, 150), text);
    }

    #[test]
    fn extracts_only_valid_inline_image_blocks() {
        let result = json!({
            "content": [
                {"type": "text", "text": "Loaded image"},
                {"type": "image", "data": "aW1hZ2U=", "mimeType": "image/png"},
                {"type": "image", "data": "bm90LWltYWdl", "mimeType": "application/octet-stream"},
                {"type": "image", "mimeType": "image/jpeg"}
            ],
            "details": {}
        });
        assert_eq!(read_image_data(&result), vec!["aW1hZ2U="]);
        assert!(read_image_data(&json!({"content": "text"})).is_empty());
    }

    #[test]
    fn preview_height_preserves_common_ratios_within_bounds() {
        assert_eq!(preview_height(1920, 1080), 405);
        assert_eq!(preview_height(100, 1000), 420);
        assert_eq!(preview_height(1000, 100), 160);
        assert_eq!(preview_height(0, 0), 320);
    }
}
