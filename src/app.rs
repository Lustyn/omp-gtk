use crate::alerts::{
    self, AlertKind, Alerts, Preferences, SoundEvent, SoundPackChoice, WindowStatus,
};
use crate::sound_registry::{self, RegistryPack};
use crate::ui::{
    agent_hub as agent_hub_ui,
    attachments::{self, AttachmentId, ComposerAttachments},
    chat, composer,
    conversation::ConversationView,
    model_picker, session_actions, sidebar, sound_settings, todos, tool_components, workspace,
};

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use serde_json::{Value, json};

use crate::agent_hub::AgentHubState;
use crate::bridge::protocol::{
    BranchMessagesResponse, BranchResponse, HandoffResponse, InterruptMode, ModelSummary,
    QueueMode, RpcEvent, RpcResponse, SessionState, SlashCommand, SubagentMessages,
    SubagentSnapshot, SubagentUpdate, TodoPhase, ToolEnd, ToolStart, ToolUpdate, message_cost,
    message_role, message_text, message_thinking, message_tool_calls, tool_result_parts,
};
use crate::bridge::{BridgeClient, MANUAL_CONTINUE_PROMPT, OmpBridge};
use crate::commands::{CommandCompletion, completions, unsupported_native_mode_error};
use crate::session_catalog::{self, SessionEntry};
use chat::{MessageBody, MessageRole, ThinkingBlock};
use sidebar::SessionRow;
use todos::validate_phases;
use tool_components::{ToolActivityGroup, ToolCard};
use workspace::WorkspaceView;

const TASK_PROGRESS_INTERVAL: Duration = Duration::from_secs(30);
const PROMPT_BURST_WINDOW: Duration = Duration::from_secs(5);
const PROMPT_BURST_THRESHOLD: usize = 3;
const WARM_SESSION_VIEW_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolResultSource {
    Live,
    History,
}

impl ToolResultSource {
    const fn applies_live_effects(self) -> bool {
        matches!(self, Self::Live)
    }
}

fn should_finish_hydrated_tool_group(is_streaming: bool) -> bool {
    !is_streaming
}

fn has_visible_text(text: &str) -> bool {
    !text.trim().is_empty()
}

fn is_resume_shortcut(message: &str) -> bool {
    message == "."
}

fn is_hidden_resume_message(message: &Value) -> bool {
    matches!(message_role(message), Some("user" | "developer"))
        && message_text(message) == MANUAL_CONTINUE_PROMPT
}

fn completion_is_unread(
    was_running: bool,
    active_runtime: Option<u64>,
    completed_runtime: u64,
    window_active: bool,
) -> bool {
    was_running && (active_runtime != Some(completed_runtime) || !window_active)
}

fn cloned_or_insert_with<T: Clone>(slot: &RefCell<Option<T>>, create: impl FnOnce() -> T) -> T {
    let existing = slot.borrow().clone();
    if let Some(existing) = existing {
        return existing;
    }
    let value = create();
    slot.replace(Some(value.clone()));
    value
}
fn touch_bounded_lru(order: &mut VecDeque<u64>, value: u64, limit: usize) -> Vec<u64> {
    order.retain(|existing| *existing != value);
    order.push_back(value);
    let mut evicted = Vec::new();
    while order.len() > limit {
        if let Some(value) = order.pop_front() {
            evicted.push(value);
        }
    }
    evicted
}
fn identifies_same_session(left: &SessionEntry, right: &SessionEntry) -> bool {
    left.runtime_id
        .zip(right.runtime_id)
        .is_some_and(|(left, right)| left == right)
        || left
            .path
            .as_ref()
            .zip(right.path.as_ref())
            .is_some_and(|(left, right)| left == right)
}
fn messages_need_hydration(
    cached: Option<&[Value]>,
    view_hydrated: bool,
    messages: &[Value],
) -> bool {
    !view_hydrated || cached != Some(messages)
}

pub(crate) fn build(app: &adw::Application) {
    let initial_runtime_id = 1;
    let ui = workspace::build(app, initial_runtime_id);
    let initial_conversation = ui.conversation();
    let (runtimes, active_runtime, events) = match OmpBridge::spawn() {
        Ok(bridge) => {
            let events = bridge.events.clone();
            let mut runtimes = HashMap::new();
            runtimes.insert(
                initial_runtime_id,
                SessionRuntime {
                    bridge,
                    running: false,
                    ready: false,
                    delivery_target: None,
                    disconnected: None,
                    workspace: std::env::current_dir().ok(),
                    state: None,
                },
            );
            (runtimes, Some(initial_runtime_id), Some(events))
        }
        Err(error) => {
            ui.composer.set_input_sensitive(false);
            ui.conversation()
                .show_disconnected(&format!("The local runtime could not be started. {error}"));
            (HashMap::new(), None, None)
        }
    };

    let alerts = Alerts::new(app);
    let present_action = gio::SimpleAction::new("present", None);
    let window = ui.window.clone();
    present_action.connect_activate(move |_, _| window.present());
    app.add_action(&present_action);

    let recent_sessions = session_catalog::load_recent_sessions();
    let controller = Rc::new(AppController {
        ui,
        alerts,
        runtimes: RefCell::new(runtimes),
        active_runtime: Cell::new(active_runtime),
        next_runtime_id: Cell::new(initial_runtime_id + 1),
        models: RefCell::new(Vec::new()),
        commands: RefCell::new(Vec::new()),
        thinking_levels: RefCell::new(Vec::new()),
        thinking_buttons: RefCell::new(Vec::new()),
        completion_items: RefCell::new(Vec::new()),
        completion_index: Cell::new(0),
        pending_user_messages: RefCell::new(VecDeque::new()),
        streaming_message: RefCell::new(None),
        attachments: RefCell::new(ComposerAttachments::default()),
        pending_submissions: RefCell::new(VecDeque::new()),
        pasted_image_count: Cell::new(0),
        streaming_thinking: RefCell::new(None),
        active_tool_group: RefCell::new(None),
        tool_groups_by_call_id: RefCell::new(HashMap::new()),
        agent_hub: RefCell::new(AgentHubState::default()),
        agent_hub_rows: RefCell::new(Vec::new()),
        collapsed_agents: RefCell::new(HashSet::new()),
        subagent_transcript: RefCell::new(None),
        subagent_active_tool_group: RefCell::new(None),
        subagent_tool_groups_by_call_id: RefCell::new(HashMap::new()),
        session_rows: RefCell::new(Vec::new()),
        active_sessions: RefCell::new(recent_sessions),
        conversation_views: RefCell::new(HashMap::from([(
            initial_runtime_id,
            WarmConversation {
                view: initial_conversation,
                messages: None,
                hydrated: false,
            },
        )])),
        conversation_view_lru: RefCell::new(VecDeque::from([initial_runtime_id])),
        unread_runtimes: RefCell::new(HashSet::new()),
        current_session_file: RefCell::new(None),
        hydrated_agent_session: RefCell::new(None),
        current_session_title: RefCell::new("New conversation".to_owned()),
        pending_session_notice: RefCell::new(None),
        branch_picker: RefCell::new(None),
        branch_busy: Cell::new(false),
        handoff_busy: Cell::new(false),
        active_subagent: RefCell::new(None),
        session_cost: Cell::new(0.0),
        extension_widgets: RefCell::new(HashMap::new()),
        extension_statuses: RefCell::new(HashMap::new()),
        extension_dialogs: RefCell::new(HashMap::new()),
        current_model: RefCell::new(None),
        ready: Cell::new(false),
        running: Cell::new(false),
        queued_message_count: Cell::new(0),
        goal_completion_calls: RefCell::new(HashSet::new()),
        goal_completed_this_run: Cell::new(false),
        window_status: Cell::new(WindowStatus::Ready),
        session_sound_started: Cell::new(false),
        last_task_progress: Cell::new(None),
        recent_prompts: RefCell::new(VecDeque::new()),
    });

    controller.wire_interactions();
    controller.render_session_sidebar(controller.active_sessions.borrow().clone());
    controller.set_window_status(WindowStatus::Ready);
    if let (Some(runtime_id), Some(events)) = (active_runtime, events) {
        AppController::run_event_loop(&controller, runtime_id, events);
    }
    let weak = Rc::downgrade(&controller);
    glib::timeout_add_local(Duration::from_millis(750), move || {
        let Some(controller) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        controller.refresh_titles_from_disk();
        controller.tick_sound_events();
        glib::ControlFlow::Continue
    });
    controller.ui.window.present();
    eprintln!("omp native bridge UI ready");
}

