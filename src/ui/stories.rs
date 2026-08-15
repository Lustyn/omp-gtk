use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::path::PathBuf;

use gtk::prelude::*;
use gtk4 as gtk;
use serde_json::json;

use super::chat::{ChatStatus, MessageRole, TelemetryWidgets};
use super::conversation::ConversationView;
use super::model_picker::ModelPickerView;
use super::tool_components::ToolCard;
use super::{composer, sidebar};
use crate::bridge::protocol::{InterruptMode, ModelSummary, ModelThinking, QueueMode};
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

const STORIES: [Story; 19] = [
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
        id: "conversation/markdown",
        title: "Conversation · Markdown",
        width: 900,
        height: 720,
        build: conversation_markdown,
    },
    Story {
        id: "composer/ready",
        title: "Composer · Ready",
        width: 900,
        height: 180,
        build: composer_ready,
    },
    Story {
        id: "composer/running-empty",
        title: "Composer · Running, empty draft",
        width: 900,
        height: 190,
        build: composer_running_empty,
    },
    Story {
        id: "composer/running-draft",
        title: "Composer · Running with draft",
        width: 900,
        height: 220,
        build: composer_running_draft,
    },
    Story {
        id: "composer/queued",
        title: "Composer · Queued messages",
        width: 900,
        height: 220,
        build: composer_queued,
    },
    Story {
        id: "composer/disconnected",
        title: "Composer · Disconnected",
        width: 900,
        height: 180,
        build: composer_disconnected,
    },
    Story {
        id: "composer/attachments-empty",
        title: "Composer attachments · Empty",
        width: 900,
        height: 180,
        build: composer_attachments_empty,
    },
    Story {
        id: "composer/attachments-populated",
        title: "Composer attachments · Populated",
        width: 900,
        height: 280,
        build: composer_attachments_populated,
    },
    Story {
        id: "composer/attachments-multiple",
        title: "Composer attachments · Multiple",
        width: 900,
        height: 280,
        build: composer_attachments_multiple,
    },
    Story {
        id: "composer/attachments-error",
        title: "Composer attachments · Read error retained",
        width: 900,
        height: 360,
        build: composer_attachments_error,
    },
    Story {
        id: "model-picker/default",
        title: "Model picker · Default",
        width: 760,
        height: 660,
        build: model_picker_default,
    },
    Story {
        id: "thinking-picker/default",
        title: "Thinking picker · Default",
        width: 760,
        height: 680,
        build: thinking_picker_default,
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
        id: "tool-card/read-image",
        title: "Tool card · Read image",
        width: 780,
        height: 560,
        build: tool_card_read_image,
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
fn conversation_markdown() -> gtk::Widget {
    let view = ConversationView::main();
    view.append_message(
        MessageRole::Assistant,
        "# Markdown history\n\nMessages now support **strong emphasis**, *italics*, \
         ~~strikethrough~~, and `inline code`.\n\n## Structured content\n\n\
         1. Ordered and unordered lists\n2. [Safe links](https://example.com)\n\
         3. Escaped HTML such as <button>\n\n> Block quotes preserve their visual hierarchy.\n\n\
         ```rust\nfn render(markdown: &str) {\n    println!(\"{markdown}\");\n}\n```",
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

fn composer_running_empty() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_model("anthropic", "Claude Opus 4.6");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view.set_running_turn_action(true);
    view.set_queue_state(
        QueueMode::OneAtATime,
        QueueMode::OneAtATime,
        InterruptMode::Immediate,
        0,
    );
    view.set_primary_action(true, true);
    view.widget().clone()
}

fn composer_running_draft() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_model("anthropic", "Claude Opus 4.6");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view.set_text("Run the accessibility review after this turn");
    view.set_running_turn_action(false);
    view.append_subagent_chip(&composer::subagent_chip("Designer", "Working", true));
    view.append_subagent_chip(&composer::subagent_chip("Reviewer", "Done", false));
    view.set_subagent_count("1 active · 2 total");
    view.set_subagents_visible(true);
    view.set_primary_action(true, true);
    view.widget().clone()
}

fn composer_queued() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_model("openai-codex", "GPT-5.6-Sol");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view.set_queue_state(
        QueueMode::All,
        QueueMode::OneAtATime,
        InterruptMode::Wait,
        3,
    );
    view.set_running_turn_action(true);
    view.set_primary_action(true, true);
    view.widget().clone()
}

fn composer_disconnected() -> gtk::Widget {
    let view = composer::build();
    view.set_model("openai-codex", "GPT-5.6-Sol");
    view.set_text("This draft remains available while omp reconnects");
    view.set_input_sensitive(false);
    view.set_primary_action(false, false);
    view.widget().clone()
}

