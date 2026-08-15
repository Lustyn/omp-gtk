use std::path::PathBuf;

use gtk::prelude::*;
use gtk4 as gtk;
use serde_json::json;

use super::chat::{ChatStatus, MessageRole, TelemetryWidgets};
use super::conversation::ConversationView;
use super::model_picker::ModelPickerView;
use super::tool_components::ToolCard;
use super::{composer, sidebar};
use crate::bridge::protocol::ModelSummary;
use crate::session_catalog::SessionEntry;

#[derive(Clone, Copy)]
pub(crate) struct Story {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) build: fn() -> gtk::Widget,
}

pub(crate) fn all() -> &'static [Story] {
    &STORIES
}

pub(crate) fn find(id: &str) -> Option<Story> {
    STORIES.iter().copied().find(|story| story.id == id)
}

const STORIES: [Story; 12] = [
    Story {
        id: "header/ready",
        title: "Header · Ready",
        width: 900,
        height: 110,
        build: header_ready,
    },
    Story {
        id: "header/working",
        title: "Header · Working",
        width: 900,
        height: 110,
        build: header_working,
    },
    Story {
        id: "sidebar/sessions",
        title: "Sidebar · Sessions",
        width: 320,
        height: 700,
        build: sidebar_sessions,
    },
    Story {
        id: "conversation/empty",
        title: "Conversation · Empty",
        width: 900,
        height: 620,
        build: conversation_empty,
    },
    Story {
        id: "conversation/tool-use",
        title: "Conversation · Tool use",
        width: 900,
        height: 700,
        build: conversation_tool_use,
    },
    Story {
        id: "composer/ready",
        title: "Composer · Ready",
        width: 900,
        height: 180,
        build: composer_ready,
    },
    Story {
        id: "composer/running",
        title: "Composer · Running with agents",
        width: 900,
        height: 220,
        build: composer_running,
    },
    Story {
        id: "model-picker/default",
        title: "Model picker · Default",
        width: 760,
        height: 660,
        build: model_picker_default,
    },
    Story {
        id: "tool-card/running",
        title: "Tool card · Running",
        width: 780,
        height: 180,
        build: tool_card_running,
    },
    Story {
        id: "tool-card/success",
        title: "Tool card · Success",
        width: 780,
        height: 220,
        build: tool_card_success,
    },
    Story {
        id: "tool-card/error",
        title: "Tool card · Error",
        width: 780,
        height: 260,
        build: tool_card_error,
    },
    Story {
        id: "thinking/streaming",
        title: "Thinking · Streaming",
        width: 780,
        height: 220,
        build: thinking_streaming,
    },
];

fn header_ready() -> gtk::Widget {
    header("ready")
}

fn header_working() -> gtk::Widget {
    header("working")
}

fn header(state: &str) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("header-box");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("chat-header");
    let title = gtk::Label::new(Some("Refactor native component architecture"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("chat-title");
    let status = ChatStatus::new();
    if state == "working" {
        status.activity("Using read");
    } else {
        status.idle();
    }
    row.append(&title);
    row.append(&status.root);
    let telemetry = TelemetryWidgets::new("~/code/omp-native");
    telemetry.set_context(14_200, 272_000, 5.2);
    telemetry.set_cost(0.024);
    telemetry.set_throughput((state == "working").then_some(86.4));
    root.append(&row);
    root.append(&telemetry.root);
    root.upcast()
}

fn sidebar_sessions() -> gtk::Widget {
    let view = sidebar::build();
    for entry in [
        SessionEntry {
            path: Some(PathBuf::from("/tmp/current.jsonl")),
            title: "Refactor native component architecture".to_owned(),
            subtitle: "omp-native · 42 messages · Just now".to_owned(),
            cwd: Some(PathBuf::from("/home/agent/code/omp-native")),
            current: true,
        },
        SessionEntry {
            path: Some(PathBuf::from("/tmp/accessibility.jsonl")),
            title: "Improve accessible component selectors".to_owned(),
            subtitle: "omp-native · 16 messages · 18m ago".to_owned(),
            cwd: Some(PathBuf::from("/home/agent/code/omp-native")),
            current: false,
        },
        SessionEntry {
            path: Some(PathBuf::from("/tmp/release.jsonl")),
            title: "Prepare the release and verify packaging".to_owned(),
            subtitle: "desktop-client · 8 messages · 2h ago".to_owned(),
            cwd: Some(PathBuf::from("/home/agent/code/desktop-client")),
            current: false,
        },
    ] {
        view.list.append(&sidebar::session_row(entry).row);
    }
    view.root.upcast()
}

fn conversation_empty() -> gtk::Widget {
    let view = ConversationView::main();
    view.show_empty();
    view.widget().clone()
}

fn conversation_tool_use() -> gtk::Widget {
    let view = ConversationView::main();
    view.append_message(
        MessageRole::User,
        "Split the UI into independently visualizable components.",
    );
    view.append_thinking(
        "I’ll first map component ownership, then isolate the runtime from rendering.",
        false,
    );
    let card = ToolCard::new(
        "read",
        &json!({"path": "src/ui/composer.rs:1-220"}),
        Some("Inspecting composer component"),
    );
    card.complete(&json!({"text": "Loaded 220 lines"}), false);
    view.append(&card.root);
    view.append_message(
        MessageRole::Assistant,
        "The composer now owns its widget state and exposes behavior-oriented methods.",
    );
    view.widget().clone()
}

fn composer_ready() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_model("openai-codex", "GPT-5.6-Sol");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view.set_text("Describe the component you want to build");
    view.set_primary_action(true, false);
    view.widget().clone()
}