struct StreamingMessage {
    body: MessageBody,
    text: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmissionIntent {
    Default,
    FollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmissionAction {
    Prompt,
    Steer,
    FollowUp,
}

impl SubmissionAction {
    fn select(running: bool, intent: SubmissionIntent) -> Self {
        if !running {
            return Self::Prompt;
        }
        match intent {
            SubmissionIntent::Default => Self::Steer,
            SubmissionIntent::FollowUp => Self::FollowUp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerKeyAction {
    Submit,
    FollowUp,
    Newline,
}

fn composer_key_action(key: gdk::Key, modifiers: gdk::ModifierType) -> Option<ComposerKeyAction> {
    let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
    if key == gdk::Key::q && control {
        return Some(ComposerKeyAction::FollowUp);
    }
    if !matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
        return None;
    }
    if modifiers.intersects(gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::ALT_MASK) {
        return Some(ComposerKeyAction::Newline);
    }
    Some(if control {
        ComposerKeyAction::FollowUp
    } else {
        ComposerKeyAction::Submit
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeliveryModes {
    steering_mode: QueueMode,
    follow_up_mode: QueueMode,
    interrupt_mode: InterruptMode,
}

impl From<&Preferences> for DeliveryModes {
    fn from(preferences: &Preferences) -> Self {
        Self {
            steering_mode: preferences.steering_mode,
            follow_up_mode: preferences.follow_up_mode,
            interrupt_mode: preferences.interrupt_mode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeliveryPreferenceUpdates {
    steering_mode: Option<QueueMode>,
    follow_up_mode: Option<QueueMode>,
    interrupt_mode: Option<InterruptMode>,
}
impl DeliveryPreferenceUpdates {
    fn is_empty(self) -> bool {
        self.steering_mode.is_none()
            && self.follow_up_mode.is_none()
            && self.interrupt_mode.is_none()
    }
}
fn should_send_delivery_preference_updates(
    delivery_target: Option<DeliveryModes>,
    desired: DeliveryModes,
    updates: DeliveryPreferenceUpdates,
) -> bool {
    !updates.is_empty() && delivery_target != Some(desired)
}

fn delivery_preference_updates(
    preferences: &Preferences,
    state: &SessionState,
) -> DeliveryPreferenceUpdates {
    DeliveryPreferenceUpdates {
        steering_mode: (state.steering_mode != preferences.steering_mode)
            .then_some(preferences.steering_mode),
        follow_up_mode: (state.follow_up_mode != preferences.follow_up_mode)
            .then_some(preferences.follow_up_mode),
        interrupt_mode: (state.interrupt_mode != preferences.interrupt_mode)
            .then_some(preferences.interrupt_mode),
    }
}

fn send_delivery_preference_updates(
    client: &BridgeClient,
    updates: DeliveryPreferenceUpdates,
) -> Result<(), crate::bridge::BridgeError> {
    let mut first_error = None;
    for result in [
        updates
            .steering_mode
            .map(|mode| client.set_steering_mode(mode)),
        updates
            .follow_up_mode
            .map(|mode| client.set_follow_up_mode(mode)),
        updates
            .interrupt_mode
            .map(|mode| client.set_interrupt_mode(mode)),
    ]
    .into_iter()
    .flatten()
    {
        if let Err(error) = result
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}
fn session_actions_are_relevant(
    ready: bool,
    persisted: bool,
    has_messages: bool,
    running: bool,
    session_busy: bool,
    subagent_open: bool,
    chat_visible: bool,
) -> bool {
    ready
        && persisted
        && has_messages
        && !running
        && !session_busy
        && !subagent_open
        && chat_visible
}

struct PendingSubmission {
    request_id: String,
    draft_text: String,
    message: String,
    displayed_message: Option<MessageBody>,
    attachment_ids: Vec<AttachmentId>,
}

struct SubagentTranscriptState {
    id: String,
    next_byte: u64,
    request_id: Option<String>,
    pending_refresh: bool,
    has_content: bool,
}
struct SessionRuntime {
    bridge: OmpBridge,
    running: bool,
    ready: bool,
    disconnected: Option<String>,
    delivery_target: Option<DeliveryModes>,
    workspace: Option<PathBuf>,
    state: Option<SessionState>,
}
// Retains the rendered widget tree, scroll position, and matching RPC snapshot as one LRU entry.
struct WarmConversation {
    view: ConversationView,
    messages: Option<Vec<Value>>,
    hydrated: bool,
}

fn insert_session_by_creation(entries: &mut Vec<SessionEntry>, entry: SessionEntry) {
    let position = entries
        .iter()
        .position(|existing| {
            (entry.created_at, entry.runtime_id) > (existing.created_at, existing.runtime_id)
        })
        .unwrap_or(entries.len());
    entries.insert(position, entry);
}

fn update_current_session(entries: &mut Vec<SessionEntry>, mut current: SessionEntry) {
    for entry in entries.iter_mut() {
        entry.current = false;
    }
    if let Some(position) = entries
        .iter()
        .position(|entry| identifies_same_session(entry, &current))
    {
        current.created_at = entries[position].created_at;
        entries[position] = current;
    } else {
        insert_session_by_creation(entries, current);
    }
}

fn reorder_session_entries(
    entries: &mut Vec<SessionEntry>,
    source_runtime_id: u64,
    target_runtime_id: u64,
    insert_after: bool,
) -> bool {
    if source_runtime_id == target_runtime_id {
        return false;
    }
    let Some(source_position) = entries
        .iter()
        .position(|entry| entry.runtime_id == Some(source_runtime_id))
    else {
        return false;
    };
    let source = entries.remove(source_position);
    let Some(target_position) = entries
        .iter()
        .position(|entry| entry.runtime_id == Some(target_runtime_id))
    else {
        entries.insert(source_position, source);
        return false;
    };
    let destination = target_position + usize::from(insert_after);
    entries.insert(destination, source);
    destination != source_position
}

struct AppController {
    ui: WorkspaceView,
    alerts: Alerts,
    runtimes: RefCell<HashMap<u64, SessionRuntime>>,
    active_runtime: Cell<Option<u64>>,
    next_runtime_id: Cell<u64>,
    models: RefCell<Vec<ModelSummary>>,
    commands: RefCell<Vec<SlashCommand>>,
    thinking_levels: RefCell<Vec<String>>,
    thinking_buttons: RefCell<Vec<gtk::Button>>,
    completion_items: RefCell<Vec<CommandCompletion>>,
    completion_index: Cell<usize>,
    pending_user_messages: RefCell<VecDeque<String>>,
    streaming_message: RefCell<Option<StreamingMessage>>,
    attachments: RefCell<ComposerAttachments>,
    unread_runtimes: RefCell<HashSet<u64>>,
    pending_submissions: RefCell<VecDeque<PendingSubmission>>,
    pasted_image_count: Cell<u64>,
    streaming_thinking: RefCell<Option<ThinkingBlock>>,
    active_tool_group: RefCell<Option<ToolActivityGroup>>,
    tool_groups_by_call_id: RefCell<HashMap<String, ToolActivityGroup>>,
    agent_hub: RefCell<AgentHubState>,
    agent_hub_rows: RefCell<Vec<agent_hub_ui::AgentHubRow>>,
    collapsed_agents: RefCell<HashSet<String>>,
    subagent_transcript: RefCell<Option<SubagentTranscriptState>>,
    subagent_active_tool_group: RefCell<Option<ToolActivityGroup>>,
    subagent_tool_groups_by_call_id: RefCell<HashMap<String, ToolActivityGroup>>,
    session_rows: RefCell<Vec<SessionRow>>,
    active_sessions: RefCell<Vec<SessionEntry>>,
    conversation_views: RefCell<HashMap<u64, WarmConversation>>,
    conversation_view_lru: RefCell<VecDeque<u64>>,
    current_session_file: RefCell<Option<PathBuf>>,
    hydrated_agent_session: RefCell<Option<PathBuf>>,
    current_session_title: RefCell<String>,
    pending_session_notice: RefCell<Option<String>>,
    branch_picker: RefCell<Option<session_actions::BranchPickerDialog>>,
    branch_busy: Cell<bool>,
    handoff_busy: Cell<bool>,
    active_subagent: RefCell<Option<String>>,
    session_cost: Cell<f64>,
    extension_widgets: RefCell<HashMap<String, gtk::Label>>,
    extension_statuses: RefCell<HashMap<String, String>>,
    extension_dialogs: RefCell<HashMap<String, adw::AlertDialog>>,
    current_model: RefCell<Option<(String, String)>>,
    ready: Cell<bool>,
    running: Cell<bool>,
    queued_message_count: Cell<usize>,
    goal_completion_calls: RefCell<HashSet<String>>,
    goal_completed_this_run: Cell<bool>,
    window_status: Cell<WindowStatus>,
    session_sound_started: Cell<bool>,
    last_task_progress: Cell<Option<Instant>>,
    recent_prompts: RefCell<VecDeque<Instant>>,
}

impl AppController {
    fn wire_interactions(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ui.composer.connect_changed(move || {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            controller.update_send_state();
            controller.update_completions();
        });

        let key_controller = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(controller) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                && key == gdk::Key::v
                && controller.clipboard_has_supported_image()
            {
                controller.paste_clipboard_image();
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::n {
                controller.start_new_session();
                return glib::Propagation::Stop;
            }
            if let Some(action) = composer_key_action(key, modifiers) {
                let intent = match action {
                    ComposerKeyAction::Submit => SubmissionIntent::Default,
                    ComposerKeyAction::FollowUp => SubmissionIntent::FollowUp,
                    ComposerKeyAction::Newline => return glib::Propagation::Proceed,
                };
                if controller.ui.composer.completions_visible() {
                    controller.accept_completion(Some(intent));
                } else {
                    controller.submit_current_with_intent(intent);
                }
                return glib::Propagation::Stop;
            }
            if !controller.ui.composer.completions_visible() {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Down => {
                    controller.move_completion(1);
                    glib::Propagation::Stop
                }
                gdk::Key::Up => {
                    controller.move_completion(-1);
                    glib::Propagation::Stop
                }
                gdk::Key::Tab => {
                    controller.accept_completion(None);
                    glib::Propagation::Stop
                }
                gdk::Key::ISO_Left_Tab => {
                    controller.move_completion(-1);
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter => glib::Propagation::Proceed,
                gdk::Key::Escape => {
                    controller.hide_completions();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.ui.composer.add_key_controller(key_controller);

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_send_clicked(move || {
            if let Some(controller) = weak.upgrade() {
                controller.submit_current();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_stop_clicked(move || {
            if let Some(controller) = weak.upgrade() {
                controller.stop_current_turn();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_attach_clicked(move || {
            if let Some(controller) = weak.upgrade() {
                controller.present_image_picker();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.new_chat_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.start_new_session();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.history_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.present_history();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.preferences_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.present_settings();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_branch_clicked(move || {
            if let Some(controller) = weak.upgrade() {
                controller.present_branch_picker();
            }
        });
        let weak = Rc::downgrade(self);
        self.ui.composer.connect_handoff_clicked(move || {
            if let Some(controller) = weak.upgrade() {
                controller.present_handoff();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.telemetry.cwd_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.present_workspace_picker();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.window.connect_is_active_notify(move |window| {
            if !window.is_active() {
                return;
            }
            let Some(controller) = weak.upgrade() else {
                return;
            };
            controller.alerts.withdraw();
            if let Some(runtime_id) = controller.active_runtime.get()
                && controller.unread_runtimes.borrow_mut().remove(&runtime_id)
            {
                controller.update_activity_counts();
            }
        });

        let sidebar_root = self.ui.sidebar_root.clone();
        let show_sidebar = self.ui.show_sidebar_button.clone();
        self.ui.hide_sidebar_button.connect_clicked(move |_| {
            sidebar_root.set_visible(false);
            show_sidebar.set_visible(true);
        });

        let sidebar_root = self.ui.sidebar_root.clone();
        let show_sidebar = self.ui.show_sidebar_button.clone();
        self.ui.show_sidebar_button.connect_clicked(move |_| {
            sidebar_root.set_visible(true);
            show_sidebar.set_visible(false);
        });

        let weak = Rc::downgrade(self);
        self.ui.agent_hub_button.connect_toggled(move |button| {
            if button.is_active()
                && let Some(controller) = weak.upgrade()
            {
                controller.refresh_agent_surfaces();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.back_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.close_subagent_view();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.session_list.connect_row_selected(move |_, row| {
            let (Some(controller), Some(row)) = (weak.upgrade(), row) else {
                return;
            };
            let selected = controller
                .session_rows
                .borrow()
                .iter()
                .find(|session| session.row == *row)
                .cloned();
            if let Some(selected) = selected {
                controller.open_session(&selected.entry);
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_model_clicked(move || {
            if let Some(controller) = weak.upgrade() {
                controller.present_model_picker();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_thinking_clicked(move || {
            if let Some(controller) = weak.upgrade() {
                controller.ui.composer.show_thinking_popover();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_completion_activated(move |index| {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            controller.completion_index.set(index as usize);
            controller.accept_completion(None);
        });

        let weak = Rc::downgrade(self);
        self.ui.window.connect_close_request(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.alerts.play(SoundEvent::SessionEnd);
                for runtime in controller.runtimes.borrow().values() {
                    runtime.bridge.shutdown();
                }
            }
            glib::Propagation::Proceed
        });
    }

    fn run_event_loop(this: &Rc<Self>, runtime_id: u64, events: async_channel::Receiver<RpcEvent>) {
        let controller = this.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                controller.handle_runtime_event(runtime_id, event);
            }
        });
    }

    fn active_client(&self) -> Option<BridgeClient> {
        self.active_runtime
            .get()
            .and_then(|runtime_id| self.runtime_client(runtime_id))
    }

    fn runtime_client(&self, runtime_id: u64) -> Option<BridgeClient> {
        self.runtimes
            .borrow()
            .get(&runtime_id)
            .map(|runtime| runtime.bridge.client.clone())
    }
    fn show_runtime_conversation(&self, runtime_id: u64) -> bool {
        let cached = self
            .conversation_views
            .borrow()
            .get(&runtime_id)
            .map(|conversation| conversation.view.clone());
        let was_cached = cached.is_some();
        let conversation = cached.unwrap_or_else(|| {
            let view = ConversationView::main();
            view.show_loading(
                "Opening conversation",
                "Restoring messages, tools, and session state.",
                "Reading conversation history",
            );
            self.conversation_views.borrow_mut().insert(
                runtime_id,
                WarmConversation {
                    view: view.clone(),
                    messages: None,
                    hydrated: false,
                },
            );
            view
        });
        self.ui.show_session_conversation(runtime_id, &conversation);
        let evicted = touch_bounded_lru(
            &mut self.conversation_view_lru.borrow_mut(),
            runtime_id,
            WARM_SESSION_VIEW_LIMIT,
        );
        for evicted_runtime_id in evicted {
            self.conversation_views
                .borrow_mut()
                .remove(&evicted_runtime_id);
            self.ui.remove_session_conversation(evicted_runtime_id);
        }
        was_cached
    }

    fn remove_runtime_conversation(&self, runtime_id: u64) {
        self.conversation_views.borrow_mut().remove(&runtime_id);
        self.conversation_view_lru
            .borrow_mut()
            .retain(|existing| *existing != runtime_id);
        self.ui.remove_session_conversation(runtime_id);
    }

    fn apply_global_delivery_preferences(&self, runtime_id: u64, state: &SessionState) {
        let preferences = self.alerts.preferences();
        let desired = DeliveryModes::from(&preferences);
        let updates = delivery_preference_updates(&preferences, state);
        let client = {
            let mut runtimes = self.runtimes.borrow_mut();
            let Some(runtime) = runtimes.get_mut(&runtime_id) else {
                return;
            };
            if updates.is_empty() {
                runtime.delivery_target = None;
                return;
            }
            if !should_send_delivery_preference_updates(runtime.delivery_target, desired, updates) {
                return;
            }
            runtime.delivery_target = Some(desired);
            runtime.bridge.client.clone()
        };
        if let Err(error) = send_delivery_preference_updates(&client, updates) {
            if let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id)
                && runtime.delivery_target == Some(desired)
            {
                runtime.delivery_target = None;
            }
            if self.active_runtime.get() == Some(runtime_id) {
                self.show_error(&format!(
                    "Could not apply global message delivery settings: {error}"
                ));
            } else {
                eprintln!("Could not apply global message delivery settings: {error}");
            }
        }
    }

    fn send_to_ready_runtimes(
        &self,
        desired: DeliveryModes,
        send: impl Fn(&BridgeClient) -> Result<(), crate::bridge::BridgeError>,
    ) {
        let mut first_error = None;
        {
            let mut runtimes = self.runtimes.borrow_mut();
            for runtime in runtimes.values_mut().filter(|runtime| runtime.ready) {
                runtime.delivery_target = Some(desired);
                if let Err(error) = send(&runtime.bridge.client) {
                    runtime.delivery_target = None;
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        if let Some(error) = first_error {
            self.show_error(&format!(
                "Could not apply global message delivery setting: {error}"
            ));
        }
    }

    fn set_global_steering_mode(&self, mode: QueueMode) {
        if mode == self.alerts.preferences().steering_mode {
            return;
        }
        if let Err(error) = self.alerts.set_steering_mode(mode) {
            self.show_error(&error);
            return;
        }
        let desired = DeliveryModes::from(&self.alerts.preferences());
        self.send_to_ready_runtimes(desired, |client| client.set_steering_mode(mode));
    }

    fn set_global_follow_up_mode(&self, mode: QueueMode) {
        if mode == self.alerts.preferences().follow_up_mode {
            return;
        }
        if let Err(error) = self.alerts.set_follow_up_mode(mode) {
            self.show_error(&error);
            return;
        }
        let desired = DeliveryModes::from(&self.alerts.preferences());
        self.send_to_ready_runtimes(desired, |client| client.set_follow_up_mode(mode));
    }

    fn set_global_interrupt_mode(&self, mode: InterruptMode) {
        if mode == self.alerts.preferences().interrupt_mode {
            return;
        }
        if let Err(error) = self.alerts.set_interrupt_mode(mode) {
            self.show_error(&error);
            return;
        }
        let desired = DeliveryModes::from(&self.alerts.preferences());
        self.send_to_ready_runtimes(desired, |client| client.set_interrupt_mode(mode));
    }

    fn spawn_runtime(
        self: &Rc<Self>,
        session_path: Option<&Path>,
        workspace: Option<&Path>,
    ) -> std::io::Result<u64> {
        let bridge = OmpBridge::spawn_for_session(session_path, workspace)?;
        let events = bridge.events.clone();
        let runtime_id = self.next_runtime_id.get();
        self.next_runtime_id.set(runtime_id + 1);
        let workspace = session_path
            .and_then(|path| session_catalog::session_entry(Some(path), "", false, None).cwd)
            .or_else(|| workspace.map(Path::to_owned))
            .or_else(|| std::env::current_dir().ok());
        self.runtimes.borrow_mut().insert(
            runtime_id,
            SessionRuntime {
                bridge,
                running: false,
                ready: false,
                disconnected: None,
                delivery_target: None,
                workspace,
                state: None,
            },
        );
        Self::run_event_loop(self, runtime_id, events);
        Ok(runtime_id)
    }

    fn apply_inactive_state(&self, runtime_id: u64, state: SessionState) {
        if let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id) {
            runtime.ready = true;
            runtime.running = state.is_streaming;
            runtime.state = Some(state.clone());
        }
        let fallback_workspace = self
            .runtimes
            .borrow()
            .get(&runtime_id)
            .and_then(|runtime| runtime.workspace.clone());
        self.apply_global_delivery_preferences(runtime_id, &state);
        let path = state.session_file.as_deref().map(PathBuf::from);
        let disk_title = path
            .as_deref()
            .and_then(session_catalog::read_session_title);
        let mut entries = self.active_sessions.borrow_mut();
        let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.runtime_id == Some(runtime_id))
        else {
            return;
        };
        let resolved = session_catalog::authoritative_title(
            state.session_name.as_deref(),
            disk_title.as_deref(),
        );
        let title =
            session_catalog::authoritative_title(Some(&resolved), Some(entry.title.as_str()));
        let mut refreshed = session_catalog::session_entry(
            path.as_deref(),
            &title,
            false,
            fallback_workspace.as_deref(),
        );
        refreshed.runtime_id = Some(runtime_id);
        refreshed.created_at = entry.created_at;
        refreshed.running = state.is_streaming;
        if let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id) {
            runtime.workspace = refreshed.cwd.clone();
        }
        *entry = refreshed;
        drop(entries);
        self.persist_recent_sessions();
    }

    fn handle_runtime_event(self: &Rc<Self>, runtime_id: u64, event: RpcEvent) {
        match &event {
            RpcEvent::Ready => {
                if let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id) {
                    runtime.ready = true;
                    runtime.delivery_target = None;
                    runtime.disconnected = None;
                }
            }
            RpcEvent::Disconnected(message) => {
                if let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id) {
                    runtime.ready = false;
                    runtime.delivery_target = None;
                    runtime.disconnected = Some(message.clone());
                }
            }
            RpcEvent::Response(response)
                if !response.success
                    && matches!(
                        response.command.as_str(),
                        "set_steering_mode" | "set_follow_up_mode" | "set_interrupt_mode"
                    ) =>
            {
                if let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id) {
                    runtime.delivery_target = None;
                }
            }
            _ => {}
        }
        if matches!(event, RpcEvent::Ready)
            && let Some(client) = self.runtime_client(runtime_id)
            && let Err(error) = client.initialize()
        {
            if self.active_runtime.get() == Some(runtime_id) {
                self.show_error(&error.to_string());
            }
            return;
        }

        let was_running = self
            .runtimes
            .borrow()
            .get(&runtime_id)
            .is_some_and(|runtime| runtime.running);
        if matches!(event, RpcEvent::AgentEnd)
            && completion_is_unread(
                was_running,
                self.active_runtime.get(),
                runtime_id,
                self.ui.window.is_active(),
            )
        {
            self.unread_runtimes.borrow_mut().insert(runtime_id);
        }

        let running = match &event {
            RpcEvent::AgentStart => Some(true),
            RpcEvent::AgentEnd | RpcEvent::Disconnected(_) => Some(false),
            _ => None,
        };
        if let Some(running) = running {
            if let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id) {
                runtime.running = running;
            }
            for entry in self.active_sessions.borrow_mut().iter_mut() {
                if entry.runtime_id == Some(runtime_id) {
                    entry.running = running;
                }
            }
        }

        if self.active_runtime.get() != Some(runtime_id)
            && matches!(
                event,
                RpcEvent::MessageStart(_)
                    | RpcEvent::TextDelta(_)
                    | RpcEvent::ThinkingDelta(_)
                    | RpcEvent::MessageEnd(_)
                    | RpcEvent::ToolStart(_)
                    | RpcEvent::ToolUpdate(_)
                    | RpcEvent::ToolEnd(_)
            )
            && let Some(conversation) = self.conversation_views.borrow_mut().get_mut(&runtime_id)
        {
            conversation.hydrated = false;
        }

        if self.active_runtime.get() != Some(runtime_id) {
            match &event {
                RpcEvent::Response(response)
                    if response.success && response.command == "get_state" =>
                {
                    if let Some(data) = response.data.clone()
                        && let Ok(state) = serde_json::from_value::<SessionState>(data)
                    {
                        self.apply_inactive_state(runtime_id, state);
                    }
                }
                RpcEvent::SessionInfo { title: Some(title) } if !title.trim().is_empty() => {
                    if let Some(entry) = self
                        .active_sessions
                        .borrow_mut()
                        .iter_mut()
                        .find(|entry| entry.runtime_id == Some(runtime_id))
                    {
                        entry.title = title.trim().to_owned();
                    }
                }
                _ => {}
            }
            if running.is_some()
                || matches!(event, RpcEvent::Response(_) | RpcEvent::SessionInfo { .. })
            {
                self.render_session_sidebar(self.active_sessions.borrow().clone());
                self.update_activity_counts();
            }
            return;
        }

        self.handle_event(event);
        if running.is_some() {
            self.refresh_session_sidebar();
        }
    }

    fn handle_event(self: &Rc<Self>, event: RpcEvent) {
        match event {
            RpcEvent::Ready => {
                if !self.session_sound_started.replace(true) {
                    self.alerts.play(SoundEvent::SessionStart);
                }
            }
            RpcEvent::Response(response) => self.handle_response(response),
            RpcEvent::Commands(commands) => {
                self.commands.replace(commands);
                self.update_completions();
            }
            RpcEvent::MessageStart(message) => self.message_started(&message),
            RpcEvent::TextDelta(delta) => self.append_stream_delta(&delta),
            RpcEvent::ThinkingDelta(delta) => self.append_thinking_delta(&delta),
            RpcEvent::MessageEnd(message) => self.message_ended(&message),
            RpcEvent::AgentStart => {
                self.finish_active_tool_group();
                self.running.set(true);
                self.goal_completed_this_run.set(false);
                self.goal_completion_calls.borrow_mut().clear();
                self.last_task_progress.set(Some(Instant::now()));
                self.alerts.play(SoundEvent::TaskAcknowledge);
                self.set_window_status(WindowStatus::Working);
                self.ui.chat_status.activity("Thinking");
                self.update_activity_counts();
                self.update_send_state();
            }
            RpcEvent::AgentEnd => {
                let was_running = self.running.replace(false);
                self.last_task_progress.set(None);
                if let Some(thinking) = self.streaming_thinking.borrow_mut().take() {
                    thinking.finish(None);
                    self.refresh_active_activity_summary();
                }
                self.finish_active_tool_group();
                self.streaming_message.borrow_mut().take();
                self.ui.chat_status.idle();
                let goal_completed = self.goal_completed_this_run.get();
                self.set_window_status(if goal_completed {
                    WindowStatus::GoalComplete
                } else {
                    WindowStatus::Ready
                });
                if let Some(alert) = alerts::alert_for_agent_end(was_running, goal_completed) {
                    self.alerts.play(SoundEvent::TaskComplete);
                    self.send_alert(alert);
                }
                self.update_activity_counts();
                self.update_send_state();
                if let Some(client) = self.active_client() {
                    let _ = client.refresh_state();
                }
            }
            RpcEvent::ToolStart(tool) => self.tool_started(tool),
            RpcEvent::ToolUpdate(tool) => self.tool_updated(tool),
            RpcEvent::ToolEnd(tool) => self.tool_ended(tool, ToolResultSource::Live),
            RpcEvent::Subagent(update) => self.subagent_updated(update),
            RpcEvent::CommandOutput(text) => {
                if !text.is_empty() {
                    self.remove_empty_state();
                    self.append_grouped_notice(&text, false);
                    self.scroll_to_bottom();
                }
            }
            RpcEvent::PromptResult(agent_invoked) => {
                if !agent_invoked {
                    self.pending_user_messages.borrow_mut().pop_front();
                }
            }
            RpcEvent::SessionInfo { title } => {
                if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                    self.set_session_title(&title);
                    self.refresh_session_sidebar();
                }
            }
            RpcEvent::ConfigChanged => {
                if let Some(client) = self.active_client() {
                    let _ = client.refresh_state();
                }
            }
            RpcEvent::Notice { level, message } => {
                self.ui
                    .conversation()
                    .append_notice(&message, level == "error");
                self.scroll_to_bottom();
            }
            RpcEvent::ModelChanged => {
                if let Some(client) = self.active_client() {
                    let _ = client.refresh_state();
                }
            }
            RpcEvent::ThinkingChanged(level) => {
                if let Some(level) = level {
                    self.select_thinking(&level);
                } else if let Some(client) = self.active_client() {
                    let _ = client.refresh_state();
                }
            }
            RpcEvent::ExtensionUi(request) => self.handle_extension_ui(request),
            RpcEvent::Stderr(line) => eprintln!("omp: {line}"),
            RpcEvent::Disconnected(message) => {
                self.ready.set(false);
                self.running.set(false);
                self.set_window_status(WindowStatus::Disconnected);
                self.ui.chat_status.disconnected();
                self.ui.composer.set_input_sensitive(false);
                self.update_send_state();
                while !self.pending_submissions.borrow().is_empty() {
                    self.reject_submission_response(None);
                }
                self.show_error(&message);
            }
            RpcEvent::Other => {}
        }
    }

    fn handle_response(self: &Rc<Self>, response: RpcResponse) {
        if response.command == "get_subagent_messages" && !response.success {
            self.subagent_transcript_failed(
                response.id.as_deref(),
                response
                    .error
                    .as_deref()
                    .unwrap_or("omp could not read the agent transcript"),
            );
            return;
        }
        if !response.success {
            if matches!(response.command.as_str(), "prompt" | "steer" | "follow_up") {
                self.reject_submission_response(response.id.as_deref());
            }
            if matches!(
                response.command.as_str(),
                "set_steering_mode" | "set_follow_up_mode" | "set_interrupt_mode"
            ) {
                self.render_authoritative_queue_state();
            }
            let error = response
                .error
                .as_deref()
                .unwrap_or("omp rejected the request");
            match response.command.as_str() {
                "get_branch_messages" | "branch" => {
                    self.branch_busy.set(false);
                    if let Some(picker) = self.branch_picker.borrow().as_ref() {
                        picker.show_error(error);
                    }
                    self.finish_session_action();
                }
                "handoff" => {
                    self.handoff_busy.set(false);
                    self.finish_session_action();
                    self.show_error(error);
                }
                _ => self.show_error(error),
            }
            return;
        }
        if matches!(response.command.as_str(), "prompt" | "steer" | "follow_up") {
            self.accept_submission_response(response.id.as_deref());
        }
        if matches!(
            response.command.as_str(),
            "set_steering_mode" | "set_follow_up_mode" | "set_interrupt_mode"
        ) && let Some(client) = self.active_client()
        {
            let _ = client.refresh_state();
        }
        let response_id = response.id.clone();
        if response.command == "get_subagent_messages" && response.data.is_none() {
            self.subagent_transcript_failed(
                response_id.as_deref(),
                "omp returned no agent transcript data",
            );
            return;
        }
        let Some(data) = response.data else {
            match response.command.as_str() {
                "get_branch_messages" | "branch" => {
                    self.branch_busy.set(false);
                    if let Some(picker) = self.branch_picker.borrow().as_ref() {
                        picker.show_error("omp returned an incomplete branch response.");
                    }
                    self.finish_session_action();
                }
                "handoff" => {
                    self.handoff_busy.set(false);
                    self.finish_session_action();
                    self.show_error("omp returned an incomplete handoff response.");
                }
                _ => {}
            }
            return;
        };

        match response.command.as_str() {
            "get_branch_messages" => {
                self.branch_busy.set(false);
                match serde_json::from_value::<BranchMessagesResponse>(data) {
                    Ok(response) => {
                        if let Some(picker) = self.branch_picker.borrow().as_ref() {
                            picker.set_candidates(response.messages);
                        }
                    }
                    Err(_) => {
                        if let Some(picker) = self.branch_picker.borrow().as_ref() {
                            picker.show_error("omp returned invalid branch message data.");
                        }
                    }
                }
                self.finish_session_action();
            }
            "branch" => {
                self.branch_busy.set(false);
                match serde_json::from_value::<BranchResponse>(data) {
                    Ok(result) if !result.cancelled => {
                        if let Some(picker) = self.branch_picker.borrow().as_ref() {
                            picker.close();
                        }
                        self.refresh_after_confirmed_session_change(
                            "Opening branched conversation",
                            "Loading branched conversation…",
                        );
                    }
                    Ok(_) => {
                        if let Some(picker) = self.branch_picker.borrow().as_ref() {
                            picker.show_error(
                                "omp cancelled the branch. The current conversation is unchanged.",
                            );
                        }
                        self.finish_session_action();
                    }
                    Err(_) => {
                        if let Some(picker) = self.branch_picker.borrow().as_ref() {
                            picker.show_error("omp returned invalid branch result data.");
                        }
                        self.finish_session_action();
                    }
                }
            }
            "handoff" => {
                self.handoff_busy.set(false);
                match serde_json::from_value::<Option<HandoffResponse>>(data) {
                    Ok(Some(result)) => {
                        self.pending_session_notice.replace(
                            result
                                .saved_path
                                .map(|path| format!("Handoff document saved to: {path}")),
                        );
                        self.refresh_after_confirmed_session_change(
                            "Opening handoff conversation",
                            "Loading handoff conversation…",
                        );
                    }
                    Ok(None) => {
                        self.finish_session_action();
                        self.show_error(
                            "omp cancelled the handoff. The current conversation is unchanged.",
                        );
                    }
                    Err(_) => {
                        self.finish_session_action();
                        self.show_error("omp returned invalid handoff result data.");
                    }
                }
            }
            "get_state" => match serde_json::from_value::<SessionState>(data) {
                Ok(state) => self.apply_state(state),
                Err(error) => {
                    self.show_error(&format!("omp returned invalid session state: {error}"))
                }
            },
            "get_available_models" => {
                if let Some(models) = data.get("models").cloned()
                    && let Ok(models) = serde_json::from_value::<Vec<ModelSummary>>(models)
                {
                    self.apply_models(models);
                }
            }
            "get_available_commands" => {
                if let Some(commands) = data.get("commands").cloned()
                    && let Ok(commands) = serde_json::from_value::<Vec<SlashCommand>>(commands)
                {
                    self.commands.replace(commands);
                }
            }
            "get_messages" => {
                let mut messages = data
                    .get("messages")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if let Some(persisted) = self
                    .current_session_file
                    .borrow()
                    .as_deref()
                    .and_then(|path| session_catalog::read_session_messages(path).ok())
                    .filter(|persisted| persisted.len() > messages.len())
                {
                    messages = persisted;
                }
                let runtime_id = self.active_runtime.get();
                let should_hydrate = runtime_id.is_none_or(|runtime_id| {
                    let conversations = self.conversation_views.borrow();
                    let Some(conversation) = conversations.get(&runtime_id) else {
                        return true;
                    };
                    messages_need_hydration(
                        conversation.messages.as_deref(),
                        conversation.hydrated,
                        &messages,
                    )
                });
                if should_hydrate {
                    self.hydrate_messages(&messages);
                }
                if let Some(runtime_id) = runtime_id
                    && let Some(conversation) =
                        self.conversation_views.borrow_mut().get_mut(&runtime_id)
                {
                    conversation.messages = Some(messages);
                }
                if let Some(notice) = self.pending_session_notice.borrow_mut().take() {
                    self.ui.conversation().append_notice(&notice, false);
                    self.scroll_to_bottom();
                }
            }
            "get_subagents" => {
                if let Some(agents) = data.get("subagents").cloned()
                    && let Ok(agents) = serde_json::from_value::<Vec<SubagentSnapshot>>(agents)
                {
                    self.agent_hub.borrow_mut().apply_snapshot(agents);
                    self.refresh_agent_surfaces();
                }
            }
            "get_subagent_messages" => {
                if let Ok(transcript) = serde_json::from_value::<SubagentMessages>(data) {
                    self.apply_subagent_transcript(response_id.as_deref(), transcript);
                } else {
                    self.subagent_transcript_failed(
                        response_id.as_deref(),
                        "omp returned invalid agent transcript metadata",
                    );
                }
            }
            "prompt" => {
                if data.get("agentInvoked").and_then(Value::as_bool) == Some(false) {
                    self.pending_user_messages.borrow_mut().pop_front();
                }
            }
            "set_session_name" => self.refresh_after_session_change(),
            _ => {}
        }
    }

    fn take_pending_submission(&self, request_id: Option<&str>) -> Option<PendingSubmission> {
        let mut pending = self.pending_submissions.borrow_mut();
        let index = match request_id {
            Some(request_id) => pending
                .iter()
                .position(|submission| submission.request_id == request_id)?,
            None => (!pending.is_empty()).then_some(0)?,
        };
        pending.remove(index)
    }

    fn remove_pending_user_message(&self, message: &str) {
        let mut pending = self.pending_user_messages.borrow_mut();
        if let Some(index) = pending.iter().position(|pending| pending == message) {
            pending.remove(index);
        }
    }

    fn reject_submission_response(self: &Rc<Self>, request_id: Option<&str>) {
        let Some(submission) = self.take_pending_submission(request_id) else {
            return;
        };
        self.remove_pending_user_message(&submission.message);
        if let Some(message) = submission.displayed_message {
            self.ui.conversation().remove_message(&message);
            if self.ui.conversation().is_empty() {
                self.show_empty_state();
            }
        }
        self.attachments
            .borrow_mut()
            .resolve_submission(&submission.attachment_ids, false);
        if !self.running.get() {
            self.ui.chat_status.idle();
        }
        self.update_send_state();
    }

    fn refresh_after_confirmed_session_change(&self, activity: &str, notice: &str) {
        self.branch_busy.set(false);
        self.handoff_busy.set(false);
        self.close_subagent_view();
        self.clear_messages();
        self.clear_subagents();
        self.ui.chat_status.activity(activity);
        self.ui
            .conversation()
            .show_loading(activity, notice, "Syncing conversation state");
        self.update_send_state();
        self.refresh_after_session_change();
    }

    fn finish_session_action(&self) {
        if self.running.get() {
            self.ui.chat_status.activity("Working");
        } else {
            self.ui.chat_status.idle();
        }
        self.update_send_state();
    }

    fn accept_submission_response(&self, request_id: Option<&str>) {
        let Some(submission) = self.take_pending_submission(request_id) else {
            return;
        };
        self.goal_completed_this_run.set(false);
        self.goal_completion_calls.borrow_mut().clear();
        self.record_prompt_sound();
        self.set_window_status(WindowStatus::Working);
        self.remove_empty_state();
        if self.ui.composer.text() == submission.draft_text {
            self.ui.composer.set_text("");
        }
        self.attachments
            .borrow_mut()
            .resolve_submission(&submission.attachment_ids, true);
        if self.attachments.borrow().is_empty() {
            self.ui.composer.clear_attachment_previews();
        } else {
            for id in submission.attachment_ids {
                self.ui.composer.remove_attachment_preview(id);
            }
        }
        self.hide_completions();
        self.scroll_to_bottom();
        if let Some(client) = self.active_client() {
            let _ = client.refresh_state();
        }
        self.update_send_state();
    }

    fn refresh_after_session_change(&self) {
        if let Some(client) = self.active_client() {
            let _ = client.refresh_state();
            let _ = client.refresh_messages();
            let _ = client.refresh_subagents();
        }
    }

    fn render_workspace(&self, cwd: Option<&Path>) {
        if let Some(cwd) = cwd {
            self.ui.telemetry.cwd.set_text(&compact_path(cwd));
            self.ui
                .telemetry
                .cwd
                .set_tooltip_text(Some(&cwd.to_string_lossy()));
        } else {
            self.ui.telemetry.cwd.set_text("No workspace");
            self.ui.telemetry.cwd.set_tooltip_text(None);
        }
    }

    fn current_workspace(&self) -> Option<PathBuf> {
        self.active_runtime
            .get()
            .and_then(|runtime_id| {
                self.runtimes
                    .borrow()
                    .get(&runtime_id)
                    .and_then(|runtime| runtime.workspace.clone())
            })
            .or_else(|| std::env::current_dir().ok())
    }

    fn apply_state(self: &Rc<Self>, state: SessionState) {
        self.ready.set(true);
        self.running.set(state.is_streaming);
        if let Some(group) = self.active_tool_group.borrow().as_ref() {
            group.set_working(state.is_streaming);
        }
        let runtime_id = self.active_runtime.get();
        if let Some(runtime_id) = runtime_id
            && let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id)
        {
            runtime.ready = true;
            runtime.running = state.is_streaming;
            runtime.state = Some(state.clone());
        }
        self.ui.composer.set_input_sensitive(true);
        self.queued_message_count.set(state.queued_message_count);
        self.render_authoritative_queue_state();
        if let Some(runtime_id) = runtime_id {
            self.apply_global_delivery_preferences(runtime_id, &state);
        }
        self.reconcile_todos(state.todo_phases.clone());

        let session_file = state.session_file.as_deref().map(PathBuf::from);
        let disk_title = session_file
            .as_deref()
            .and_then(session_catalog::read_session_title);
        self.current_session_file.replace(session_file.clone());
        self.restore_persisted_subagents();
        let resolved = session_catalog::authoritative_title(
            state.session_name.as_deref(),
            disk_title.as_deref(),
        );
        let title = session_catalog::authoritative_title(
            Some(&resolved),
            Some(&self.current_session_title.borrow()),
        );
        self.set_session_title(&title);
        let fallback_workspace = runtime_id
            .and_then(|runtime_id| self.runtimes.borrow().get(&runtime_id)?.workspace.clone());
        let current_entry = session_catalog::session_entry(
            session_file.as_deref(),
            &title,
            true,
            fallback_workspace.as_deref(),
        );
        if let Some(runtime_id) = runtime_id
            && let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id)
        {
            runtime.workspace = current_entry.cwd.clone();
        }
        self.render_workspace(current_entry.cwd.as_deref());

        let mut model_window = 0;
        if let Some(model) = state.model {
            model_window = model.context_window.unwrap_or(0);
            self.current_model
                .replace(Some((model.provider.clone(), model.id.clone())));
            self.ui.composer.set_model_provider(&model.provider);
            let efforts = model
                .thinking
                .as_ref()
                .map(|thinking| thinking.efforts.clone())
                .unwrap_or_default();
            self.apply_thinking_levels(efforts, state.thinking_level.as_deref());
            self.select_model(&model.provider, &model.id);
        } else {
            self.apply_thinking_levels(Vec::new(), state.thinking_level.as_deref());
        }
        if let Some(context) = state.context_usage {
            let window = if context.context_window > 0 {
                context.context_window
            } else {
                model_window
            };
            self.ui
                .telemetry
                .set_context(context.tokens, window, context.percent);
        }
        self.ui.telemetry.set_throughput(state.tokens_per_second);
        if state.is_streaming {
            self.ui.chat_status.activity(if state.is_compacting {
                "Compacting"
            } else {
                "Working"
            });
            self.set_window_status(WindowStatus::Working);
        } else {
            self.ui.chat_status.idle();
            if self.window_status.get() != WindowStatus::GoalComplete {
                self.set_window_status(WindowStatus::Ready);
            }
        }
        self.refresh_session_sidebar();
        self.update_activity_counts();
        self.update_send_state();
    }

    fn reconcile_todos(&self, phases: Vec<TodoPhase>) {
        if let Err(error) = validate_phases(&phases) {
            self.reject_todo_reconciliation(&format!("omp returned invalid todo state: {error}"));
            return;
        }
        self.ui.todos.set_phases(&phases);
        self.ui.todos.set_error(None);
    }

    fn reject_todo_reconciliation(&self, message: &str) {
        self.ui.todos.set_error(Some(message));
        self.show_error(message);
    }

    fn set_session_title(&self, title: &str) {
        let title = if title.trim().is_empty() {
            "New conversation"
        } else {
            title.trim()
        };
        self.current_session_title.replace(title.to_owned());
        if let Some(runtime_id) = self.active_runtime.get()
            && let Some(entry) = self
                .active_sessions
                .borrow_mut()
                .iter_mut()
                .find(|entry| entry.runtime_id == Some(runtime_id))
        {
            entry.title = title.to_owned();
        }
        if self.active_subagent.borrow().is_none() {
            self.ui.title.set_text(title);
        }
        self.refresh_window_title();
    }

    fn refresh_titles_from_disk(self: &Rc<Self>) {
        let active_runtime = self.active_runtime.get();
        let current_title = self.current_session_title.borrow().clone();
        let entries = self
            .active_sessions
            .borrow()
            .iter()
            .map(|entry| {
                let current = entry.runtime_id == active_runtime;
                let mut refreshed = session_catalog::session_entry(
                    entry.path.as_deref(),
                    if current {
                        &current_title
                    } else {
                        &entry.title
                    },
                    current,
                    entry.cwd.as_deref(),
                );
                refreshed.title = session_catalog::authoritative_title(
                    Some(&refreshed.title),
                    Some(if current {
                        &current_title
                    } else {
                        &entry.title
                    }),
                );
                refreshed.runtime_id = entry.runtime_id;
                refreshed.running = entry.running;
                refreshed.created_at = entry.created_at;
                refreshed
            })
            .collect::<Vec<_>>();
        if entries == *self.active_sessions.borrow() {
            return;
        }
        if let Some(current) = entries.iter().find(|entry| entry.current)
            && current.title != current_title
        {
            self.set_session_title(&current.title);
        }
        self.active_sessions.replace(entries.clone());
        self.render_session_sidebar(entries);
    }

    fn persist_recent_sessions(&self) {
        if let Err(error) = session_catalog::save_recent_sessions(&self.active_sessions.borrow()) {
            eprintln!("Could not save recent sessions: {error}");
        }
    }

    fn refresh_session_sidebar(self: &Rc<Self>) {
        let runtime_id = self.active_runtime.get();
        let running = runtime_id
            .and_then(|runtime_id| {
                self.runtimes
                    .borrow()
                    .get(&runtime_id)
                    .map(|runtime| runtime.running)
            })
            .unwrap_or(false);
        let fallback_workspace = runtime_id.and_then(|runtime_id| {
            self.runtimes
                .borrow()
                .get(&runtime_id)
                .and_then(|runtime| runtime.workspace.clone())
        });
        let mut current = session_catalog::session_entry(
            self.current_session_file.borrow().as_deref(),
            &self.current_session_title.borrow(),
            true,
            fallback_workspace.as_deref(),
        );
        current.runtime_id = runtime_id;
        current.running = running;
        let mut entries = self.active_sessions.borrow_mut();
        update_current_session(&mut entries, current);
        let rendered = entries.clone();
        drop(entries);
        self.persist_recent_sessions();
        self.render_session_sidebar(rendered);
    }

    fn render_session_sidebar(self: &Rc<Self>, entries: Vec<SessionEntry>) {
        while let Some(child) = self.ui.session_list.first_child() {
            self.ui.session_list.remove(&child);
        }
        self.session_rows.borrow_mut().clear();
        for entry in entries {
            let session = sidebar::session_row(entry);
            session.open_action.set_sensitive(!session.entry.current);
            if let Some(target_runtime_id) = session.entry.runtime_id {
                let weak = Rc::downgrade(self);
                sidebar::make_session_draggable(
                    &session.row,
                    target_runtime_id,
                    move |source_runtime_id, insert_after| {
                        if let Some(controller) = weak.upgrade() {
                            controller.reorder_session(
                                source_runtime_id,
                                target_runtime_id,
                                insert_after,
                            );
                        }
                    },
                );
            }
            let open_entry = session.entry.clone();
            let weak = Rc::downgrade(self);
            session.open_action.connect_clicked(move |_| {
                if let Some(controller) = weak.upgrade() {
                    controller.open_session(&open_entry);
                }
            });
            let rename_entry = session.entry.clone();
            let weak = Rc::downgrade(self);
            session.rename_action.connect_clicked(move |_| {
                if let Some(controller) = weak.upgrade() {
                    controller.present_rename_dialog(&rename_entry);
                }
            });
            let close_entry = session.entry.clone();
            let weak = Rc::downgrade(self);
            session.close_action.connect_clicked(move |_| {
                if let Some(controller) = weak.upgrade() {
                    controller.close_session(&close_entry);
                }
            });
            let delete_entry = session.entry.clone();
            let weak = Rc::downgrade(self);
            session.delete_action.connect_clicked(move |_| {
                if let Some(controller) = weak.upgrade() {
                    controller.present_delete_dialog(&delete_entry);
                }
            });
            self.ui.session_list.append(&session.row);
            if session.entry.current {
                self.ui.session_list.select_row(Some(&session.row));
            }
            self.session_rows.borrow_mut().push(session);
        }
        self.update_activity_counts();
    }

    fn reorder_session(
        self: &Rc<Self>,
        source_runtime_id: u64,
        target_runtime_id: u64,
        insert_after: bool,
    ) {
        let mut entries = self.active_sessions.borrow_mut();
        if !reorder_session_entries(
            &mut entries,
            source_runtime_id,
            target_runtime_id,
            insert_after,
        ) {
            return;
        }
        let rendered = entries.clone();
        drop(entries);
        self.persist_recent_sessions();
        self.render_session_sidebar(rendered);
    }

    fn present_image_picker(self: &Rc<Self>) {
        if !self.ready.get() {
            return;
        }
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PNG and JPEG images"));
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Attach images")
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let weak = Rc::downgrade(self);
        dialog.open_multiple(
            Some(&self.ui.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(files) = result else {
                    return;
                };
                let files = (0..files.n_items())
                    .filter_map(|index| files.item(index))
                    .filter_map(|item| item.downcast::<gio::File>().ok())
                    .collect::<Vec<_>>();
                if let Some(controller) = weak.upgrade() {
                    controller.load_image_files(files);
                }
            },
        );
    }

    fn load_image_files(self: &Rc<Self>, files: Vec<gio::File>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            for file in files {
                let name = file
                    .basename()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Image".to_owned());
                let bytes = match file.load_bytes_future().await {
                    Ok((bytes, _)) => bytes.as_ref().to_vec(),
                    Err(error) => {
                        if let Some(controller) = weak.upgrade() {
                            controller.show_error(&format!("Could not attach {name}: {error}"));
                        }
                        continue;
                    }
                };
                let (image, bytes) = match encode_image_in_background(bytes).await {
                    Ok(encoded) => encoded,
                    Err(error) => {
                        if let Some(controller) = weak.upgrade() {
                            controller.show_error(&format!("Could not attach {name}: {error}"));
                        }
                        continue;
                    }
                };
                let Some(controller) = weak.upgrade() else {
                    break;
                };
                controller.append_loaded_attachment(name, image, bytes);
            }
        });
    }

    fn clipboard_has_supported_image(&self) -> bool {
        let formats = gtk::prelude::WidgetExt::display(&self.ui.window)
            .clipboard()
            .formats();
        formats.contain_mime_type("image/png") || formats.contain_mime_type("image/jpeg")
    }

    fn paste_clipboard_image(self: &Rc<Self>) {
        if !self.ready.get() {
            return;
        }
        let clipboard = gtk::prelude::WidgetExt::display(&self.ui.window).clipboard();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let bytes = match clipboard
                .read_future(&["image/png", "image/jpeg"], glib::Priority::DEFAULT)
                .await
            {
                Ok((stream, _)) => match read_stream_bytes(&stream).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        if let Some(controller) = weak.upgrade() {
                            controller.show_error(&format!(
                                "Could not read the clipboard image: {error}"
                            ));
                        }
                        return;
                    }
                },
                Err(error) => {
                    if let Some(controller) = weak.upgrade() {
                        controller
                            .show_error(&format!("Could not read the clipboard image: {error}"));
                    }
                    return;
                }
            };
            let (image, bytes) = match encode_image_in_background(bytes).await {
                Ok(encoded) => encoded,
                Err(error) => {
                    if let Some(controller) = weak.upgrade() {
                        controller
                            .show_error(&format!("Could not attach the clipboard image: {error}"));
                    }
                    return;
                }
            };
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let number = controller.pasted_image_count.get() + 1;
            controller.pasted_image_count.set(number);
            controller.append_loaded_attachment(format!("Pasted image {number}.png"), image, bytes);
        });
    }

    fn append_loaded_attachment(
        self: &Rc<Self>,
        name: String,
        image: crate::bridge::protocol::ImageContent,
        bytes: Vec<u8>,
    ) {
        let bytes = glib::Bytes::from_owned(bytes);
        let stream = gio::MemoryInputStream::from_bytes(&bytes);
        let preview = match gdk_pixbuf::Pixbuf::from_stream_at_scale(
            &stream,
            76,
            64,
            true,
            None::<&gio::Cancellable>,
        ) {
            Ok(preview) => preview,
            Err(error) => {
                self.show_error(&format!("Could not preview {name}: {error}"));
                return;
            }
        };
        let texture = composer::texture_from_pixbuf(&preview);
        let id = self.attachments.borrow_mut().add(&name, image);
        let weak = Rc::downgrade(self);
        self.ui
            .composer
            .append_attachment_preview(id, &name, &texture, move |id| {
                if let Some(controller) = weak.upgrade() {
                    controller.remove_attachment(id);
                }
            });
        self.update_send_state();
    }

    fn remove_attachment(&self, id: AttachmentId) {
        if self.attachments.borrow_mut().remove(id) {
            self.ui.composer.remove_attachment_preview(id);
            self.update_send_state();
        }
    }

    fn present_workspace_picker(self: &Rc<Self>) {
        if !self.ready.get() || self.running.get() {
            return;
        }

        let current_workspace = self.current_workspace();
        let dialog = gtk::FileDialog::builder()
            .title("Select workspace")
            .modal(true)
            .build();
        if let Some(path) = current_workspace.as_deref() {
            dialog.set_initial_folder(Some(&gio::File::for_path(path)));
        }

        let weak = Rc::downgrade(self);
        dialog.select_folder(
            Some(&self.ui.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(folder) = result else {
                    return;
                };
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                let Some(path) = folder.path() else {
                    controller.show_error("Only local workspace directories are supported");
                    return;
                };
                controller.move_current_session_to(&path);
            },
        );
    }

    fn move_current_session_to(self: &Rc<Self>, path: &Path) {
        if !self.ready.get()
            || self.running.get()
            || self.current_workspace().as_deref() == Some(path)
        {
            return;
        }
        let Some(client) = self.active_client() else {
            return;
        };
        match client.move_session(path) {
            Ok(()) => {
                if let Some(runtime_id) = self.active_runtime.get()
                    && let Some(runtime) = self.runtimes.borrow_mut().get_mut(&runtime_id)
                {
                    runtime.workspace = Some(path.to_owned());
                }
                self.render_workspace(Some(path));
                self.refresh_session_sidebar();
                if self.ui.conversation().is_empty() {
                    self.show_empty_state();
                }
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn present_branch_picker(self: &Rc<Self>) {
        if !self.session_actions_available() {
            return;
        }
        if self.branch_picker.borrow().is_none() {
            let weak = Rc::downgrade(self);
            let picker = session_actions::BranchPickerDialog::new(move |entry_id| {
                if let Some(controller) = weak.upgrade() {
                    controller.branch_from_message(&entry_id);
                }
            });
            self.branch_picker.replace(Some(picker));
        }
        let picker = self.branch_picker.borrow().as_ref().cloned();
        let Some(picker) = picker else {
            return;
        };
        picker.show_loading();
        picker.present(&self.ui.window);

        let Some(client) = self.active_client() else {
            picker.show_error("The omp bridge is not available.");
            return;
        };
        self.branch_busy.set(true);
        self.update_send_state();
        if let Err(error) = client.get_branch_messages() {
            self.branch_busy.set(false);
            picker.show_error(&error.to_string());
            self.finish_session_action();
        }
    }

    fn branch_from_message(&self, entry_id: &str) {
        if !self.session_actions_available() {
            return;
        }
        let Some(client) = self.active_client() else {
            return;
        };
        self.branch_busy.set(true);
        if let Some(picker) = self.branch_picker.borrow().as_ref() {
            picker.show_branching();
        }
        self.ui.chat_status.activity("Creating branch");
        self.update_send_state();
        if let Err(error) = client.branch(entry_id) {
            self.branch_busy.set(false);
            if let Some(picker) = self.branch_picker.borrow().as_ref() {
                picker.show_error(&error.to_string());
            }
            self.finish_session_action();
        }
    }

    fn present_handoff(self: &Rc<Self>) {
        if !self.session_actions_available() {
            return;
        }
        let weak = Rc::downgrade(self);
        session_actions::present_handoff(&self.ui.window, move |instructions| {
            if let Some(controller) = weak.upgrade() {
                controller.start_handoff(&instructions);
            }
        });
    }

    fn start_handoff(&self, instructions: &str) {
        if !self.session_actions_available() {
            return;
        }
        let Some(client) = self.active_client() else {
            return;
        };
        let instructions = instructions.trim();
        self.handoff_busy.set(true);
        self.ui.chat_status.activity("Generating handoff");
        self.update_send_state();
        if let Err(error) = client.handoff((!instructions.is_empty()).then_some(instructions)) {
            self.handoff_busy.set(false);
            self.finish_session_action();
            self.show_error(&error.to_string());
        }
    }

    fn present_history(self: &Rc<Self>) {
        let sessions =
            session_catalog::discover_all_sessions(self.current_session_file.borrow().as_deref());
        let active_paths = self
            .active_sessions
            .borrow()
            .iter()
            .filter_map(|entry| entry.path.clone())
            .collect::<HashSet<_>>();
        let weak = Rc::downgrade(self);
        sidebar::present_history(&self.ui.window, sessions, &active_paths, move |entry| {
            if let Some(controller) = weak.upgrade() {
                controller.open_session(&entry);
            }
        });
    }

    fn activate_runtime(self: &Rc<Self>, runtime_id: u64, entry: &SessionEntry) {
        let Some((ready, running, disconnected, cached_state)) =
            self.runtimes.borrow().get(&runtime_id).map(|runtime| {
                (
                    runtime.ready,
                    runtime.running,
                    runtime.disconnected.clone(),
                    runtime.state.clone(),
                )
            })
        else {
            return;
        };
        self.active_runtime.set(Some(runtime_id));
        self.unread_runtimes.borrow_mut().remove(&runtime_id);
        self.ready.set(ready);
        self.running.set(running);
        self.branch_busy.set(false);
        self.handoff_busy.set(false);
        self.pending_submissions.borrow_mut().clear();
        self.current_session_file.replace(entry.path.clone());
        self.current_session_title.replace(entry.title.clone());
        self.reset_message_bindings();
        let was_cached = self.show_runtime_conversation(runtime_id);
        self.clear_subagents();
        self.set_session_title(&entry.title);
        if let Some(state) = cached_state {
            self.apply_state(state);
        }
        self.ui.composer.set_input_sensitive(ready);
        if let Some(message) = disconnected {
            self.set_window_status(WindowStatus::Disconnected);
            self.ui.chat_status.disconnected();
            self.ui.conversation().show_disconnected(&message);
        } else {
            self.ui.chat_status.activity("Opening conversation");
            if !was_cached {
                self.ui.conversation().show_loading(
                    "Opening conversation",
                    "Restoring messages, tools, and session state.",
                    "Reading conversation history",
                );
            }
            if ready {
                self.refresh_after_session_change();
            }
        }
        self.update_send_state();
        self.update_activity_counts();
    }

    fn open_session(self: &Rc<Self>, entry: &SessionEntry) {
        if entry.current || self.branch_busy.get() || self.handoff_busy.get() {
            return;
        }
        if let Some(runtime_id) = entry.runtime_id {
            self.activate_runtime(runtime_id, entry);
            return;
        }
        let Some(path) = entry.path.as_deref() else {
            return;
        };
        match self.spawn_runtime(Some(path), entry.cwd.as_deref()) {
            Ok(runtime_id) => {
                let mut opened = entry.clone();
                opened.runtime_id = Some(runtime_id);
                opened.current = true;
                opened.running = false;
                update_current_session(&mut self.active_sessions.borrow_mut(), opened.clone());
                self.persist_recent_sessions();
                self.activate_runtime(runtime_id, &opened);
            }
            Err(error) => self.show_error(&format!("Could not start omp: {error}")),
        }
    }

    fn close_session(self: &Rc<Self>, entry: &SessionEntry) {
        if entry.current {
            self.alerts.play(SoundEvent::SessionEnd);
        }
        if let Some(runtime_id) = entry.runtime_id {
            self.runtimes.borrow_mut().remove(&runtime_id);
            self.unread_runtimes.borrow_mut().remove(&runtime_id);
            self.remove_runtime_conversation(runtime_id);
        }
        self.active_sessions
            .borrow_mut()
            .retain(|active| !identifies_same_session(active, entry));
        self.persist_recent_sessions();
        let next = entry
            .current
            .then(|| self.active_sessions.borrow().first().cloned())
            .flatten();
        if let Some(next) = next {
            self.open_session(&next);
        } else if entry.current {
            self.active_runtime.set(None);
            self.start_new_session_in(entry.cwd.as_deref());
        } else {
            self.render_session_sidebar(self.active_sessions.borrow().clone());
        }
        self.update_activity_counts();
    }

    fn present_delete_dialog(self: &Rc<Self>, entry: &SessionEntry) {
        let Some(path) = entry.path.clone() else {
            return;
        };
        let dialog = adw::AlertDialog::builder()
            .heading("Delete conversation?")
            .body("This permanently deletes the transcript and its subagent data.")
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        let current = entry.current;
        let weak = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            if response != "delete" {
                return;
            }
            let Some(controller) = weak.upgrade() else {
                return;
            };
            if current {
                let previous_runtime = controller.active_runtime.get();
                controller.start_new_session();
                if controller.active_runtime.get() == previous_runtime {
                    return;
                }
                if let Some(runtime_id) = previous_runtime {
                    controller.runtimes.borrow_mut().remove(&runtime_id);
                    controller.unread_runtimes.borrow_mut().remove(&runtime_id);
                    controller
                        .active_sessions
                        .borrow_mut()
                        .retain(|entry| entry.runtime_id != Some(runtime_id));
                    controller.remove_runtime_conversation(runtime_id);
                }
                controller.delete_closed_session(&path);
            } else {
                controller.delete_closed_session(&path);
            }
        });
        dialog.present(Some(&self.ui.window));
    }

    fn delete_closed_session(self: &Rc<Self>, path: &Path) {
        match session_catalog::delete_session_files(path) {
            Ok(()) => {
                self.active_sessions
                    .borrow_mut()
                    .retain(|entry| entry.path.as_deref() != Some(path));
                self.persist_recent_sessions();
                self.render_session_sidebar(self.active_sessions.borrow().clone());
            }
            Err(error) => self.show_error(&format!("Could not delete conversation: {error}")),
        }
    }

    fn present_rename_dialog(self: &Rc<Self>, entry: &SessionEntry) {
        let name = gtk::Entry::new();
        name.set_text(&entry.title);
        name.set_activates_default(true);
        name.set_max_length(120);
        let dialog = adw::AlertDialog::builder()
            .heading("Rename conversation")
            .body("Use a short title that makes this conversation easy to find.")
            .extra_child(&name)
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("rename", "Rename")]);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        let runtime_id = entry.runtime_id;
        let was_current = entry.current;
        let weak = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            if response != "rename" {
                return;
            }
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let title = name.text().trim().to_owned();
            if title.is_empty() {
                return;
            }
            let Some(client) =
                runtime_id.and_then(|runtime_id| controller.runtime_client(runtime_id))
            else {
                return;
            };
            match client.set_session_name(&title) {
                Ok(()) if was_current => controller.set_session_title(&title),
                Ok(()) => {
                    if let Some(entry) = controller
                        .active_sessions
                        .borrow_mut()
                        .iter_mut()
                        .find(|entry| entry.runtime_id == runtime_id)
                    {
                        entry.title = title.clone();
                    }
                    controller.render_session_sidebar(controller.active_sessions.borrow().clone());
                }
                Err(error) => controller.show_error(&error.to_string()),
            }
        });
        dialog.present(Some(&self.ui.window));
    }

    fn present_model_picker(self: &Rc<Self>) {
        if self.ui.composer.model_picker_visible() {
            self.ui.composer.close_model_picker();
            return;
        }
        let models = self.models.borrow().clone();
        if models.is_empty() {
            return;
        }
        let selected = self.current_model.borrow().clone();
        let weak = Rc::downgrade(self);
        let composer_for_select = self.ui.composer.clone();
        let composer_for_close = self.ui.composer.clone();
        let view = model_picker::ModelPickerView::new(
            models,
            selected,
            move |model| {
                composer_for_select.close_model_picker();
                if let Some(controller) = weak.upgrade() {
                    controller.choose_model(model);
                }
            },
            move || composer_for_close.close_model_picker(),
        );
        self.ui.composer.show_model_picker(view.widget());
        view.focus_search();
    }

    fn choose_model(&self, model: ModelSummary) {
        let Some(client) = self.active_client() else {
            return;
        };
        match client.set_model(&model.provider, &model.id) {
            Ok(()) => {
                self.current_model
                    .replace(Some((model.provider.clone(), model.id.clone())));
                self.show_model(&model);
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn apply_models(&self, models: Vec<ModelSummary>) {
        self.models.replace(models);
        self.ui
            .composer
            .set_model_sensitive(!self.models.borrow().is_empty());
        let current = self.current_model.borrow().clone();
        if let Some((provider, model_id)) = current {
            self.select_model_inner(&provider, &model_id);
        } else if let Some(model) = self.models.borrow().first() {
            self.show_model(model);
        }
    }

    fn select_model(&self, provider: &str, model_id: &str) {
        self.select_model_inner(provider, model_id);
    }

    fn select_model_inner(&self, provider: &str, model_id: &str) {
        let model = self
            .models
            .borrow()
            .iter()
            .find(|model| model.provider == provider && model.id == model_id)
            .cloned();
        if let Some(model) = model {
            self.current_model
                .replace(Some((model.provider.clone(), model.id.clone())));
            self.show_model(&model);
        }
    }

    fn show_model(&self, model: &ModelSummary) {
        self.ui
            .composer
            .set_model(&model.provider, model.display_name());
    }

    fn apply_thinking_levels(self: &Rc<Self>, mut efforts: Vec<String>, selected: Option<&str>) {
        efforts.retain(|effort| effort != "off" && effort != "inherit");
        efforts.insert(0, "off".to_owned());
        if let Some(selected) = selected
            && !efforts.iter().any(|effort| effort == selected)
        {
            efforts.push(selected.to_owned());
        }
        efforts.dedup();
        self.ui.composer.clear_thinking_options();
        self.thinking_buttons.borrow_mut().clear();
        self.thinking_levels.replace(efforts.clone());
        for level in &efforts {
            let button = composer::thinking_option(level);
            let requested = level.clone();
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                let Some(client) = controller.active_client() else {
                    return;
                };
                match client.set_thinking_level(&requested) {
                    Ok(()) => {
                        controller.select_thinking(&requested);
                        controller.ui.composer.close_thinking_popover();
                    }
                    Err(error) => controller.show_error(&error.to_string()),
                }
            });
            self.ui.composer.append_thinking_option(&button);
            self.thinking_buttons.borrow_mut().push(button);
        }
        self.ui.composer.set_thinking_sensitive(!efforts.is_empty());
        self.select_thinking_inner(selected.unwrap_or("off"));
    }

    fn select_thinking(&self, level: &str) {
        self.select_thinking_inner(level);
    }

    fn select_thinking_inner(&self, level: &str) {
        let Some(index) = self
            .thinking_levels
            .borrow()
            .iter()
            .position(|candidate| candidate == level)
        else {
            return;
        };
        self.ui.composer.set_thinking_label(level);
        for (button_index, button) in self.thinking_buttons.borrow().iter().enumerate() {
            if button_index == index {
                button.add_css_class("thinking-option-selected");
            } else {
                button.remove_css_class("thinking-option-selected");
            }
        }
    }

    fn hydrate_messages(self: &Rc<Self>, messages: &[Value]) {
        self.clear_messages();
        let mut cost = 0.0;
        for message in messages {
            if message.get("synthetic").and_then(Value::as_bool) == Some(true)
                || is_hidden_resume_message(message)
            {
                continue;
            }
            match message_role(message) {
                Some("user") => {
                    let text = message_text(message);
                    if has_visible_text(&text) {
                        self.finish_active_tool_group();
                        self.ui
                            .conversation()
                            .append_message(MessageRole::User, &text);
                    }
                }
                Some("assistant") => {
                    let thinking = message_thinking(message);
                    if has_visible_text(&thinking) {
                        self.append_grouped_thinking(&thinking, false);
                    }
                    for tool in message_tool_calls(message) {
                        self.ensure_grouped_tool(&tool);
                    }
                    let text = message_text(message);
                    if has_visible_text(&text) {
                        self.finish_active_tool_group();
                        self.ui
                            .conversation()
                            .append_message(MessageRole::Assistant, &text);
                    }
                    cost += message_cost(message);
                }
                Some("toolResult") => {
                    if let Some((id, name, result, is_error)) = tool_result_parts(message) {
                        self.tool_ended(
                            ToolEnd {
                                id,
                                name,
                                result,
                                is_error,
                            },
                            ToolResultSource::History,
                        );
                    }
                }
                Some("custom") | Some("developer") => {
                    let text = message_text(message);
                    if has_visible_text(&text) {
                        self.append_grouped_notice(&text, false);
                    }
                }
                _ => {}
            }
        }
        if should_finish_hydrated_tool_group(self.running.get()) {
            self.finish_active_tool_group();
        }
        self.session_cost.set(cost);
        self.ui.telemetry.set_cost(cost);
        if self.ui.conversation().is_empty() {
            self.show_empty_state();
        } else {
            self.remove_empty_state();
        }
        self.refresh_session_sidebar();
        self.scroll_to_bottom();
        self.update_send_state();
        if let Some(runtime_id) = self.active_runtime.get()
            && let Some(conversation) = self.conversation_views.borrow_mut().get_mut(&runtime_id)
        {
            conversation.hydrated = true;
        }
    }

    fn message_started(&self, message: &Value) {
        if matches!(message_role(message), Some("user" | "assistant")) {
            self.remove_empty_state();
        }
        match message_role(message) {
            Some("user") => {
                let text = message_text(message);
                if !has_visible_text(&text) {
                    return;
                }
                self.finish_active_tool_group();
                let matches_pending = {
                    let mut pending = self.pending_user_messages.borrow_mut();
                    if let Some(index) = pending.iter().position(|pending| pending == &text) {
                        pending.remove(index);
                        true
                    } else {
                        false
                    }
                };
                if !matches_pending {
                    self.ui
                        .conversation()
                        .append_message(MessageRole::User, &text);
                    self.scroll_to_bottom();
                }
            }
            Some("assistant") => {
                let thinking = message_thinking(message);
                self.streaming_thinking.borrow_mut().take();
                if has_visible_text(&thinking) {
                    self.streaming_thinking
                        .replace(Some(self.append_grouped_thinking(&thinking, true)));
                }
                let text = message_text(message);
                self.streaming_message.borrow_mut().take();
                if has_visible_text(&text) {
                    self.finish_active_tool_group();
                    let body = self
                        .ui
                        .conversation()
                        .append_streaming_message(MessageRole::Assistant, &text);
                    self.streaming_message
                        .replace(Some(StreamingMessage { body, text }));
                    self.scroll_to_bottom();
                }
            }
            _ => {}
        }
    }

    fn append_stream_delta(&self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let mut slot = self.streaming_message.borrow_mut();
        if slot.is_none() && !has_visible_text(delta) {
            return;
        }
        self.remove_empty_state();
        if slot.is_none() {
            self.finish_active_tool_group();
            let body = self
                .ui
                .conversation()
                .append_streaming_message(MessageRole::Assistant, "");
            *slot = Some(StreamingMessage {
                body,
                text: String::new(),
            });
        }
        let streaming = slot.as_mut().expect("streaming message exists");
        streaming.text.push_str(delta);
        streaming.body.set_streaming_text(&streaming.text);
        drop(slot);
        self.scroll_to_bottom();
    }

    fn append_thinking_delta(&self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let mut slot = self.streaming_thinking.borrow_mut();
        if slot.is_none() && !has_visible_text(delta) {
            return;
        }
        self.remove_empty_state();
        self.ui.chat_status.activity("Reasoning");
        if slot.is_none() {
            *slot = Some(self.append_grouped_thinking("", true));
        }
        slot.as_ref()
            .expect("streaming thinking exists")
            .append(delta);
        self.refresh_active_activity_summary();
        drop(slot);
        self.scroll_to_bottom();
    }

    fn message_ended(&self, message: &Value) {
        match message_role(message) {
            Some("assistant") => {
                let final_thinking = message_thinking(message);
                if let Some(thinking) = self.streaming_thinking.borrow_mut().take() {
                    thinking.finish(Some(&final_thinking));
                    self.refresh_active_activity_summary();
                } else if has_visible_text(&final_thinking) {
                    self.append_grouped_thinking(&final_thinking, false);
                }
                for tool in message_tool_calls(message) {
                    self.ensure_grouped_tool(&tool);
                }
                let final_text = message_text(message);
                let has_final_text = has_visible_text(&final_text);
                if has_final_text {
                    self.finish_active_tool_group();
                }
                if let Some(mut streaming) = self.streaming_message.borrow_mut().take() {
                    if has_final_text {
                        streaming.text = final_text;
                    }
                    if has_visible_text(&streaming.text) {
                        streaming.body.set_text(&streaming.text);
                    }
                } else if has_final_text {
                    self.ui
                        .conversation()
                        .append_message(MessageRole::Assistant, &final_text);
                }
                let cost = self.session_cost.get() + message_cost(message);
                self.session_cost.set(cost);
                self.ui.telemetry.set_cost(cost);
            }
            Some("toolResult") => {
                if let Some((id, name, result, is_error)) = tool_result_parts(message) {
                    self.tool_ended(
                        ToolEnd {
                            id,
                            name,
                            result,
                            is_error,
                        },
                        ToolResultSource::Live,
                    );
                }
            }
            Some("custom") | Some("developer") => {
                let text = message_text(message);
                if has_visible_text(&text) {
                    self.append_grouped_notice(&text, false);
                }
            }
            _ => {}
        }
        self.scroll_to_bottom();
    }

    fn tool_started(&self, tool: ToolStart) {
        if alerts::is_goal_completion(&tool.name, &tool.args) {
            self.goal_completion_calls
                .borrow_mut()
                .insert(tool.id.clone());
        }
        let (_, card) = self.ensure_grouped_tool(&tool);
        self.ui
            .chat_status
            .activity(&format!("Using {}", card.title.text()));
        self.scroll_to_bottom();
    }

    fn tool_updated(&self, tool: ToolUpdate) {
        let existing = self.tool_groups_by_call_id.borrow().get(&tool.id).cloned();
        let group = existing.unwrap_or_else(|| {
            self.ensure_grouped_tool(&ToolStart {
                id: tool.id.clone(),
                name: tool.name.clone(),
                args: Value::Null,
                intent: None,
            })
            .0
        });
        group.update_partial(&tool.id, &tool.partial_result);
    }

    fn tool_ended(&self, tool: ToolEnd, source: ToolResultSource) {
        let existing = self.tool_groups_by_call_id.borrow().get(&tool.id).cloned();
        let group = existing.unwrap_or_else(|| {
            self.ensure_grouped_tool(&ToolStart {
                id: tool.id.clone(),
                name: tool.name.clone(),
                args: Value::Null,
                intent: None,
            })
            .0
        });
        group.complete(&tool.id, &tool.result, tool.is_error);
        if !source.applies_live_effects() {
            return;
        }
        if tool.is_error {
            self.alerts
                .play(alerts::sound_event_for_error(&tool.result.to_string()));
        }
        if tool.name == "todo"
            && !tool.is_error
            && let Some(client) = self.active_client()
        {
            let _ = client.refresh_state();
        }
        let completed_goal =
            self.goal_completion_calls.borrow_mut().remove(&tool.id) && !tool.is_error;
        if completed_goal {
            self.goal_completed_this_run.set(true);
            self.set_window_status(WindowStatus::GoalComplete);
            self.alerts.play(SoundEvent::TaskComplete);
            self.send_alert(AlertKind::GoalComplete);
        }
        if self.running.get() {
            self.ui.chat_status.activity("Working");
        }
        self.scroll_to_bottom();
    }

    fn active_activity_group(&self) -> ToolActivityGroup {
        cloned_or_insert_with(&self.active_tool_group, || {
            let group = ToolActivityGroup::new();
            group.set_working(self.running.get());
            self.ui.conversation().append(&group.root);
            group
        })
    }

    fn append_grouped_thinking(&self, text: &str, streaming: bool) -> ThinkingBlock {
        let group = self.active_activity_group();
        group.append_thinking(text, streaming)
    }

    fn append_grouped_notice(&self, text: &str, is_error: bool) {
        let group = self.active_activity_group();
        group.append_notice(text, is_error);
    }

    fn refresh_active_activity_summary(&self) {
        if let Some(group) = self.active_tool_group.borrow().as_ref() {
            group.refresh_summary();
        }
    }

    fn ensure_grouped_tool(&self, tool: &ToolStart) -> (ToolActivityGroup, ToolCard) {
        if let Some(group) = self.tool_groups_by_call_id.borrow().get(&tool.id) {
            let group = group.clone();
            let card = group.ensure_card(&tool.id, &tool.name, &tool.args, tool.intent.as_deref());
            return (group, card);
        }

        let group = self.active_activity_group();
        let card = group.ensure_card(&tool.id, &tool.name, &tool.args, tool.intent.as_deref());
        self.tool_groups_by_call_id
            .borrow_mut()
            .insert(tool.id.clone(), group.clone());
        (group, card)
    }

    fn finish_active_tool_group(&self) {
        if let Some(group) = self.active_tool_group.borrow_mut().take() {
            group.finish();
        }
    }

    fn subagent_updated(self: &Rc<Self>, update: SubagentUpdate) {
        let updated_id = self.agent_hub.borrow_mut().apply_update(update);
        if updated_id.is_none() {
            return;
        }
        self.refresh_agent_surfaces();
        if self.active_subagent.borrow().as_deref() == updated_id.as_deref() {
            self.request_subagent_transcript();
        }
    }

    fn refresh_agent_surfaces(self: &Rc<Self>) {
        let (tree_rows, active_count, total_count, selected) = {
            let hub = self.agent_hub.borrow();
            let selected = self
                .active_subagent
                .borrow()
                .as_deref()
                .and_then(|id| hub.get(id))
                .cloned();
            (
                hub.visible_rows(&self.collapsed_agents.borrow()),
                hub.active_count(),
                hub.len(),
                selected,
            )
        };

        self.ui.agent_hub.clear_rows();
        let mut rendered_rows = Vec::with_capacity(tree_rows.len());
        for row in &tree_rows {
            let expanded = !self.collapsed_agents.borrow().contains(&row.agent.id);
            let rendered = agent_hub_ui::agent_row(row, expanded);
            let id = rendered.id.clone();
            let weak = Rc::downgrade(self);
            rendered.open.connect_clicked(move |_| {
                if let Some(controller) = weak.upgrade() {
                    controller.open_subagent_view(&id);
                }
            });
            if let Some(expander) = &rendered.expander {
                let id = rendered.id.clone();
                let weak = Rc::downgrade(self);
                expander.connect_toggled(move |button| {
                    if let Some(controller) = weak.upgrade() {
                        controller.set_agent_expanded(&id, button.is_active());
                    }
                });
            }
            self.ui.agent_hub.append_row(&rendered);
            rendered_rows.push(rendered);
        }
        self.agent_hub_rows.replace(rendered_rows);
        self.ui.agent_hub.set_counts(active_count, total_count);

        if let Some(agent) = selected {
            self.ui
                .agent_hub
                .select_id(&agent.id, &self.agent_hub_rows.borrow());
        } else {
            self.active_subagent.borrow_mut().take();
            self.subagent_transcript.borrow_mut().take();
            self.clear_subagent_tool_groups();
            self.ui.agent_hub.unselect_all();
        }
        self.update_activity_counts();
        if total_count == 0
            && self.ui.content_stack.visible_child_name().as_deref() == Some("subagent")
        {
            self.close_subagent_view();
        }
    }

    fn open_agent_hub(self: &Rc<Self>) {
        self.refresh_agent_surfaces();
        if !self.agent_hub.borrow().is_empty() {
            self.ui.agent_hub.set_revealed(true);
        }
    }

    fn set_agent_expanded(self: &Rc<Self>, id: &str, expanded: bool) {
        if expanded {
            self.collapsed_agents.borrow_mut().remove(id);
        } else {
            self.collapsed_agents.borrow_mut().insert(id.to_owned());
        }
        self.refresh_agent_surfaces();
    }

    fn open_subagent_view(self: &Rc<Self>, id: &str) {
        let Some(agent) = self.agent_hub.borrow().get(id).cloned() else {
            return;
        };
        self.open_agent_hub();
        self.ui.content_stack.set_visible_child_name("subagent");
        self.ui.back_button.set_visible(true);
        self.ui.composer.set_session_actions_visible(false);
        self.ui.composer_clamp.set_visible(false);
        if self.active_subagent.borrow().as_deref() == Some(id) {
            self.ui
                .agent_hub
                .select_id(id, &self.agent_hub_rows.borrow());
            self.request_subagent_transcript();
            return;
        }

        self.active_subagent.replace(Some(id.to_owned()));
        self.clear_subagent_tool_groups();
        self.ui.subagent_conversation.clear();
        self.ui.subagent_conversation.append_notice(
            &format!("Loading {}’s transcript…", agent.display_name()),
            false,
        );
        self.ui
            .agent_hub
            .select_id(id, &self.agent_hub_rows.borrow());
        self.ui
            .title
            .set_text(&format!("Agent · {}", agent.display_name()));
        self.subagent_transcript
            .replace(Some(SubagentTranscriptState {
                id: id.to_owned(),
                next_byte: 0,
                request_id: None,
                pending_refresh: false,
                has_content: false,
            }));
        self.request_subagent_transcript();
    }

    fn request_subagent_transcript(&self) {
        let historical_session =
            self.subagent_transcript
                .borrow()
                .as_ref()
                .and_then(|transcript| {
                    self.agent_hub
                        .borrow()
                        .get(&transcript.id)
                        .filter(|agent| agent.historical)
                        .and_then(|agent| agent.session_file.as_deref())
                        .map(PathBuf::from)
                        .map(|path| (transcript.id.clone(), path))
                });
        if let Some((id, path)) = historical_session {
            self.load_persisted_subagent_transcript(&id, &path);
            return;
        }

        let Some(client) = self.active_client() else {
            self.ui
                .subagent_conversation
                .append_notice("omp is not connected", true);
            return;
        };
        let (id, from_byte) = {
            let mut transcript = self.subagent_transcript.borrow_mut();
            let Some(transcript) = transcript.as_mut() else {
                return;
            };
            if transcript.request_id.is_some() {
                transcript.pending_refresh = true;
                return;
            }
            let from_byte = (transcript.next_byte > 0).then_some(transcript.next_byte);
            (transcript.id.clone(), from_byte)
        };

        match client.get_subagent_messages(&id, from_byte) {
            Ok(request_id) => {
                if let Some(transcript) = self.subagent_transcript.borrow_mut().as_mut()
                    && transcript.id == id
                {
                    transcript.request_id = Some(request_id);
                }
            }
            Err(error) => self.subagent_transcript_failed(None, &error.to_string()),
        }
    }

    fn load_persisted_subagent_transcript(&self, id: &str, path: &Path) {
        let messages = match session_catalog::read_session_messages(path) {
            Ok(messages) => messages,
            Err(error) => {
                self.subagent_transcript_failed(
                    None,
                    &format!("Could not read the saved agent transcript: {error}"),
                );
                return;
            }
        };
        {
            let mut transcript = self.subagent_transcript.borrow_mut();
            let Some(transcript) = transcript.as_mut().filter(|transcript| transcript.id == id)
            else {
                return;
            };
            transcript.request_id = None;
            transcript.pending_refresh = false;
            transcript.has_content = !messages.is_empty();
        }
        self.ui.subagent_conversation.clear();
        self.clear_subagent_tool_groups();
        if messages.is_empty() {
            self.ui.subagent_conversation.append_notice(
                "This saved agent has not produced transcript messages.",
                false,
            );
        } else {
            self.append_subagent_messages(&messages);
        }
        self.ui.subagent_conversation.scroll_to_bottom();
    }

    fn apply_subagent_transcript(
        &self,
        response_id: Option<&str>,
        transcript_response: SubagentMessages,
    ) {
        let expected_session = self.active_subagent.borrow().as_deref().and_then(|id| {
            self.agent_hub
                .borrow()
                .get(id)
                .and_then(|agent| agent.session_file.clone())
        });
        if expected_session
            .as_deref()
            .is_some_and(|session| session != transcript_response.session_file)
        {
            return;
        }

        let (clear_first, replace, messages, request_again) = {
            let mut transcript = self.subagent_transcript.borrow_mut();
            let Some(transcript) = transcript.as_mut() else {
                return;
            };
            if response_id.is_some() && transcript.request_id.as_deref() != response_id {
                return;
            }
            transcript.request_id = None;
            let replace = transcript_response.reset || transcript_response.from_byte == 0;
            transcript.next_byte = transcript_response.next_byte;
            let messages = transcript_response.messages;
            let clear_first = replace || (!transcript.has_content && !messages.is_empty());
            if replace {
                transcript.has_content = false;
            }
            if !messages.is_empty() {
                transcript.has_content = true;
            }
            let request_again = transcript.pending_refresh;
            transcript.pending_refresh = false;
            (clear_first, replace, messages, request_again)
        };

        if clear_first {
            self.ui.subagent_conversation.clear();
            if replace {
                self.clear_subagent_tool_groups();
            }
        }
        if !messages.is_empty() {
            self.append_subagent_messages(&messages);
        } else if replace {
            self.ui.subagent_conversation.append_notice(
                "This agent has not produced transcript messages yet.",
                false,
            );
        }
        self.ui.subagent_conversation.scroll_to_bottom();
        if request_again {
            self.request_subagent_transcript();
        }
    }

    fn subagent_transcript_failed(&self, response_id: Option<&str>, error: &str) {
        {
            let mut transcript = self.subagent_transcript.borrow_mut();
            let Some(transcript) = transcript.as_mut() else {
                return;
            };
            if response_id.is_some() && transcript.request_id.as_deref() != response_id {
                return;
            }
            transcript.request_id = None;
            transcript.pending_refresh = false;
        }
        self.ui.subagent_conversation.clear();
        self.clear_subagent_tool_groups();
        self.ui.subagent_conversation.append_notice(error, true);
    }

    fn append_subagent_messages(&self, messages: &[Value]) {
        for message in messages {
            match message_role(message) {
                Some("user") => {
                    let text = message_text(message);
                    if has_visible_text(&text) {
                        self.finish_subagent_tool_group();
                        self.ui
                            .subagent_conversation
                            .append_message(MessageRole::User, &text);
                    }
                }
                Some("assistant") => {
                    let thinking = message_thinking(message);
                    if has_visible_text(&thinking) {
                        self.append_subagent_grouped_thinking(&thinking);
                    }
                    for tool in message_tool_calls(message) {
                        self.ensure_subagent_grouped_tool(&tool);
                    }
                    let text = message_text(message);
                    if has_visible_text(&text) {
                        self.finish_subagent_tool_group();
                        self.ui
                            .subagent_conversation
                            .append_message(MessageRole::Assistant, &text);
                    }
                }
                Some("toolResult") => {
                    if let Some((id, name, result, is_error)) = tool_result_parts(message) {
                        let existing = self
                            .subagent_tool_groups_by_call_id
                            .borrow()
                            .get(&id)
                            .cloned();
                        let group = existing.unwrap_or_else(|| {
                            self.ensure_subagent_grouped_tool(&ToolStart {
                                id: id.clone(),
                                name,
                                args: Value::Null,
                                intent: None,
                            })
                        });
                        group.complete(&id, &result, is_error);
                    }
                }
                Some("custom") | Some("developer") => {
                    let text = message_text(message);
                    if has_visible_text(&text) {
                        self.append_subagent_grouped_notice(&text, false);
                    }
                }
                _ => {}
            }
        }
        let agent_active = self
            .active_subagent
            .borrow()
            .as_deref()
            .and_then(|id| {
                self.agent_hub
                    .borrow()
                    .get(id)
                    .map(|agent| agent.is_active())
            })
            .unwrap_or(false);
        if !agent_active {
            self.finish_subagent_tool_group();
        }
    }

    fn active_subagent_activity_group(&self) -> ToolActivityGroup {
        cloned_or_insert_with(&self.subagent_active_tool_group, || {
            let working = self
                .active_subagent
                .borrow()
                .as_deref()
                .and_then(|id| {
                    self.agent_hub
                        .borrow()
                        .get(id)
                        .map(|agent| agent.is_active())
                })
                .unwrap_or(false);
            let group = ToolActivityGroup::new();
            group.set_working(working);
            self.ui.subagent_conversation.append(&group.root);
            group
        })
    }

    fn append_subagent_grouped_thinking(&self, text: &str) {
        let group = self.active_subagent_activity_group();
        group.append_thinking(text, false);
    }

    fn append_subagent_grouped_notice(&self, text: &str, is_error: bool) {
        let group = self.active_subagent_activity_group();
        group.append_notice(text, is_error);
    }

    fn ensure_subagent_grouped_tool(&self, tool: &ToolStart) -> ToolActivityGroup {
        if let Some(group) = self.subagent_tool_groups_by_call_id.borrow().get(&tool.id) {
            return group.clone();
        }
        let group = self.active_subagent_activity_group();
        group.ensure_card(&tool.id, &tool.name, &tool.args, tool.intent.as_deref());
        self.subagent_tool_groups_by_call_id
            .borrow_mut()
            .insert(tool.id.clone(), group.clone());
        group
    }

    fn finish_subagent_tool_group(&self) {
        if let Some(group) = self.subagent_active_tool_group.borrow_mut().take() {
            group.finish();
        }
    }

    fn clear_subagent_tool_groups(&self) {
        self.subagent_active_tool_group.borrow_mut().take();
        self.subagent_tool_groups_by_call_id.borrow_mut().clear();
    }

    fn close_subagent_view(&self) {
        self.active_subagent.borrow_mut().take();
        self.subagent_transcript.borrow_mut().take();
        self.clear_subagent_tool_groups();
        self.ui.agent_hub.unselect_all();
        self.ui.content_stack.set_visible_child_name("chat");
        self.ui.back_button.set_visible(false);
        self.ui.composer_clamp.set_visible(true);
        self.ui.title.set_text(&self.current_session_title.borrow());
        self.update_send_state();
    }

    fn restore_persisted_subagents(self: &Rc<Self>) {
        let Some(path) = self.current_session_file.borrow().clone() else {
            self.hydrated_agent_session.borrow_mut().take();
            return;
        };
        if self.hydrated_agent_session.borrow().as_deref() == Some(path.as_path()) {
            return;
        }
        self.hydrated_agent_session.replace(Some(path.clone()));
        let Ok(agents) = session_catalog::read_session_subagents(&path) else {
            return;
        };
        if agents.is_empty() {
            return;
        }
        self.agent_hub.borrow_mut().apply_snapshot(agents);
        self.refresh_agent_surfaces();
    }

    fn clear_subagents(&self) {
        self.agent_hub.borrow_mut().clear();
        self.agent_hub_rows.borrow_mut().clear();
        self.collapsed_agents.borrow_mut().clear();
        self.hydrated_agent_session.borrow_mut().take();
        self.active_subagent.borrow_mut().take();
        self.subagent_transcript.borrow_mut().take();
        self.clear_subagent_tool_groups();
        self.ui.agent_hub.clear_rows();
        self.ui.agent_hub.set_counts(0, 0);
        self.ui.agent_hub.unselect_all();
        if self.ui.content_stack.visible_child_name().as_deref() == Some("subagent") {
            self.ui.content_stack.set_visible_child_name("chat");
            self.ui.back_button.set_visible(false);
            self.ui.composer_clamp.set_visible(true);
        }
        self.update_send_state();
        self.update_activity_counts();
    }

    fn update_activity_counts(&self) {
        let (active_agents, total_agents) = {
            let hub = self.agent_hub.borrow();
            (hub.active_count(), hub.len())
        };
        self.ui.set_agent_hub_activity(active_agents, total_agents);

        let running_sessions = self
            .runtimes
            .borrow()
            .values()
            .filter(|runtime| runtime.running)
            .count();
        let unread_sessions = self.unread_runtimes.borrow().len();
        let active_items = active_agents + running_sessions;
        self.ui
            .sidebar_activity_count
            .set_text(&format!("{running_sessions} active"));
        self.ui
            .sidebar_activity_count
            .set_tooltip_text(Some(&format!(
                "{running_sessions} active {}",
                if running_sessions == 1 {
                    "session"
                } else {
                    "sessions"
                }
            )));
        if running_sessions > 0 {
            self.ui.sidebar_activity_count.add_css_class("has-items");
        } else {
            self.ui.sidebar_activity_count.remove_css_class("has-items");
        }
        self.ui
            .sidebar_unread_count
            .set_text(&format!("{unread_sessions} unread"));
        self.ui.sidebar_unread_count.set_tooltip_text(Some(&format!(
            "{unread_sessions} unread {}",
            if unread_sessions == 1 {
                "session"
            } else {
                "sessions"
            }
        )));
        if unread_sessions > 0 {
            self.ui.sidebar_unread_count.add_css_class("has-items");
        } else {
            self.ui.sidebar_unread_count.remove_css_class("has-items");
        }
        let unread_runtimes = self.unread_runtimes.borrow();
        for session in self.session_rows.borrow().iter() {
            let unread = session
                .entry
                .runtime_id
                .is_some_and(|runtime_id| unread_runtimes.contains(&runtime_id));
            if unread {
                session.row.add_css_class("unread-session");
            } else {
                session.row.remove_css_class("unread-session");
            }
            session.indicator.queue_draw();
            if session.entry.current {
                session.badge.set_visible(active_agents > 0);
                session.badge.set_text(&active_agents.to_string());
                session
                    .badge
                    .set_tooltip_text(Some("Active subagents in this conversation"));
            }
        }
        if active_items > 0 {
            self.set_window_status(WindowStatus::Working);
        } else if self.window_status.get() == WindowStatus::Working {
            self.set_window_status(WindowStatus::Ready);
        }
    }

    fn stop_current_turn(&self) {
        if !self.running.get() {
            return;
        }
        if let Some(client) = self.active_client() {
            match client.abort() {
                Ok(()) => self.ui.chat_status.activity("Stopping"),
                Err(error) => self.show_error(&error.to_string()),
            }
        }
    }

    fn start_new_session(self: &Rc<Self>) {
        let workspace = self.current_workspace();
        self.start_new_session_in(workspace.as_deref());
    }

    fn start_new_session_in(self: &Rc<Self>, workspace: Option<&Path>) {
        if self.branch_busy.get() || self.handoff_busy.get() {
            return;
        }
        let workspace = workspace
            .map(Path::to_owned)
            .or_else(|| std::env::current_dir().ok());
        match self.spawn_runtime(None, workspace.as_deref()) {
            Ok(runtime_id) => {
                self.goal_completed_this_run.set(false);
                self.goal_completion_calls.borrow_mut().clear();
                self.current_session_file.borrow_mut().take();
                let mut entry = session_catalog::session_entry(
                    None,
                    "New conversation",
                    true,
                    workspace.as_deref(),
                );
                entry.runtime_id = Some(runtime_id);
                insert_session_by_creation(&mut self.active_sessions.borrow_mut(), entry.clone());
                self.activate_runtime(runtime_id, &entry);
                self.ui.chat_status.activity("Starting conversation");
                self.ui.conversation().show_loading(
                    "Creating a new conversation",
                    "Preparing a fresh omp session in this workspace.",
                    "Starting the local runtime",
                );
            }
            Err(error) => self.show_error(&format!("Could not start omp: {error}")),
        }
    }

    fn submit_current(&self) {
        self.submit_current_with_intent(SubmissionIntent::Default);
    }

    fn submit_current_with_intent(&self, intent: SubmissionIntent) {
        if self.branch_busy.get()
            || self.handoff_busy.get()
            || !self.ready.get()
            || !self.pending_submissions.borrow().is_empty()
        {
            return;
        }
        let draft_text = self.ui.composer.text();
        let message = draft_text.trim().to_owned();
        let Some(client) = self.active_client() else {
            return;
        };
        if let Some(error) = unsupported_native_mode_error(&message) {
            self.show_error(&error);
            return;
        }
        let resume = is_resume_shortcut(&message);
        if message.is_empty() && self.attachments.borrow().is_empty() {
            return;
        }
        let request = if resume {
            client.resume()
        } else {
            let attachments = self.attachments.borrow();
            let action = SubmissionAction::select(self.running.get(), intent);
            match action {
                SubmissionAction::Prompt => client.prompt(&message, attachments.images()),
                SubmissionAction::Steer => client.steer(&message, attachments.images()),
                SubmissionAction::FollowUp => client.follow_up(&message, attachments.images()),
            }
        };
        match request {
            Ok(request_id) => {
                let attachment_ids = if resume {
                    Vec::new()
                } else {
                    self.attachments.borrow().ids().collect::<Vec<_>>()
                };
                let submitted_message = if resume {
                    MANUAL_CONTINUE_PROMPT.to_owned()
                } else {
                    message.clone()
                };
                let displayed_message = (!resume).then(|| {
                    let body = self
                        .ui
                        .conversation()
                        .append_message(MessageRole::User, &message);
                    self.scroll_to_bottom();
                    body
                });
                self.pending_user_messages
                    .borrow_mut()
                    .push_back(submitted_message.clone());
                self.pending_submissions
                    .borrow_mut()
                    .push_back(PendingSubmission {
                        request_id,
                        draft_text,
                        message: submitted_message,
                        displayed_message,
                        attachment_ids,
                    });
                self.ui.chat_status.activity("Sending");
                self.hide_completions();
                self.update_send_state();
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn render_authoritative_queue_state(&self) {
        self.ui
            .composer
            .set_queued_message_count(self.queued_message_count.get());
    }

    fn session_actions_available(&self) -> bool {
        session_actions_are_relevant(
            self.ready.get(),
            self.current_session_file.borrow().is_some(),
            !self.ui.conversation().is_empty(),
            self.running.get(),
            self.branch_busy.get() || self.handoff_busy.get(),
            self.active_subagent.borrow().is_some(),
            self.ui.content_stack.visible_child_name().as_deref() == Some("chat"),
        )
    }

    fn update_send_state(&self) {
        let ready = self.ready.get();
        let running = self.running.get();
        let session_busy = self.branch_busy.get() || self.handoff_busy.get();
        let primary_ready = ready && !session_busy && self.pending_submissions.borrow().is_empty();
        self.ui.composer.set_primary_action(primary_ready, running);
        self.ui
            .composer
            .set_attachment_sensitive(ready && !session_busy);
        self.ui
            .composer
            .set_submission_pending(!self.pending_submissions.borrow().is_empty());
        self.ui
            .telemetry
            .cwd_button
            .set_sensitive(ready && !running && !session_busy);
        let session_actions_available = self.session_actions_available();
        self.ui
            .composer
            .set_session_actions_visible(session_actions_available);
    }

    fn update_completions(&self) {
        let text = self.ui.composer.text();
        let candidates = completions(&text, &self.commands.borrow());
        self.completion_items.replace(candidates);
        self.completion_index.set(0);
        self.ui.composer.clear_completion_rows();
        for completion in self.completion_items.borrow().iter() {
            self.ui
                .composer
                .append_completion_row(&completion_row(completion));
        }
        if self.ui.composer.select_completion(0) {
            self.ui.composer.show_completions();
        } else {
            self.hide_completions();
        }
    }

    fn move_completion(&self, direction: isize) {
        let count = self.completion_items.borrow().len();
        if count == 0 {
            return;
        }
        let index =
            (self.completion_index.get() as isize + direction).rem_euclid(count as isize) as usize;
        self.completion_index.set(index);
        self.ui.composer.select_completion(index as i32);
    }

    fn accept_completion(&self, submission: Option<SubmissionIntent>) {
        let Some(completion) = self
            .completion_items
            .borrow()
            .get(self.completion_index.get())
            .cloned()
        else {
            return;
        };
        self.ui.composer.set_text(&completion.replacement);
        self.ui.composer.focus();
        if let Some(intent) = submission
            && !completion.replacement.ends_with(' ')
        {
            self.hide_completions();
            self.submit_current_with_intent(intent);
        }
    }

    fn hide_completions(&self) {
        self.ui.composer.hide_completions();
    }

    fn reset_message_bindings(&self) {
        self.streaming_message.borrow_mut().take();
        self.streaming_thinking.borrow_mut().take();
        self.active_tool_group.borrow_mut().take();
        self.tool_groups_by_call_id.borrow_mut().clear();
        self.pending_user_messages.borrow_mut().clear();
    }

    fn clear_messages(&self) {
        self.ui.conversation().clear();
        self.reset_message_bindings();
        if let Some(runtime_id) = self.active_runtime.get()
            && let Some(conversation) = self.conversation_views.borrow_mut().get_mut(&runtime_id)
        {
            conversation.hydrated = false;
        }
    }

    fn show_empty_state(self: &Rc<Self>) {
        let current_workspace = self.current_workspace();
        let sessions =
            session_catalog::discover_all_sessions(self.current_session_file.borrow().as_deref());
        let recent_workspaces =
            session_catalog::recent_workspaces(&sessions, current_workspace.as_deref(), 3);
        let weak = Rc::downgrade(self);
        let select_workspace = move |path: PathBuf| {
            if let Some(controller) = weak.upgrade() {
                controller.move_current_session_to(&path);
            }
        };
        let weak = Rc::downgrade(self);
        let browse = move || {
            if let Some(controller) = weak.upgrade() {
                controller.present_workspace_picker();
            }
        };
        self.ui.conversation().show_workspace_onboarding(
            &recent_workspaces,
            current_workspace.as_deref(),
            select_workspace,
            browse,
        );
    }

    fn remove_empty_state(&self) {
        self.ui.conversation().hide_empty();
    }

    fn scroll_to_bottom(&self) {
        self.ui.conversation().scroll_to_bottom();
    }

    fn tick_sound_events(&self) {
        if !self.running.get() {
            return;
        }
        let now = Instant::now();
        if self
            .last_task_progress
            .get()
            .is_some_and(|last| now.duration_since(last) >= TASK_PROGRESS_INTERVAL)
        {
            self.last_task_progress.set(Some(now));
            self.alerts.play(SoundEvent::TaskProgress);
        }
    }

    fn record_prompt_sound(&self) {
        let now = Instant::now();
        let mut prompts = self.recent_prompts.borrow_mut();
        while prompts
            .front()
            .is_some_and(|sent| now.duration_since(*sent) > PROMPT_BURST_WINDOW)
        {
            prompts.pop_front();
        }
        prompts.push_back(now);
        if prompts.len() >= PROMPT_BURST_THRESHOLD {
            prompts.clear();
            self.alerts.play(SoundEvent::UserSpam);
        }
    }

    fn show_error(&self, message: &str) {
        self.alerts.play(alerts::sound_event_for_error(message));
        if !self.ready.get() && self.ui.conversation().is_empty() {
            self.ui.conversation().show_disconnected(message);
        } else {
            self.ui.conversation().append_notice(message, true);
            self.scroll_to_bottom();
        }
    }
    fn set_window_status(&self, status: WindowStatus) {
        self.window_status.set(status);
        self.refresh_window_title();
    }

    fn refresh_window_title(&self) {
        let title = alerts::window_title(
            self.window_status.get(),
            &self.current_session_title.borrow(),
        );
        self.ui.window.set_title(Some(&title));
    }

    fn send_alert(&self, kind: AlertKind) {
        self.alerts.notify(
            kind,
            &self.current_session_title.borrow(),
            !self.ui.window.is_active(),
        );
    }

    fn present_settings(self: &Rc<Self>) {
        let preferences = self.alerts.preferences();
        let settings = sound_settings::SettingsDialog::new(
            &preferences,
            &self.sound_pack_choices(),
            self.alerts.installed_pack_count(),
        );
        let weak = Rc::downgrade(self);
        settings.connect_steering_mode_changed(move |mode| {
            if let Some(controller) = weak.upgrade() {
                controller.set_global_steering_mode(mode);
            }
        });

        let weak = Rc::downgrade(self);
        settings.connect_follow_up_mode_changed(move |mode| {
            if let Some(controller) = weak.upgrade() {
                controller.set_global_follow_up_mode(mode);
            }
        });

        let weak = Rc::downgrade(self);
        settings.connect_interrupt_mode_changed(move |mode| {
            if let Some(controller) = weak.upgrade() {
                controller.set_global_interrupt_mode(mode);
            }
        });

        let weak = Rc::downgrade(self);
        settings
            .desktop_notifications
            .connect_active_notify(move |row| {
                if let Some(controller) = weak.upgrade()
                    && let Err(error) = controller.alerts.set_desktop_notifications(row.is_active())
                {
                    controller.show_error(&error);
                }
            });

        let weak = Rc::downgrade(self);
        let settings_for_toggle = settings.clone();
        settings.sounds.connect_active_notify(move |row| {
            settings_for_toggle.set_sounds_enabled(row.is_active());
            if let Some(controller) = weak.upgrade()
                && let Err(error) = controller.alerts.set_sounds(row.is_active())
            {
                controller.show_error(&error);
            }
        });

        let weak = Rc::downgrade(self);
        settings.volume.connect_value_changed(move |scale| {
            if let Some(controller) = weak.upgrade()
                && let Err(error) = controller.alerts.set_volume(scale.value() / 100.0)
            {
                controller.show_error(&error);
            }
        });

        for event_row in settings.event_rows.iter() {
            let weak = Rc::downgrade(self);
            event_row.connect_changed(move |event, pack_id| {
                if let Some(controller) = weak.upgrade()
                    && let Err(error) = controller.alerts.set_event_pack(event, pack_id.as_deref())
                {
                    controller.show_error(&error);
                }
            });
            let weak = Rc::downgrade(self);
            event_row.connect_preview(move |event, pack_id| {
                if let Some(controller) = weak.upgrade() {
                    controller.alerts.preview(event, &pack_id);
                }
            });
        }

        let weak = Rc::downgrade(self);
        let settings_for_browser = settings.clone();
        settings.browse_packs.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.present_sound_pack_browser(&settings_for_browser);
            }
        });
        settings.present(&self.ui.window);
    }

    fn sound_pack_choices(&self) -> HashMap<SoundEvent, Vec<SoundPackChoice>> {
        SoundEvent::ALL
            .into_iter()
            .map(|event| (event, self.alerts.sound_pack_choices(event)))
            .collect()
    }

    fn present_sound_pack_browser(self: &Rc<Self>, settings: &sound_settings::SettingsDialog) {
        let browser = sound_settings::PackBrowserDialog::new();
        let weak = Rc::downgrade(self);
        let browser_for_retry = browser.clone();
        let settings_for_retry = settings.clone();
        browser.connect_retry(move || {
            if let Some(controller) = weak.upgrade() {
                controller.load_sound_pack_registry(
                    settings_for_retry.clone(),
                    browser_for_retry.clone(),
                );
            }
        });
        browser.present(&self.ui.window);
        self.load_sound_pack_registry(settings.clone(), browser);
    }

    fn load_sound_pack_registry(
        self: &Rc<Self>,
        settings: sound_settings::SettingsDialog,
        browser: sound_settings::PackBrowserDialog,
    ) {
        browser.show_loading();
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(sound_registry::fetch_registry());
        });
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Ok(result) = receiver.recv().await else {
                browser.show_error("The sound pack catalog stopped responding.");
                return;
            };
            let packs = match result {
                Ok(packs) => packs,
                Err(error) => {
                    browser.show_error(&error);
                    return;
                }
            };
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let installed = controller.alerts.installed_pack_names();
            let weak = Rc::downgrade(&controller);
            let settings_for_install = settings.clone();
            let browser_for_install = browser.clone();
            browser.set_packs(&packs, &installed, move |pack, button| {
                if let Some(controller) = weak.upgrade() {
                    controller.install_sound_pack(
                        pack,
                        button,
                        settings_for_install.clone(),
                        browser_for_install.clone(),
                    );
                }
            });
        });
    }

    fn install_sound_pack(
        self: &Rc<Self>,
        pack: RegistryPack,
        button: gtk::Button,
        settings: sound_settings::SettingsDialog,
        browser: sound_settings::PackBrowserDialog,
    ) {
        sound_settings::PackBrowserDialog::set_installing(&button);
        let installed_name = pack.display_name.clone();
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(sound_registry::install_pack(&pack));
        });
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = match receiver.recv().await {
                Ok(result) => result,
                Err(_) => Err("The sound pack installation stopped unexpectedly.".to_owned()),
            };
            let Some(controller) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(_) => {
                    controller.alerts.refresh_sound_packs();
                    sound_settings::PackBrowserDialog::set_installed(&button);
                    settings.refresh_packs(
                        &controller.alerts.preferences(),
                        &controller.sound_pack_choices(),
                        controller.alerts.installed_pack_count(),
                    );
                    browser
                        .dialog
                        .add_toast(adw::Toast::new(&format!("Installed {installed_name}")));
                }
                Err(error) => browser.set_install_error(&button, &error),
            }
        });
    }

    fn handle_extension_ui(self: &Rc<Self>, request: Value) {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match method {
            "notify" => {
                let message = request
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let is_error = request.get("notifyType").and_then(Value::as_str) == Some("error");
                if is_error {
                    self.alerts.play(alerts::sound_event_for_error(message));
                }
                self.ui.conversation().append_notice(message, is_error);
            }
            "setTitle" => {
                if let Some(title) = request.get("title").and_then(Value::as_str) {
                    self.ui.title.set_text(title);
                }
            }
            "set_editor_text" => {
                if let Some(text) = request.get("text").and_then(Value::as_str) {
                    self.ui.composer.set_text(text);
                    self.ui.composer.focus();
                }
            }
            "setStatus" => self.update_extension_status(&request),
            "setWidget" => self.update_extension_widget(&request),
            "cancel" => {
                if let Some(target_id) = request.get("targetId").and_then(Value::as_str)
                    && let Some(dialog) = self.extension_dialogs.borrow_mut().remove(target_id)
                {
                    dialog.force_close();
                }
            }
            "open_url" => {
                if let Some(url) = request
                    .get("launchUrl")
                    .or_else(|| request.get("url"))
                    .and_then(Value::as_str)
                    && let Err(error) =
                        gio::AppInfo::launch_default_for_uri(url, gio::AppLaunchContext::NONE)
                {
                    self.show_error(&format!("Could not open URL: {error}"));
                }
            }
            "confirm" | "select" | "input" | "editor" => {
                self.alerts.play(SoundEvent::InputRequired);
                self.present_extension_dialog(request);
            }
            _ => {}
        }
    }

    fn update_extension_status(&self, request: &Value) {
        let Some(key) = request.get("statusKey").and_then(Value::as_str) else {
            return;
        };
        let mut statuses = self.extension_statuses.borrow_mut();
        match request.get("statusText").and_then(Value::as_str) {
            Some(text) if !text.is_empty() => {
                statuses.insert(key.to_owned(), text.to_owned());
            }
            _ => {
                statuses.remove(key);
            }
        }
        let mut entries = statuses.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(left, _)| *left);
        let text = entries
            .into_iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>()
            .join(" · ");
        self.ui.composer.set_extension_status(&text);
    }

    fn update_extension_widget(&self, request: &Value) {
        let Some(key) = request.get("widgetKey").and_then(Value::as_str) else {
            return;
        };
        if let Some(label) = self.extension_widgets.borrow_mut().remove(key) {
            self.ui.composer.remove_extension_widget(&label);
        }

        let Some(lines) = request.get("widgetLines").and_then(Value::as_array) else {
            return;
        };
        let text = lines
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return;
        }
        let label = gtk::Label::new(Some(&text));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_selectable(true);
        label.add_css_class("extension-widget");
        let below_editor =
            request.get("widgetPlacement").and_then(Value::as_str) == Some("belowEditor");
        self.ui
            .composer
            .append_extension_widget(&label, below_editor);
        self.extension_widgets
            .borrow_mut()
            .insert(key.to_owned(), label);
    }

    fn present_extension_dialog(self: &Rc<Self>, request: Value) {
        let id = request
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let title = request
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("omp")
            .to_owned();
        let body = request
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let dialog = adw::AlertDialog::new(Some(&title), Some(&body));
        dialog.set_close_response("cancel");
        dialog.add_response("cancel", "Cancel");

        let mut options = Vec::new();
        let mut entry = None;
        let mut editor = None;
        match method.as_str() {
            "confirm" => {
                dialog.add_response("confirm", "Continue");
                dialog.set_default_response(Some("confirm"));
                dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
            }
            "select" => {
                options = request
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect();
                for (index, option) in options.iter().enumerate() {
                    dialog.add_response(&format!("option-{index}"), option);
                }
            }
            "input" => {
                let widget = gtk::Entry::new();
                widget.set_placeholder_text(request.get("placeholder").and_then(Value::as_str));
                widget.set_activates_default(true);
                dialog.set_extra_child(Some(&widget));
                dialog.add_response("submit", "Submit");
                dialog.set_default_response(Some("submit"));
                entry = Some(widget);
            }
            "editor" => {
                let widget = gtk::TextView::new();
                widget.set_wrap_mode(gtk::WrapMode::WordChar);
                widget.set_size_request(520, 180);
                widget.buffer().set_text(
                    request
                        .get("prefill")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                );
                let scroll = gtk::ScrolledWindow::builder()
                    .min_content_height(180)
                    .child(&widget)
                    .build();
                dialog.set_extra_child(Some(&scroll));
                dialog.add_response("submit", "Submit");
                dialog.set_default_response(Some("submit"));
                editor = Some(widget);
            }
            _ => return,
        }

        self.extension_dialogs
            .borrow_mut()
            .insert(id.clone(), dialog.clone());
        let dialog_id = id.clone();
        let weak: Weak<Self> = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            controller
                .extension_dialogs
                .borrow_mut()
                .remove(&dialog_id);
            let payload = if response == "cancel" {
                json!({ "type": "extension_ui_response", "id": id, "cancelled": true })
            } else if method == "confirm" {
                json!({ "type": "extension_ui_response", "id": id, "confirmed": response == "confirm" })
            } else if method == "select" {
                let index = response
                    .strip_prefix("option-")
                    .and_then(|value| value.parse::<usize>().ok());
                match index.and_then(|index| options.get(index)) {
                    Some(value) => json!({ "type": "extension_ui_response", "id": id, "value": value }),
                    None => json!({ "type": "extension_ui_response", "id": id, "cancelled": true }),
                }
            } else if let Some(entry) = entry.as_ref() {
                json!({ "type": "extension_ui_response", "id": id, "value": entry.text().to_string() })
            } else if let Some(editor) = editor.as_ref() {
                let buffer = editor.buffer();
                let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                json!({ "type": "extension_ui_response", "id": id, "value": value.to_string() })
            } else {
                json!({ "type": "extension_ui_response", "id": id, "cancelled": true })
            };
            if let Some(client) = controller.active_client() { let _ = client.respond_to_extension(payload); }
        });
        dialog.present(Some(&self.ui.window));
    }
}

fn completion_row(completion: &CommandCompletion) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 3);
    content.set_margin_top(9);
    content.set_margin_bottom(9);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let heading = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::new(Some(&completion.label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.add_css_class("completion-command");
    heading.append(&label);
    if let Some(detail) = completion.detail.as_deref() {
        let detail = gtk::Label::new(Some(detail));
        detail.add_css_class("completion-detail");
        heading.append(&detail);
    }
    let description = gtk::Label::new(Some(&completion.description));
    description.set_xalign(0.0);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    description.add_css_class("completion-description");
    content.append(&heading);
    content.append(&description);
    row.set_child(Some(&content));
    row
}

fn compact_path(path: &Path) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home).to_string_lossy().into_owned();
        if let Some(relative) = text.strip_prefix(&home) {
            text = format!("~{relative}");
        }
    }
    if text.chars().count() <= 54 {
        return text;
    }
    let parts = text.rsplit('/').take(3).collect::<Vec<_>>();
    format!(
        "…/{}",
        parts.into_iter().rev().collect::<Vec<_>>().join("/")
    )
}

async fn read_stream_bytes(stream: &gio::InputStream) -> Result<Vec<u8>, glib::Error> {
    let mut output = Vec::new();
    loop {
        let bytes = stream
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await?;
        if bytes.is_empty() {
            return Ok(output);
        }
        output.extend_from_slice(bytes.as_ref());
    }
}

async fn encode_image_in_background(
    bytes: Vec<u8>,
) -> Result<(crate::bridge::protocol::ImageContent, Vec<u8>), String> {
    gio::spawn_blocking(move || {
        let image = attachments::encode_image(&bytes)?;
        Ok((image, bytes))
    })
    .await
    .map_err(|_| "Image encoding task panicked".to_owned())?
}
#[cfg(test)]
mod tests {
    use super::{
        ComposerKeyAction, DeliveryModes, SubmissionAction, SubmissionIntent, ToolResultSource,
        WARM_SESSION_VIEW_LIMIT, cloned_or_insert_with, completion_is_unread, composer_key_action,
        delivery_preference_updates, gdk, has_visible_text, identifies_same_session,
        insert_session_by_creation, is_hidden_resume_message, is_resume_shortcut,
        messages_need_hydration, reorder_session_entries, session_actions_are_relevant,
        should_finish_hydrated_tool_group, should_send_delivery_preference_updates,
        touch_bounded_lru, update_current_session,
    };
    use crate::alerts::Preferences;
    use crate::bridge::protocol::{InterruptMode, QueueMode, SessionState};
    use crate::session_catalog::{SessionEntry, session_entry};
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::time::{Duration, SystemTime};

    #[test]
    fn tabbing_back_to_streaming_session_keeps_rolling_tool_group_open() {
        assert!(!should_finish_hydrated_tool_group(true));
        assert!(should_finish_hydrated_tool_group(false));
    }

    #[test]
    fn warm_session_cache_evicts_the_least_recently_viewed_session() {
        let mut order = VecDeque::from([1, 2, 3, 4]);

        assert!(touch_bounded_lru(&mut order, 2, WARM_SESSION_VIEW_LIMIT).is_empty());
        assert_eq!(order, [1, 3, 4, 2]);
        assert_eq!(
            touch_bounded_lru(&mut order, 5, WARM_SESSION_VIEW_LIMIT),
            [1]
        );
        assert_eq!(order, [3, 4, 2, 5]);
    }

    #[test]
    fn unchanged_messages_keep_the_warm_view_intact() {
        let messages = vec![json!({"role": "user", "content": "hello"})];

        assert!(!messages_need_hydration(Some(&messages), true, &messages));
        assert!(messages_need_hydration(Some(&messages), false, &messages));
        assert!(messages_need_hydration(
            Some(&messages),
            true,
            &[json!({"role": "assistant", "content": "updated"})],
        ));
    }

    #[test]
    fn whitespace_only_messages_do_not_split_tool_activity() {
        assert!(!has_visible_text(""));
        assert!(!has_visible_text(" \n\t"));
        assert!(has_visible_text("\nResult ready\n"));
    }

    #[test]
    fn completed_sessions_are_unread_until_viewed() {
        assert!(completion_is_unread(true, Some(1), 2, true));
        assert!(completion_is_unread(true, Some(1), 1, false));
        assert!(!completion_is_unread(true, Some(1), 1, true));
        assert!(!completion_is_unread(false, Some(1), 2, true));
    }

    #[test]
    fn hydrated_tool_results_do_not_apply_live_completion_effects() {
        assert!(!ToolResultSource::History.applies_live_effects());
        assert!(ToolResultSource::Live.applies_live_effects());
    }
    #[test]
    fn active_group_initializer_can_access_its_refcell() {
        let slot = RefCell::new(None);
        let value = cloned_or_insert_with(&slot, || {
            assert!(slot.try_borrow_mut().is_ok());
            42
        });

        assert_eq!(value, 42);
        assert_eq!(*slot.borrow(), Some(42));
    }

    #[test]
    fn composer_shortcuts_choose_delivery_without_persistent_view_state() {
        assert_eq!(
            composer_key_action(gdk::Key::Return, gdk::ModifierType::empty()),
            Some(ComposerKeyAction::Submit)
        );
        assert_eq!(
            composer_key_action(gdk::Key::Return, gdk::ModifierType::CONTROL_MASK),
            Some(ComposerKeyAction::FollowUp)
        );
        assert_eq!(
            composer_key_action(gdk::Key::q, gdk::ModifierType::CONTROL_MASK),
            Some(ComposerKeyAction::FollowUp)
        );
        assert_eq!(
            composer_key_action(gdk::Key::Return, gdk::ModifierType::SHIFT_MASK),
            Some(ComposerKeyAction::Newline)
        );
        assert_eq!(
            composer_key_action(gdk::Key::Return, gdk::ModifierType::ALT_MASK),
            Some(ComposerKeyAction::Newline)
        );
    }

    #[test]
    fn submission_intent_maps_idle_text_to_prompt_and_running_text_to_delivery() {
        assert_eq!(
            SubmissionAction::select(false, SubmissionIntent::FollowUp),
            SubmissionAction::Prompt
        );
        assert_eq!(
            SubmissionAction::select(true, SubmissionIntent::Default),
            SubmissionAction::Steer
        );
        assert_eq!(
            SubmissionAction::select(true, SubmissionIntent::FollowUp),
            SubmissionAction::FollowUp
        );
    }

    #[test]
    fn dot_is_the_hidden_manual_resume_shortcut() {
        assert!(is_resume_shortcut("."));
        assert!(!is_resume_shortcut("c"));
        assert!(!is_resume_shortcut("continue"));

        let hidden = json!({
            "role": "user",
            "content": crate::bridge::MANUAL_CONTINUE_PROMPT,
        });
        assert!(is_hidden_resume_message(&hidden));
        assert!(!is_hidden_resume_message(&json!({
            "role": "user",
            "content": ".",
        })));
        assert!(!is_hidden_resume_message(&json!({
            "role": "assistant",
            "content": crate::bridge::MANUAL_CONTINUE_PROMPT,
        })));
    }

    #[test]
    fn session_actions_require_an_idle_persisted_conversation_with_messages() {
        assert!(session_actions_are_relevant(
            true, true, true, false, false, false, true,
        ));
        assert!(!session_actions_are_relevant(
            true, true, false, false, false, false, true,
        ));

        assert!(!session_actions_are_relevant(
            true, true, true, true, false, false, true,
        ));
        assert!(!session_actions_are_relevant(
            true, true, true, false, false, true, false,
        ));
    }

    #[test]
    fn sessions_start_newest_first_and_focus_does_not_reorder_them() {
        let mut entries = Vec::new();
        insert_session_by_creation(&mut entries, test_session(1, 10, false));
        insert_session_by_creation(&mut entries, test_session(3, 30, false));
        insert_session_by_creation(&mut entries, test_session(2, 20, false));
        assert_eq!(runtime_order(&entries), [3, 2, 1]);

        update_current_session(&mut entries, test_session(1, 99, true));
        assert_eq!(runtime_order(&entries), [3, 2, 1]);
        assert!(entries[2].current);
        assert_eq!(
            entries[2].created_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(10)
        );
    }

    #[test]
    fn restored_session_is_reused_when_the_runtime_reports_its_path() {
        let path = std::path::Path::new("/tmp/omp-restored-session.jsonl");
        let mut restored = session_entry(Some(path), "Restored", false, None);
        restored.created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let mut current = session_entry(Some(path), "Current", true, None);
        current.runtime_id = Some(1);

        let mut entries = vec![restored];
        update_current_session(&mut entries, current);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].runtime_id, Some(1));
        assert!(entries[0].current);
        assert_eq!(
            entries[0].created_at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(10)
        );
    }