fn attachment_story_composer() -> composer::ComposerView {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_attachment_sensitive(true);
    view.set_model("openai-codex", "GPT-5.6-Sol");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view
}

fn story_attachment_texture() -> gtk::gdk::Texture {
    gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from_static(include_bytes!(
        "../assets/omp.svg"
    )))
    .expect("story attachment is a valid image")
}

fn append_story_attachment(view: &composer::ComposerView, id: u64, name: &str) {
    view.append_attachment_preview(id, name, &story_attachment_texture(), |_| {});
}

fn composer_attachments_empty() -> gtk::Widget {
    let view = attachment_story_composer();
    view.set_primary_action(true, false);
    view.widget().clone()
}

fn composer_attachments_populated() -> gtk::Widget {
    let view = attachment_story_composer();
    view.set_text("Describe what is shown");
    append_story_attachment(&view, 1, "architecture.png");
    view.set_primary_action(true, false);
    view.widget().clone()
}

fn composer_attachments_multiple() -> gtk::Widget {
    let view = attachment_story_composer();
    view.set_text("Compare these in order");
    append_story_attachment(&view, 1, "first.png");
    append_story_attachment(&view, 2, "second.jpg");
    append_story_attachment(&view, 3, "third.png");
    view.set_primary_action(true, false);
    view.widget().clone()
}

fn composer_attachments_error() -> gtk::Widget {
    let conversation = ConversationView::transcript();
    conversation.append_notice(
        "Could not attach corrupt.jpg: Only PNG and JPEG images can be attached",
        true,
    );
    let view = attachment_story_composer();
    view.set_text("Keep this draft for retry");
    append_story_attachment(&view, 1, "retained.png");
    view.set_primary_action(true, false);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.append(conversation.widget());
    root.append(view.widget());
    root.upcast()
}

fn model_picker_default() -> gtk::Widget {
    let view = ModelPickerView::new(
        picker_models(),
        Some(("openai-codex".to_owned(), "gpt-5.6-sol".to_owned())),
        |_| {},
        || {},
    );
    view.widget().clone()
}

fn thinking_picker_default() -> gtk::Widget {
    let composer = composer::build();
    composer.set_input_sensitive(true);
    composer.set_model("openai-codex", "GPT-5.6-Sol");
    composer.set_model_sensitive(true);
    for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
        let option = composer::thinking_option(level);
        if level == "medium" {
            option.add_css_class("thinking-option-selected");
        }
        composer.append_thinking_option(&option);
    }
    composer.set_thinking_label("medium");
    composer.set_thinking_sensitive(true);
    let composer_for_model = composer.clone();
    composer.connect_model_clicked(move || composer_for_model.close_thinking_popover());
    let composer_for_popup = composer.clone();
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
        composer_for_popup.show_thinking_popover();
    });
    composer.widget().set_vexpand(false);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(composer.widget());
    root.upcast()
}

fn picker_models() -> Vec<ModelSummary> {
    vec![
        model("anthropic", "claude-opus-4-6", "Claude Opus 4.6", 1_000_000),
        model(
            "anthropic",
            "claude-sonnet-4-5",
            "Claude Sonnet 4.5",
            200_000,
        ),
        model("openai-codex", "gpt-5.6-sol", "GPT-5.6-Sol", 272_000),
        model("openai-codex", "gpt-5.4-mini", "GPT-5.4-Mini", 128_000),
        model("google", "gemini-3-pro", "Gemini 3 Pro", 1_000_000),
        model(
            "github-copilot",
            "claude-sonnet-4.6",
            "Claude Sonnet 4.6",
            200_000,
        ),
        model("mistral", "devstral-2", "Devstral 2", 256_000),
        model("openrouter", "deepseek/deepseek-v4", "DeepSeek V4", 164_000),
        model("ollama-cloud", "qwen3-coder", "Qwen3 Coder", 128_000),
    ]
}

fn model(provider: &str, id: &str, name: &str, context_window: u64) -> ModelSummary {
    ModelSummary {
        provider: provider.to_owned(),
        id: id.to_owned(),
        name: Some(name.to_owned()),
        thinking: Some(ModelThinking {
            efforts: vec!["low".to_owned(), "medium".to_owned(), "high".to_owned()],
        }),
        context_window: Some(context_window),
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

fn tool_card_read_image() -> gtk::Widget {
    let card = ToolCard::new(
        "read",
        &json!({"path": "src/assets/omp.svg"}),
        Some("Reading application artwork"),
    );
    card.complete(
        &json!({
            "content": [
                {"type": "text", "text": "Decoded image"},
                {
                    "type": "image",
                    "data": STANDARD.encode(include_bytes!("../assets/omp.svg")),
                    "mimeType": "image/svg+xml"
                }
            ],
            "details": {}
        }),
        false,
    );
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
