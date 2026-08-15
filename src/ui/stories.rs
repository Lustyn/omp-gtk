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
use super::{agent_hub, composer, session_actions, sidebar, todos};
use crate::agent_hub::AgentHubState;
use crate::bridge::protocol::{
    BranchMessage, InterruptMode, ModelSummary, ModelThinking, QueueMode, SubagentSnapshot, TodoItem,
    TodoPhase, TodoStatus,
};
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

const STORIES: [Story; 42] = [
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
        id: "todos/empty",
        title: "Todos · Empty",
        width: 900,
        height: 180,
        build: todos_empty,
    },
    Story {
        id: "todos/multi-phase",
        title: "Todos · Multiple phases",
        width: 900,
        height: 560,
        build: todos_multi_phase,
    },
    Story {
        id: "todos/blocked",
        title: "Todos · Blocked",
        width: 900,
        height: 280,
        build: todos_blocked,
    },
    Story {
        id: "todos/completed",
        title: "Todos · Completed",
        width: 900,
        height: 280,
        build: todos_completed,
    },
    Story {
        id: "todos/abandoned",
        title: "Todos · Abandoned",
        width: 900,
        height: 280,
        build: todos_abandoned,
    },
    Story {
        id: "todos/active",
        title: "Todos · Active task",
        width: 900,
        height: 320,
        build: todos_active,
    },
    Story {
        id: "todos/long-text",
        title: "Todos · Long text",
        width: 900,
        height: 360,
        build: todos_long_text,
    },
    Story {
        id: "agent-hub/empty",
        title: "Agent Hub · Empty",
        width: 1100,
        height: 700,
        build: agent_hub_empty,
    },
    Story {
        id: "agent-hub/running",
        title: "Agent Hub · Running",
        width: 1100,
        height: 700,
        build: agent_hub_running,
    },
    Story {
        id: "agent-hub/tree",
        title: "Agent Hub · Parent and child",
        width: 1100,
        height: 700,
        build: agent_hub_tree,
    },
    Story {
        id: "agent-hub/terminal",
        title: "Agent Hub · Terminal",
        width: 1100,
        height: 700,
        build: agent_hub_terminal,
    },
    Story {
        id: "agent-hub/failure",
        title: "Agent Hub · Failure",
        width: 1100,
        height: 700,
        build: agent_hub_failure,
    },
    Story {
        id: "agent-hub/long-task",
        title: "Agent Hub · Long task",
        width: 1100,
        height: 700,
        build: agent_hub_long_task,
    },
    Story {
        id: "agent-hub/transcript",
        title: "Agent Hub · Live transcript",
        width: 1100,
        height: 700,
        build: agent_hub_transcript,
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
        id: "branch-picker/linear",
        title: "Branch picker · Linear candidates",
        width: 720,
        height: 640,
        build: branch_picker_linear,
    },
    Story {
        id: "branch-picker/multiple",
        title: "Branch picker · Multiple candidates",
        width: 720,
        height: 640,
        build: branch_picker_multiple,
    },
    Story {
        id: "branch-picker/long-content",
        title: "Branch picker · Long labels and previews",
        width: 720,
        height: 640,
        build: branch_picker_long_content,
    },
    Story {
        id: "branch-picker/empty",
        title: "Branch picker · Empty",
        width: 720,
        height: 640,
        build: branch_picker_empty,
    },
    Story {
        id: "branch-picker/error",
        title: "Branch picker · Error",
        width: 720,
        height: 640,
        build: branch_picker_error,
    },
    Story {
        id: "handoff/default",
        title: "Handoff · Default",
        width: 620,
        height: 440,
        build: handoff_default,
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

fn todos_empty() -> gtk::Widget {
    todo_story(Vec::new())
}

fn todos_multi_phase() -> gtk::Widget {
    todo_story(vec![
        TodoPhase {
            name: "Research".to_owned(),
            tasks: vec![
                todo_item("Inspect the omp RPC todo contract", TodoStatus::Completed),
                todo_item("Map native workspace state", TodoStatus::InProgress),
                todo_item("Review accessible controls", TodoStatus::Pending),
            ],
        },
        TodoPhase {
            name: "Implementation".to_owned(),
            tasks: vec![
                todo_item("Build the ordered todo panel", TodoStatus::Pending),
                todo_item("Add focused serialization tests", TodoStatus::Pending),
            ],
        },
    ])
}

fn todos_blocked() -> gtk::Widget {
    todo_story(vec![TodoPhase {
        name: "Release".to_owned(),
        tasks: vec![TodoItem {
            content: "Publish the native package".to_owned(),
            status: TodoStatus::Blocked,
            blocker: Some("Waiting for signing credentials from the release owner".to_owned()),
        }],
    }])
}

fn todos_completed() -> gtk::Widget {
    todo_story(vec![TodoPhase {
        name: "Verification".to_owned(),
        tasks: vec![
            todo_item("Round-trip every todo status", TodoStatus::Completed),
            todo_item("Confirm session switching", TodoStatus::Completed),
        ],
    }])
}

fn todos_abandoned() -> gtk::Widget {
    todo_story(vec![TodoPhase {
        name: "Experiments".to_owned(),
        tasks: vec![
            todo_item("Parse todo state from transcript prose", TodoStatus::Abandoned),
            todo_item("Use authoritative get_state instead", TodoStatus::InProgress),
        ],
    }])
}

fn todos_active() -> gtk::Widget {
    todo_story(vec![TodoPhase {
        name: "Native todos".to_owned(),
        tasks: vec![
            todo_item("Read protocol definitions", TodoStatus::Completed),
            todo_item("Implement authoritative reconciliation", TodoStatus::InProgress),
            todo_item("Add component stories", TodoStatus::Pending),
        ],
    }])
}

fn todos_long_text() -> gtk::Widget {
    todo_story(vec![TodoPhase {
        name: "Long-form validation and accessibility review".to_owned(),
        tasks: vec![TodoItem {
            content: "Confirm that an unusually long todo description wraps across multiple lines without hiding its state, blocker, or semantically named actions from keyboard and assistive-technology users.".to_owned(),
            status: TodoStatus::Blocked,
            blocker: Some(
                "The external accessibility audit is scheduled after the remaining workspace controls land."
                    .to_owned(),
            ),
        }],
    }])
}

fn todo_story(phases: Vec<TodoPhase>) -> gtk::Widget {
    let panel = todos::TodoPanel::new();
    panel.set_phases(&phases);
    panel.set_expanded(true);
    panel.root.upcast()
}

fn todo_item(content: &str, status: TodoStatus) -> TodoItem {
    TodoItem {
        content: content.to_owned(),
        status,
        blocker: None,
    }
}

fn agent_hub_empty() -> gtk::Widget {
    agent_hub_story(Vec::new(), None, false)
}

fn agent_hub_running() -> gtk::Widget {
    agent_hub_story(
        vec![hub_snapshot(
            "RuntimeBuilder",
            0,
            "task",
            "running",
            "Implement the runtime agent roster",
            Some(json!({
                "id": "RuntimeBuilder",
                "index": 0,
                "agent": "task",
                "agentSource": "project",
                "status": "running",
                "task": "Implement the runtime agent roster",
                "lastIntent": "Building accessible agent rows",
                "currentTool": "edit",
                "recentTools": [],
                "recentOutput": [],
                "toolCount": 7,
                "requests": 4,
                "tokens": 18400,
                "contextTokens": 22300,
                "contextWindow": 272000,
                "cost": 0.061,
                "durationMs": 96000,
                "resolvedModel": "openai-codex/gpt-5.6-sol"
            })),
        )],
        Some("RuntimeBuilder"),
        false,
    )
}

fn agent_hub_tree() -> gtk::Widget {
    let child_progress = json!({
        "id": "MetadataScout",
        "index": 0,
        "agent": "scout",
        "agentSource": "bundled",
        "status": "running",
        "task": "Inspect RPC metadata",
        "lastIntent": "Reading snapshot types",
        "currentTool": "read",
        "recentTools": [],
        "recentOutput": [],
        "toolCount": 3,
        "requests": 2,
        "tokens": 7600,
        "cost": 0.012,
        "durationMs": 44000
    });
    agent_hub_story(
        vec![
            hub_snapshot(
                "HubCoordinator",
                0,
                "task",
                "running",
                "Coordinate the agent hub milestone",
                Some(json!({
                    "id": "HubCoordinator",
                    "index": 0,
                    "agent": "task",
                    "agentSource": "project",
                    "status": "running",
                    "task": "Coordinate the agent hub milestone",
                    "lastIntent": "Delegating RPC research",
                    "currentTool": "task",
                    "recentTools": [],
                    "recentOutput": [],
                    "toolCount": 5,
                    "requests": 3,
                    "tokens": 13100,
                    "cost": 0.035,
                    "durationMs": 72000,
                    "inflightTaskDetails": { "progress": [child_progress.clone()] }
                })),
            ),
            hub_snapshot(
                "MetadataScout",
                0,
                "scout",
                "running",
                "Inspect RPC metadata",
                Some(child_progress),
            ),
        ],
        Some("MetadataScout"),
        false,
    )
}

fn agent_hub_terminal() -> gtk::Widget {
    agent_hub_story(
        vec![
            hub_snapshot(
                "CompletedReviewer",
                0,
                "reviewer",
                "completed",
                "Review the projection",
                None,
            ),
            hub_snapshot(
                "StoppedWorker",
                1,
                "task",
                "aborted",
                "Explore an obsolete approach",
                None,
            ),
        ],
        Some("CompletedReviewer"),
        false,
    )
}

fn agent_hub_failure() -> gtk::Widget {
    agent_hub_story(
        vec![hub_snapshot(
            "ProviderResearcher",
            0,
            "librarian",
            "failed",
            "Verify provider behavior",
            Some(json!({
                "id": "ProviderResearcher",
                "index": 0,
                "agent": "librarian",
                "agentSource": "bundled",
                "status": "failed",
                "task": "Verify provider behavior",
                "recentTools": [],
                "recentOutput": [],
                "toolCount": 2,
                "requests": 3,
                "tokens": 9100,
                "cost": 0.021,
                "durationMs": 81000,
                "retryFailure": {
                    "attempt": 3,
                    "errorMessage": "Provider retry budget exhausted"
                }
            })),
        )],
        Some("ProviderResearcher"),
        false,
    )
}

fn agent_hub_long_task() -> gtk::Widget {
    agent_hub_story(
        vec![hub_snapshot(
            "LongRunningMigration",
            0,
            "task",
            "running",
            "Migrate every runtime projection caller without compatibility shims",
            Some(json!({
                "id": "LongRunningMigration",
                "index": 0,
                "agent": "task",
                "agentSource": "project",
                "status": "running",
                "task": "Migrate every runtime projection caller without compatibility shims",
                "lastIntent": "Updating the final application controller callsites",
                "currentTool": "edit",
                "recentTools": [],
                "recentOutput": [],
                "toolCount": 146,
                "requests": 38,
                "tokens": 487200,
                "contextTokens": 196400,
                "contextWindow": 272000,
                "cost": 2.418,
                "durationMs": 14673000,
                "resolvedModel": "anthropic/claude-opus-4-6"
            })),
        )],
        Some("LongRunningMigration"),
        false,
    )
}

fn agent_hub_transcript() -> gtk::Widget {
    agent_hub_story(
        vec![hub_snapshot(
            "TranscriptWorker",
            0,
            "task",
            "running",
            "Keep the selected transcript current",
            Some(json!({
                "id": "TranscriptWorker",
                "index": 0,
                "agent": "task",
                "agentSource": "bundled",
                "status": "running",
                "task": "Keep the selected transcript current",
                "lastIntent": "Applying an incremental transcript read",
                "currentTool": "read",
                "recentTools": [],
                "recentOutput": [],
                "toolCount": 4,
                "requests": 3,
                "tokens": 14200,
                "cost": 0.048,
                "durationMs": 68000
            })),
        )],
        Some("TranscriptWorker"),
        true,
    )
}

fn agent_hub_story(
    snapshots: Vec<SubagentSnapshot>,
    selected: Option<&str>,
    transcript: bool,
) -> gtk::Widget {
    let mut state = AgentHubState::default();
    state.apply_snapshot(snapshots);
    let view = agent_hub::build();
    view.set_counts(state.active_count(), state.len());
    let mut rendered = Vec::new();
    for row in state.rows() {
        let row = agent_hub::agent_row(&row);
        view.append_row(&row);
        rendered.push(row);
    }
    if let Some(id) = selected
        && let Some(agent) = state.get(id)
    {
        view.show_agent(agent);
        view.select_id(id, &rendered);
        if transcript {
            view.transcript.append_message(
                MessageRole::User,
                "Load the selected agent transcript and keep it current.",
            );
            view.transcript.append_thinking(
                "I’ll read the authoritative transcript cursor, then request another slice when an event arrives.",
                false,
            );
            view.transcript.append_message(
                MessageRole::Assistant,
                "The initial transcript is loaded. New completed messages will append without duplicating earlier entries.",
            );
        } else {
            view.transcript
                .append_notice("Transcript messages have not been loaded in this story.", false);
        }
    }
    view.widget().clone()
}

fn hub_snapshot(
    id: &str,
    index: usize,
    agent: &str,
    status: &str,
    task: &str,
    progress: Option<serde_json::Value>,
) -> SubagentSnapshot {
    let mut value = json!({
        "id": id,
        "index": index,
        "agent": agent,
        "agentSource": "bundled",
        "status": status,
        "task": task,
        "description": task,
        "sessionFile": format!("/tmp/{id}.jsonl"),
        "lastUpdate": 1_786_835_700_000_u64
    });
    if let Some(progress) = progress {
        value["progress"] = progress;
    }
    serde_json::from_value(value).expect("valid agent hub story snapshot")
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

fn branch_picker_linear() -> gtk::Widget {
    branch_picker(vec![
        branch_message(
            "entry-001",
            "Map the current session lifecycle before changing the native UI.",
        ),
        branch_message(
            "entry-014",
            "Add typed RPC structures for the supported session transitions.",
        ),
        branch_message(
            "entry-029",
            "Build the branch picker around stable transcript entry IDs.",
        ),
    ])
}

fn branch_picker_multiple() -> gtk::Widget {
    branch_picker(
        [
            "Inspect the current native session switch flow.",
            "Compare the bridge payloads with omp’s RPC types.",
            "Keep the conversation draft intact when the dialog closes.",
            "Show each user message as a distinct branch point.",
            "Refresh authoritative state only after omp confirms the branch.",
            "Reconcile messages, title, todos, modes, and subagents together.",
            "Add component stories for every picker state.",
            "Cover stable entry ID selection in focused tests.",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, text)| branch_message(&format!("entry-{index:03}"), text))
        .collect(),
    )
}

fn branch_picker_long_content() -> gtk::Widget {
    branch_picker(vec![
        branch_message(
            "entry-long-label",
            "Investigate the complete authoritative session transition lifecycle across the native bridge, current conversation rendering, workspace title, todo panel, interaction modes, and every running subagent before choosing the narrowest maintainable integration point.",
        ),
        branch_message(
            "entry-long-preview",
            "Implement a deliberately long branch candidate preview that spans several visual lines so the picker proves it can wrap useful context, clamp excess content, preserve a readable action target, and still send the opaque transcript entry ID rather than any display text or visible list position.",
        ),
    ])
}

fn branch_picker_empty() -> gtk::Widget {
    branch_picker(Vec::new())
}

fn branch_picker_error() -> gtk::Widget {
    let view = session_actions::BranchPickerView::new(|_| {});
    view.show_error("omp could not load branch points for the current conversation.");
    view.widget().clone()
}

fn branch_picker(messages: Vec<BranchMessage>) -> gtk::Widget {
    let view = session_actions::BranchPickerView::new(|_| {});
    view.set_candidates(messages);
    view.widget().clone()
}

fn branch_message(entry_id: &str, text: &str) -> BranchMessage {
    BranchMessage {
        entry_id: entry_id.to_owned(),
        text: text.to_owned(),
    }
}

fn handoff_default() -> gtk::Widget {
    let view = session_actions::HandoffView::new();
    view.widget().clone()
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
