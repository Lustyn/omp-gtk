use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use gtk::prelude::*;
use gtk4 as gtk;
use serde_json::json;

use super::chat::{ChatStatus, MessageRole, TelemetryWidgets};
use super::conversation::ConversationView;
use super::model_picker::ModelPickerView;
use super::tool_components::ToolActivityGroup;
use super::{agent_hub, composer, session_actions, sidebar, todos};
use crate::agent_hub::AgentHubState;
use crate::bridge::protocol::{
    BranchMessage, ModelSummary, ModelThinking, SubagentSnapshot, TodoItem, TodoPhase, TodoStatus,
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

const STORIES: [Story; 53] = [
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
        id: "conversation/creating",
        title: "Conversation · Creating",
        width: 900,
        height: 620,
        build: conversation_creating,
    },
    Story {
        id: "conversation/disconnected",
        title: "Conversation · Disconnected",
        width: 900,
        height: 620,
        build: conversation_disconnected,
    },
    Story {
        id: "conversation/tool-use",
        title: "Conversation · Tool use",
        width: 900,
        height: 700,
        build: conversation_tool_use,
    },
    Story {
        id: "conversation/outline-track",
        title: "Conversation · Collapsed outline track",
        width: 900,
        height: 720,
        build: conversation_outline_track,
    },
    Story {
        id: "conversation/markdown",
        title: "Conversation · Markdown",
        width: 900,
        height: 720,
        build: conversation_markdown,
    },
    Story {
        id: "conversation/attention-kind",
        title: "Conversation · Attention-kind",
        width: 900,
        height: 520,
        build: conversation_attention_kind,
    },
    Story {
        id: "conversation/streaming-markdown",
        title: "Conversation · Streaming Markdown",
        width: 900,
        height: 620,
        build: conversation_streaming_markdown,
    },
    Story {
        id: "conversation/inline-code-alignment",
        title: "Conversation · Inline code alignment",
        width: 900,
        height: 360,
        build: conversation_inline_code_alignment,
    },
    Story {
        id: "conversation/rich-content-stress",
        title: "Conversation · Rich content stress",
        width: 900,
        height: 720,
        build: conversation_rich_content_stress,
    },
    Story {
        id: "composer/ready",
        title: "Composer · Ready",
        width: 900,
        height: 180,
        build: composer_ready,
    },
    Story {
        id: "composer/session-actions",
        title: "Composer · Continue actions",
        width: 900,
        height: 180,
        build: composer_session_actions,
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
        title: "Todos · Hidden when empty",
        width: 900,
        height: 180,
        build: todos_empty,
    },
    Story {
        id: "todos/multi-phase",
        title: "Todos · Expanded plan",
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
        title: "Todos · Compact active task",
        width: 900,
        height: 180,
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
        id: "branch-picker/loading",
        title: "Branch picker · Loading messages",
        width: 720,
        height: 640,
        build: branch_picker_loading,
    },
    Story {
        id: "branch-picker/linear",
        title: "Branch picker · Independent conversation",
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
        id: "branch-picker/committing",
        title: "Branch picker · Creating conversation",
        width: 720,
        height: 640,
        build: branch_picker_committing,
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
        title: "Handoff · Summarize and continue",
        width: 660,
        height: 520,
        build: handoff_default,
    },
    Story {
        id: "tool-group/running",
        title: "Tool activity · Running",
        width: 780,
        height: 250,
        build: tool_group_running,
    },
    Story {
        id: "tool-group/hub-process",
        title: "Tool activity · Managed process",
        width: 780,
        height: 250,
        build: tool_group_hub_process,
    },
    Story {
        id: "tool-group/completed",
        title: "Tool activity · Completed",
        width: 780,
        height: 250,
        build: tool_group_completed,
    },
    Story {
        id: "tool-group/read-image",
        title: "Tool activity · Read image",
        width: 780,
        height: 620,
        build: tool_group_read_image,
    },
    Story {
        id: "tool-group/error",
        title: "Tool activity · Error",
        width: 780,
        height: 300,
        build: tool_group_error,
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
    let telemetry = TelemetryWidgets::new("~/code/omp-gtk");
    telemetry.set_context(14_200, 272_000, 5.2);
    telemetry.set_cost(0.024);
    telemetry.set_throughput((state == "working").then_some(86.4));
    root.append(&row);
    root.append(&telemetry.root);
    root.upcast()
}

fn sidebar_sessions() -> gtk::Widget {
    let view = sidebar::build();
    view.active_count.set_text("1 active");
    view.active_count.add_css_class("has-items");
    view.unread_count.set_text("2 unread");
    view.unread_count.add_css_class("has-items");
    for entry in [
        SessionEntry {
            path: Some(PathBuf::from("/tmp/current.jsonl")),
            title: "Refactor native component architecture".to_owned(),
            subtitle: "omp-gtk · 42 messages · Just now".to_owned(),
            cwd: Some(PathBuf::from("/home/agent/code/omp-gtk")),
            created_at: SystemTime::UNIX_EPOCH,
            current: true,
            running: false,
            runtime_id: None,
        },
        SessionEntry {
            path: Some(PathBuf::from("/tmp/accessibility.jsonl")),
            title: "Improve accessible component selectors".to_owned(),
            subtitle: "omp-gtk · 16 messages · 18m ago".to_owned(),
            cwd: Some(PathBuf::from("/home/agent/code/omp-gtk")),
            created_at: SystemTime::UNIX_EPOCH,
            current: false,
            running: true,
            runtime_id: None,
        },
        SessionEntry {
            path: Some(PathBuf::from("/tmp/release.jsonl")),
            title: "Prepare the release and verify packaging".to_owned(),
            subtitle: "desktop-client · 8 messages · 2h ago".to_owned(),
            cwd: Some(PathBuf::from("/home/agent/code/desktop-client")),
            created_at: SystemTime::UNIX_EPOCH,
            current: false,
            running: false,
            runtime_id: None,
        },
    ] {
        let session = sidebar::session_row(entry);
        if session.entry.title.starts_with("Prepare the release") {
            session.row.add_css_class("unread-session");
            session.indicator.queue_draw();
        }
        view.list.append(&session.row);
    }
    view.root.upcast()
}

fn conversation_empty() -> gtk::Widget {
    let view = ConversationView::main();
    let selected_view = view.clone();
    view.show_workspace_onboarding(
        &[
            PathBuf::from("/home/agent/code/omp-gtk"),
            PathBuf::from("/home/agent/code/desktop-client"),
            PathBuf::from("/home/agent/code/service-api"),
        ],
        Some(std::path::Path::new("/home/agent")),
        move |path| {
            selected_view.show_loading(
                "Opening selected folder",
                &path.to_string_lossy(),
                "Changing workspace",
            );
        },
        || {},
    );
    view.widget().clone()
}

fn conversation_creating() -> gtk::Widget {
    let view = ConversationView::main();
    view.show_loading(
        "Creating a new conversation",
        "Preparing a fresh omp session in this workspace.",
        "Starting the local runtime",
    );
    view.widget().clone()
}

fn conversation_disconnected() -> gtk::Widget {
    let view = ConversationView::main();
    view.show_disconnected(
        "The local runtime stopped before a connection could be established. Check your omp installation and try again.",
    );
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
    let group = ToolActivityGroup::new();
    group.ensure_card(
        "read-composer",
        "read",
        &json!({"path": "src/ui/composer.rs:1-220"}),
        Some("Inspecting composer component"),
    );
    group.complete("read-composer", &json!({"text": "Loaded 220 lines"}), false);
    group.finish();
    view.append(&group.root);
    view.append_message(
        MessageRole::Assistant,
        "The composer now owns its widget state and exposes behavior-oriented methods.",
    );
    view.widget().clone()
}
fn conversation_outline_track() -> gtk::Widget {
    let view = ConversationView::main();
    view.append_message(
        MessageRole::Assistant,
        r#"# Deployment guide

## Prepare the release workspace
Confirm the selected branch, review pending changes, and keep the release inputs together.

## Back up persisted state
Capture the current data before changing schemas or replacing deployed binaries.

## Install the new build
Stage the release artifact and verify its checksum before switching the active version.

## Migrate stored data
Run the supported migration path once, then preserve its output for the release record.

## Restart background services
Restart each dependent service in dependency order and wait for observable readiness.

## Verify user-facing behavior
Exercise the primary workflow, error state, and recovery path against the live build.

## Review runtime health
Check resource usage and recent errors after traffic reaches the updated process.

## Complete the release
Record the deployed revision and retain the rollback inputs until the release is stable."#,
    );
    view.widget().clone()
}

fn conversation_markdown() -> gtk::Widget {
    let view = ConversationView::main();
    view.append_message(
        MessageRole::Assistant,
        r#"# Rich Markdown

Messages support **strong emphasis**, *italics*, `inline code`, and native math such as $E = mc^2$.

## Compact blocks
This paragraph starts directly after its heading without running into it.

> Quoted guidance stays muted and compact.

- List markers use omp's accent color.
- Adjacent items stay on separate lines.

$$\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$$

---

## Diagrams, code, and structured native output without truncated heading text

```mermaid
flowchart LR
    Prompt --> Parse{Block type}
    Parse -->|Math| Formula[LaTeX]
    Parse -->|Diagram| Mermaid
    Formula --> GTK
    Mermaid --> GTK
```

### Structured output
Tables and formulas become rich widgets as soon as their Markdown is complete.

| Renderer | Output | Status |
| :--- | :---: | ---: |
| LaTeX | Vector formula | Ready |
| Mermaid | Native SVG | Ready |

```rust
fn render(markdown: &str) {
    println!("{markdown}");
}
```"#,
    );
    view.set_outline_revealed(true);
    view.widget().clone()
}

fn conversation_attention_kind() -> gtk::Widget {
    let view = ConversationView::main();
    view.append_message(
        MessageRole::Assistant,
        r#"**→ The release is ready.** All focused checks passed, and the deployment can proceed.

**1 → Scope.** The renderer colors only attention-kind lead-ins at the start of a paragraph.

**2 → Safety.** Ordinary **strong emphasis** keeps the standard text color.

**3 → Next action.** Deploy the tested revision, then watch the first production request."#,
    );
    view.widget().clone()
}
fn conversation_streaming_markdown() -> gtk::Widget {
    let view = ConversationView::main();
    let source = r#"# Streaming Markdown

Completed blocks stay formatted while the active block grows.

## Live formatting
The parser keeps **unfinished emphasis readable and renders `inline code

```rust
fn render_stream(delta: &str) {
    push(delta);"#;
    let body = view.append_streaming_message(MessageRole::Assistant, source);
    body.set_streaming_text(source);
    view.widget().clone()
}

fn conversation_inline_code_alignment() -> gtk::Widget {
    let view = ConversationView::main();
    let relaxed = view.append_message(
        MessageRole::Assistant,
        "Relaxed line height keeps `inline code` on the text baseline.",
    );
    relaxed.add_css_class("story-line-height-relaxed");
    view.append_message(
        MessageRole::Assistant,
        "Default line height keeps `inline code` on the text baseline.",
    );
    view.append_message(
        MessageRole::Assistant,
        "Font metrics, not a fixed offset, position each `code pill`.",
    );
    view.widget().clone()
}
fn conversation_rich_content_stress() -> gtk::Widget {
    let view = ConversationView::main();
    view.append_message(
        MessageRole::Assistant,
        r#"## Concurrent agent pipeline

```mermaid
flowchart LR
    U[User prompt] --> R[RPC bridge]
    R --> P{Classify content}
    P -->|Plain text| M[Markdown parser]
    P -->|Tool request| T[Tool dispatcher]
    T --> S[Subagent task]
    S --> H[Agent hub]
    H --> A1[Research agent]
    H --> A2[Implementation agent]
    H --> A3[Review agent]
    A1 --> J[Aggregate results]
    A2 --> J
    A3 --> J
    J --> B{Renderable block}
    B --> L[Native formula SVG]
    B --> D[Native diagram SVG]
    B --> G[GTK table grid]
    B --> W[Wrapped text]
    L --> C[Conversation view]
    D --> C
    G --> C
    W --> C
    style A1 fill:#ffcc00,color:#ffff00,stroke:#ffcc00
    style A2 fill:#050505,color:#111111,stroke:#050505
    linkStyle 6 stroke:#111111,color:#111111
```

$$
\begin{aligned}
\mathcal{L}(x,\lambda,\mu) &= f(x) + \sum_{i=1}^{m}\lambda_i h_i(x)
  + \sum_{j=1}^{p}\mu_j g_j(x), \\
\nabla_x \mathcal{L}(x^\star,\lambda^\star,\mu^\star) &= 0, \\
h_i(x^\star) &= 0,\quad g_j(x^\star) \le 0,\quad \mu_j^\star \ge 0
\end{aligned}
$$

\[
A=
\begin{pmatrix}
4 & 12 & -16 \\
12 & 37 & -43 \\
-16 & -43 & 98
\end{pmatrix}
\]"#,
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

fn composer_session_actions() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_attachment_sensitive(true);
    view.set_model("openai-codex", "GPT-5.6-Sol");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view.set_session_actions_visible(true);
    view.set_primary_action(true, false);
    view.widget().clone()
}

fn composer_running_empty() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_model("anthropic", "Claude Opus 4.6");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
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
    view.set_primary_action(true, true);
    view.widget().clone()
}

fn composer_queued() -> gtk::Widget {
    let view = composer::build();
    view.set_input_sensitive(true);
    view.set_model("openai-codex", "GPT-5.6-Sol");
    view.set_thinking_sensitive(true);
    view.set_thinking_label("High");
    view.set_queued_message_count(3);
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

fn workspace_story(child: &gtk::Widget) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("workspace");
    root.append(child);
    root.upcast()
}

fn story_attachment_texture() -> gtk::gdk::Texture {
    const HIGH_RESOLUTION_IMAGE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="3840" height="2160" viewBox="0 0 3840 2160">
<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#5ad8e6"/><stop offset="1" stop-color="#9b4dff"/></linearGradient></defs>
<rect width="3840" height="2160" fill="#14181d"/><circle cx="1920" cy="1080" r="720" fill="url(#g)"/>
</svg>"##;
    gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from_static(HIGH_RESOLUTION_IMAGE))
        .expect("story attachment is a valid high-resolution image")
}

fn append_story_attachment(view: &composer::ComposerView, id: u64, name: &str) {
    view.append_attachment_preview(id, name, &story_attachment_texture(), |_| {});
}

fn composer_attachments_empty() -> gtk::Widget {
    let view = attachment_story_composer();
    view.set_primary_action(true, false);
    workspace_story(view.widget())
}

fn composer_attachments_populated() -> gtk::Widget {
    let view = attachment_story_composer();
    view.set_text("Describe what is shown");
    append_story_attachment(&view, 1, "architecture.png");
    view.set_primary_action(true, false);
    workspace_story(view.widget())
}

fn composer_attachments_multiple() -> gtk::Widget {
    let view = attachment_story_composer();
    view.set_text("Compare these in order");
    append_story_attachment(&view, 1, "first.png");
    append_story_attachment(&view, 2, "second.jpg");
    append_story_attachment(&view, 3, "third.png");
    view.set_primary_action(true, false);
    workspace_story(view.widget())
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
    root.add_css_class("workspace");
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
            todo_item(
                "Parse todo state from transcript prose",
                TodoStatus::Abandoned,
            ),
            todo_item(
                "Use authoritative get_state instead",
                TodoStatus::InProgress,
            ),
        ],
    }])
}