fn composer_running() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_model("anthropic", "Claude Opus 4.6");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view.set_text("Implement the native UI gallery");
    view.append_subagent_chip(&composer::subagent_chip("Designer", "Working", true));
    view.append_subagent_chip(&composer::subagent_chip("Reviewer", "Done", false));
    view.set_subagent_count("1 active · 2 total");
    view.set_subagents_visible(true);
    view.set_primary_action(true, true);
    view.widget().clone()
}

fn model_picker_default() -> gtk::Widget {
    let models = vec![
        model("anthropic", "claude-opus-4-6", "Claude Opus 4.6", 1_000_000),
        model(
            "anthropic",
            "claude-sonnet-4-5",
            "Claude Sonnet 4.5",
            200_000,
        ),
        model("openai-codex", "gpt-5.6-sol", "GPT-5.6-Sol", 272_000),
        model("openai-codex", "gpt-5.4-mini", "GPT-5.4-Mini", 128_000),
    ];
    let view = ModelPickerView::new(
        models,
        Some(("openai-codex".to_owned(), "gpt-5.6-sol".to_owned())),
        |_| {},
        || {},
    );
    view.widget().clone()
}

fn model(provider: &str, id: &str, name: &str, context_window: u64) -> ModelSummary {
    ModelSummary {
        provider: provider.to_owned(),
        id: id.to_owned(),
        name: Some(name.to_owned()),
        thinking: None,
        context_window: Some(context_window),
        max_tokens: Some(128_000),
    }
}

fn tool_card_running() -> gtk::Widget {
    let card = ToolCard::new(
        "bash",
        &json!({"command": "cargo test --all-targets"}),
        Some("Running component tests"),
    );
    card.root.clone().upcast()
}

fn tool_card_success() -> gtk::Widget {
    let card = ToolCard::new(
        "write",
        &json!({"path": "src/ui/gallery.rs", "content": "..."}),
        Some("Creating native gallery"),
    );
    card.complete(&json!({"text": "Successfully wrote 180 lines"}), false);
    card.root.clone().upcast()
}

fn tool_card_error() -> gtk::Widget {
    let card = ToolCard::new(
        "bash",
        &json!({"command": "cargo check"}),
        Some("Checking component boundary"),
    );
    card.complete(
        &json!({"error": "no field `input` on type `WorkspaceView`"}),
        true,
    );
    card.root.clone().upcast()
}

fn thinking_streaming() -> gtk::Widget {
    let view = ConversationView::transcript();
    view.append_thinking(
        "Tracing state ownership and identifying the narrowest stable component API…",
        true,
    );
    view.widget().clone()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::all;

    #[test]
    fn story_ids_are_unique_and_component_scoped() {
        let mut ids = HashSet::new();
        for story in all() {
            assert!(story.id.contains('/'), "unscoped story id: {}", story.id);
            assert!(ids.insert(story.id), "duplicate story id: {}", story.id);
            assert!(story.width > 0 && story.height > 0);
        }
    }
}