    #[test]
    fn closing_one_restored_session_keeps_the_other_cold_sessions() {
        let first = session_entry(
            Some(std::path::Path::new("/tmp/omp-first-session.jsonl")),
            "First",
            false,
            None,
        );
        let second = session_entry(
            Some(std::path::Path::new("/tmp/omp-second-session.jsonl")),
            "Second",
            false,
            None,
        );
        let mut entries = vec![first.clone(), second.clone()];

        entries.retain(|entry| !identifies_same_session(entry, &first));

        assert_eq!(entries, [second]);
    }

    #[test]
    fn dragged_sessions_move_before_or_after_the_drop_target() {
        let mut entries = vec![
            test_session(3, 30, false),
            test_session(2, 20, false),
            test_session(1, 10, true),
        ];

        assert!(reorder_session_entries(&mut entries, 1, 3, false));
        assert_eq!(runtime_order(&entries), [1, 3, 2]);
        assert!(reorder_session_entries(&mut entries, 1, 2, true));
        assert_eq!(runtime_order(&entries), [3, 2, 1]);
    }

    #[test]
    fn global_delivery_preferences_override_different_session_values_only() {
        let preferences: Preferences = serde_json::from_value(json!({
            "steering_mode": "all",
            "follow_up_mode": "one-at-a-time",
            "interrupt_mode": "wait"
        }))
        .expect("deserialize global delivery preferences");
        let state: SessionState = serde_json::from_value(json!({
            "steeringMode": "one-at-a-time",
            "followUpMode": "one-at-a-time",
            "interruptMode": "immediate"
        }))
        .expect("deserialize session state");

        let updates = delivery_preference_updates(&preferences, &state);
        assert_eq!(updates.steering_mode, Some(QueueMode::All));
        assert_eq!(updates.follow_up_mode, None);
        assert_eq!(updates.interrupt_mode, Some(InterruptMode::Wait));
        let desired = DeliveryModes::from(&preferences);
        assert!(should_send_delivery_preference_updates(
            None, desired, updates
        ));
        assert!(!should_send_delivery_preference_updates(
            Some(desired),
            desired,
            updates,
        ));
    }

    fn test_session(runtime_id: u64, created_at: u64, current: bool) -> SessionEntry {
        let mut entry = session_entry(None, &format!("Session {runtime_id}"), current, None);
        entry.runtime_id = Some(runtime_id);
        entry.created_at = SystemTime::UNIX_EPOCH + Duration::from_secs(created_at);
        entry
    }

    fn runtime_order(entries: &[SessionEntry]) -> Vec<u64> {
        entries
            .iter()
            .map(|entry| entry.runtime_id.expect("test session runtime"))
            .collect()
    }
}