fn todos_active() -> gtk::Widget {
    todo_story(vec![TodoPhase {
        name: "Native todos".to_owned(),
        tasks: vec![
            todo_item("Read protocol definitions", TodoStatus::Completed),
            todo_item(
                "Implement authoritative reconciliation",
                TodoStatus::InProgress,
            ),
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
    panel.set_revealed(true);
    workspace_story(panel.root.upcast_ref())
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
    let mut snapshots = vec![
        hub_snapshot(
            "CompletedReviewer",
            0,
            "reviewer",
            "completed",
            "Review the projection",
            Some(json!({
                "id": "CompletedReviewer",
                "index": 0,
                "agent": "reviewer",
                "agentSource": "project",
                "status": "completed",
                "task": "Review the projection",
                "recentTools": [],
                "recentOutput": [],
                "toolCount": 8,
                "requests": 5,
                "tokens": 24600,
                "cost": 0.084,
                "durationMs": 132000,
                "resolvedModel": "openai-codex/gpt-5.6-sol"
            })),
        ),
        hub_snapshot(
            "StoppedWorker",
            1,
            "task",
            "aborted",
            "Explore an obsolete approach",
            None,
        ),
    ];
    for snapshot in &mut snapshots {
        snapshot.historical = true;
    }
    agent_hub_story(snapshots, Some("CompletedReviewer"), true)
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

    let pairs = state
        .rows()
        .into_iter()
        .map(|tree_row| {
            let rendered = agent_hub::agent_row(&tree_row, true);
            view.append_row(&rendered);
            (tree_row, rendered)
        })
        .collect::<Vec<_>>();
    let pairs = Rc::new(pairs);

    let session_title_text = if transcript {
        selected
            .and_then(|id| state.get(id))
            .map(|agent| format!("Agent · {}", agent.display_name()))
            .unwrap_or_else(|| "Agent transcript".to_owned())
    } else {
        "Main conversation".to_owned()
    };
    let session_title = gtk::Label::new(Some(&session_title_text));
    session_title.set_xalign(0.0);
    session_title.set_margin_top(18);
    session_title.set_margin_start(22);
    session_title.add_css_class("chat-title");

    for (tree_row, rendered) in pairs.iter() {
        let id = tree_row.agent.id.clone();
        let name = tree_row.agent.display_name();
        let title = session_title.clone();
        let view_for_open = view.clone();
        let pairs_for_open = pairs.clone();
        rendered.open.connect_clicked(move |_| {
            title.set_text(&format!("Agent · {name}"));
            let rows = pairs_for_open
                .iter()
                .map(|(_, row)| row.clone())
                .collect::<Vec<_>>();
            view_for_open.select_id(&id, &rows);
        });

        if let Some(expander) = &rendered.expander {
            let pairs_for_toggle = pairs.clone();
            expander.connect_toggled(move |_| {
                for (candidate, candidate_row) in pairs_for_toggle.iter() {
                    let mut parent = candidate.parent_id.as_deref();
                    let mut visible = true;
                    while let Some(parent_id) = parent {
                        let Some((parent_tree, parent_row)) = pairs_for_toggle
                            .iter()
                            .find(|(tree, _)| tree.agent.id == parent_id)
                        else {
                            break;
                        };
                        if parent_row
                            .expander
                            .as_ref()
                            .is_some_and(|expander| !expander.is_active())
                        {
                            visible = false;
                            break;
                        }
                        parent = parent_tree.parent_id.as_deref();
                    }
                    candidate_row.root.set_visible(visible);
                }
            });
        }
    }

    if let Some(id) = selected {
        let rows = pairs.iter().map(|(_, row)| row.clone()).collect::<Vec<_>>();
        view.select_id(id, &rows);
    }
    view.set_revealed(!state.is_empty());

    let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
    center.append(&session_title);
    if transcript {
        let transcript_view = ConversationView::transcript();
        transcript_view.append_message(
            MessageRole::User,
            "Load the selected agent transcript and keep it current.",
        );
        transcript_view.append_thinking(
            "I’ll read the authoritative transcript cursor, then request another slice when an event arrives.",
            false,
        );
        transcript_view.append_message(
            MessageRole::Assistant,
            "The initial transcript is loaded. New completed messages append without duplicating earlier entries.",
        );
        center.append(transcript_view.widget());
    } else {
        let placeholder = gtk::Label::new(Some(
            "Agent transcripts open here without replacing the right-side session tree.",
        ));
        placeholder.set_wrap(true);
        placeholder.set_halign(gtk::Align::Center);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_vexpand(true);
        placeholder.add_css_class("conversation-hero-detail");
        center.append(&placeholder);
    }

    let todo = todos::TodoPanel::new();
    todo.set_phases(&[TodoPhase {
        name: "Implementation".to_owned(),
        tasks: vec![TodoItem {
            content: "Overhaul the agent hub side pane".to_owned(),
            status: TodoStatus::InProgress,
            blocker: None,
        }],
    }]);
    let side_panes = gtk::Box::new(gtk::Orientation::Vertical, 8);
    side_panes.set_halign(gtk::Align::End);
    side_panes.set_valign(gtk::Align::Center);
    side_panes.append(view.widget());
    side_panes.append(&todo.root);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&center));
    overlay.add_overlay(&side_panes);
    workspace_story(overlay.upcast_ref())
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
    for level in ["off", "minimal", "low", "medium", "high", "xhigh", "max"] {
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

fn branch_picker_loading() -> gtk::Widget {
    let view = session_actions::BranchPickerView::new(|_| {});
    view.widget().clone()
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

fn branch_picker_committing() -> gtk::Widget {
    let view = session_actions::BranchPickerView::new(|_| {});
    view.show_branching();
    view.widget().clone()
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

fn tool_group_running() -> gtk::Widget {
    let group = ToolActivityGroup::new();
    group.set_working(true);
    for (id, name, args, intent) in [
        (
            "read",
            "read",
            json!({"path": "src/ui/composer.rs:1-220"}),
            "Inspecting composer ownership",
        ),
        (
            "glob",
            "glob",
            json!({"path": "src/ui/**/*.rs"}),
            "Mapping UI components",
        ),
        (
            "grep",
            "grep",
            json!({"pattern": "ToolCard", "path": "src"}),
            "Finding tool rendering call sites",
        ),
        (
            "edit",
            "edit",
            json!({"input": "[src/ui/tool_components.rs#ABCD]"}),
            "Grouping tool activity",
        ),
        (
            "bash",
            "bash",
            json!({"command": "cargo test tool_components"}),
            "Running component tests",
        ),
    ] {
        group.ensure_card(id, name, &args, Some(intent));
        if id != "bash" {
            group.complete(id, &json!({"text": "Done"}), false);
        }
    }
    group.root.clone().upcast()
}

fn tool_group_hub_process() -> gtk::Widget {
    let group = ToolActivityGroup::new();
    for (id, args, intent) in [
        (
            "start",
            json!({
                "op": "start",
                "name": "stage-resume-app",
                "application": "git",
                "args": ["add", "-p", "src/app.rs"]
            }),
            "Staging resume controller hunks",
        ),
        (
            "logs",
            json!({"op": "logs", "name": "stage-resume-app", "lines": 80}),
            "Reading staging prompt",
        ),
        (
            "send",
            json!({"op": "send", "name": "stage-resume-app", "text": "y"}),
            "Selecting resume import",
        ),
    ] {
        group.ensure_card(id, "hub", &args, Some(intent));
        group.complete(id, &json!({"text": "Done"}), false);
    }
    group.finish();
    group.root.clone().upcast()
}

fn tool_group_completed() -> gtk::Widget {
    let group = ToolActivityGroup::new();
    group.append_thinking(
        "I’ll inspect the current activity flow before updating the renderer.",
        false,
    );
    group.ensure_card(
        "write",
        "write",
        &json!({"path": "src/ui/gallery.rs", "content": "..."}),
        Some("Creating native gallery"),
    );
    group.complete(
        "write",
        &json!({"text": "Successfully wrote 180 lines"}),
        false,
    );
    group.ensure_card(
        "check",
        "bash",
        &json!({"command": "cargo check"}),
        Some("Checking the grouped display"),
    );
    group.complete("check", &json!({"text": "Finished"}), false);
    group.append_notice(
        "<system-reminder>\n2 todos remain. Continue working.\n</system-reminder>",
        false,
    );
    group.finish();
    group.root.clone().upcast()
}

fn tool_group_read_image() -> gtk::Widget {
    let group = ToolActivityGroup::new();
    group.ensure_card(
        "read-image",
        "read",
        &json!({"path": "src/assets/omp.svg"}),
        Some("Reading application artwork"),
    );
    group.complete(
        "read-image",
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
    group.finish();
    group.root.clone().upcast()
}

fn tool_group_error() -> gtk::Widget {
    let group = ToolActivityGroup::new();
    group.ensure_card(
        "check",
        "bash",
        &json!({"command": "cargo check"}),
        Some("Checking component boundary"),
    );
    group.complete(
        "check",
        &json!({"error": "no field `input` on type `WorkspaceView`"}),
        true,
    );
    group.finish();
    group.root.clone().upcast()
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
