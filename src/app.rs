use crate::alerts::{self, AlertKind, Alerts, SoundEvent, SoundPackChoice, WindowStatus};
use crate::sound_registry::{self, RegistryPack};
use crate::ui::{
    chat, composer, model_picker, sidebar, sound_settings, tool_components, workspace,
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

use crate::bridge::protocol::{
    InterruptMode, ModelSummary, QueueMode, RpcEvent, RpcResponse, SessionState, SlashCommand,
    SubagentUpdate, SubagentUpdateKind, ToolEnd, ToolStart, ToolUpdate, message_cost,
    message_role, message_text, message_thinking, message_tool_calls, tool_result_parts,
};
use crate::bridge::{BridgeClient, OmpBridge};
use crate::commands::{CommandCompletion, completions};
use crate::session_catalog::{self, SessionEntry};
use chat::{MessageBody, MessageRole, ThinkingBlock};
use sidebar::SessionRow;
use tool_components::ToolCard;
use workspace::WorkspaceView;

const TASK_PROGRESS_INTERVAL: Duration = Duration::from_secs(30);
const PROMPT_BURST_WINDOW: Duration = Duration::from_secs(5);
const PROMPT_BURST_THRESHOLD: usize = 3;

pub(crate) fn build(app: &adw::Application) {
    let ui = workspace::build(app);

    let (bridge, client, events) = match OmpBridge::spawn() {
        Ok(bridge) => {
            let client = bridge.client.clone();
            let events = bridge.events.clone();
            (Some(bridge), Some(client), Some(events))
        }
        Err(error) => (None, None, {
            ui.composer.set_input_sensitive(false);
            ui.conversation
                .append_notice(&format!("Could not start omp: {error}"), true);
            None
        }),
    };

    let alerts = Alerts::new(app);
    let present_action = gio::SimpleAction::new("present", None);
    let window = ui.window.clone();
    present_action.connect_activate(move |_, _| window.present());
    app.add_action(&present_action);

    let controller = Rc::new(AppController {
        ui,
        bridge: RefCell::new(bridge),
        alerts,
        client,
        models: RefCell::new(Vec::new()),
        commands: RefCell::new(Vec::new()),
        thinking_levels: RefCell::new(Vec::new()),
        thinking_buttons: RefCell::new(Vec::new()),
        completion_items: RefCell::new(Vec::new()),
        completion_index: Cell::new(0),
        pending_user_messages: RefCell::new(VecDeque::new()),
        streaming_message: RefCell::new(None),
        pending_submission: RefCell::new(None),
        streaming_thinking: RefCell::new(None),
        tool_cards: RefCell::new(HashMap::new()),
        subagents: RefCell::new(HashMap::new()),
        subagent_buttons: RefCell::new(HashMap::new()),
        session_rows: RefCell::new(Vec::new()),
        active_sessions: RefCell::new(Vec::new()),
        pending_delete: RefCell::new(None),
        current_session_file: RefCell::new(None),
        current_session_title: RefCell::new("New conversation".to_owned()),
        active_subagent: RefCell::new(None),
        session_cost: Cell::new(0.0),
        extension_widgets: RefCell::new(HashMap::new()),
        extension_statuses: RefCell::new(HashMap::new()),
        extension_dialogs: RefCell::new(HashMap::new()),
        current_model: RefCell::new(None),
        ready: Cell::new(false),
        running: Cell::new(false),
        running_turn_action: Cell::new(RunningTurnAction::Steer),
        steering_mode: Cell::new(QueueMode::OneAtATime),
        follow_up_mode: Cell::new(QueueMode::OneAtATime),
        interrupt_mode: Cell::new(InterruptMode::Immediate),
        queued_message_count: Cell::new(0),
        reconciling_queue_state: Cell::new(false),
        goal_completion_calls: RefCell::new(HashSet::new()),
        goal_completed_this_run: Cell::new(false),
        window_status: Cell::new(WindowStatus::Ready),
        session_sound_started: Cell::new(false),
        last_task_progress: Cell::new(None),
        recent_prompts: RefCell::new(VecDeque::new()),
    });

    controller.wire_interactions();
    controller.set_window_status(WindowStatus::Ready);
    if let Some(events) = events {
        AppController::run_event_loop(&controller, events);
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
enum RunningTurnAction {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmissionAction {
    Prompt,
    Steer,
    FollowUp,
}

impl SubmissionAction {
    fn select(running: bool, running_turn_action: RunningTurnAction) -> Self {
        if !running {
            return Self::Prompt;
        }
        match running_turn_action {
            RunningTurnAction::Steer => Self::Steer,
            RunningTurnAction::FollowUp => Self::FollowUp,
        }
    }
}

struct PendingSubmission {
    request_id: String,
    draft: String,
    message: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReconciledComposerState {
    running_turn_action: RunningTurnAction,
    steering_mode: QueueMode,
    follow_up_mode: QueueMode,
    interrupt_mode: InterruptMode,
    queued_message_count: usize,
}

fn reconcile_composer_state(
    running_turn_action: RunningTurnAction,
    state: &SessionState,
) -> ReconciledComposerState {
    ReconciledComposerState {
        running_turn_action,
        steering_mode: state.steering_mode,
        follow_up_mode: state.follow_up_mode,
        interrupt_mode: state.interrupt_mode,
        queued_message_count: state.queued_message_count,
    }
}



#[derive(Clone)]
struct SubagentState {
    id: String,
    display_name: String,
    status: String,
    task: String,
    active: bool,
}

struct AppController {
    ui: WorkspaceView,
    alerts: Alerts,
    bridge: RefCell<Option<OmpBridge>>,
    client: Option<BridgeClient>,
    models: RefCell<Vec<ModelSummary>>,
    commands: RefCell<Vec<SlashCommand>>,
    thinking_levels: RefCell<Vec<String>>,
    thinking_buttons: RefCell<Vec<gtk::Button>>,
    completion_items: RefCell<Vec<CommandCompletion>>,
    completion_index: Cell<usize>,
    pending_user_messages: RefCell<VecDeque<String>>,
    pending_submission: RefCell<Option<PendingSubmission>>,
    streaming_message: RefCell<Option<StreamingMessage>>,
    streaming_thinking: RefCell<Option<ThinkingBlock>>,
    tool_cards: RefCell<HashMap<String, ToolCard>>,
    subagents: RefCell<HashMap<String, SubagentState>>,
    subagent_buttons: RefCell<HashMap<String, gtk::Button>>,
    session_rows: RefCell<Vec<SessionRow>>,
    active_sessions: RefCell<Vec<SessionEntry>>,
    pending_delete: RefCell<Option<PathBuf>>,
    current_session_file: RefCell<Option<PathBuf>>,
    current_session_title: RefCell<String>,
    active_subagent: RefCell<Option<String>>,
    session_cost: Cell<f64>,
    extension_widgets: RefCell<HashMap<String, gtk::Label>>,
    extension_statuses: RefCell<HashMap<String, String>>,
    extension_dialogs: RefCell<HashMap<String, adw::AlertDialog>>,
    current_model: RefCell<Option<(String, String)>>,
    ready: Cell<bool>,
    running: Cell<bool>,
    running_turn_action: Cell<RunningTurnAction>,
    steering_mode: Cell<QueueMode>,
    follow_up_mode: Cell<QueueMode>,
    interrupt_mode: Cell<InterruptMode>,
    queued_message_count: Cell<usize>,
    reconciling_queue_state: Cell<bool>,
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
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::n {
                controller.start_new_session();
                return glib::Propagation::Stop;
            }
            if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
                if enter_inserts_newline(modifiers) {
                    return glib::Propagation::Proceed;
                }
                if controller.ui.composer.completions_visible() {
                    controller.accept_completion(true);
                } else {
                    controller.submit_current();
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
                    controller.accept_completion(false);
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
        self.ui.composer.connect_steer_selected(move || {
            if let Some(controller) = weak.upgrade() {
                controller
                    .running_turn_action
                    .set(RunningTurnAction::Steer);
                controller.update_send_state();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_follow_up_selected(move || {
            if let Some(controller) = weak.upgrade() {
                controller
                    .running_turn_action
                    .set(RunningTurnAction::FollowUp);
                controller.update_send_state();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.composer.connect_steering_mode_changed(move |mode| {
            if let Some(controller) = weak.upgrade() {
                controller.request_steering_mode(mode);
            }
        });

        let weak = Rc::downgrade(self);
        self.ui
            .composer
            .connect_follow_up_mode_changed(move |mode| {
                if let Some(controller) = weak.upgrade() {
                    controller.request_follow_up_mode(mode);
                }
            });

        let weak = Rc::downgrade(self);
        self.ui
            .composer
            .connect_interrupt_mode_changed(move |mode| {
                if let Some(controller) = weak.upgrade() {
                    controller.request_interrupt_mode(mode);
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
                controller.present_alert_preferences();
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
            if window.is_active()
                && let Some(controller) = weak.upgrade()
            {
                controller.alerts.withdraw();
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
        self.ui.back_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.close_subagent_view();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.session_list.connect_row_activated(move |_, row| {
            let Some(controller) = weak.upgrade() else {
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
            controller.accept_completion(false);
        });

        let weak = Rc::downgrade(self);
        self.ui.window.connect_close_request(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.alerts.play(SoundEvent::SessionEnd);
                if let Some(bridge) = controller.bridge.borrow().as_ref() {
                    bridge.shutdown();
                }
            }
            glib::Propagation::Proceed
        });
    }

    fn run_event_loop(this: &Rc<Self>, events: async_channel::Receiver<RpcEvent>) {
        let controller = this.clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                controller.handle_event(event);
            }
        });
    }

    fn handle_event(self: &Rc<Self>, event: RpcEvent) {
        match event {
            RpcEvent::Ready => {
                if !self.session_sound_started.replace(true) {
                    self.alerts.play(SoundEvent::SessionStart);
                }
                if let Some(client) = &self.client
                    && let Err(error) = client.initialize()
                {
                    self.show_error(&error.to_string());
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
                }
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
                if let Some(client) = &self.client {
                    let _ = client.refresh_state();
                }
            }
            RpcEvent::ToolStart(tool) => self.tool_started(tool),
            RpcEvent::ToolUpdate(tool) => self.tool_updated(tool),
            RpcEvent::ToolEnd(tool) => self.tool_ended(tool),
            RpcEvent::Subagent(update) => self.subagent_updated(update),
            RpcEvent::CommandOutput(text) => {
                if !text.is_empty() {
                    self.remove_empty_state();
                    self.ui
                        .conversation
                        .append_message(MessageRole::Assistant, &text);
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
                if let Some(client) = &self.client {
                    let _ = client.refresh_state();
                }
            }
            RpcEvent::Notice { level, message } => {
                self.ui
                    .conversation
                    .append_notice(&message, level == "error");
                self.scroll_to_bottom();
            }
            RpcEvent::ModelChanged => {
                if let Some(client) = &self.client {
                    let _ = client.refresh_state();
                }
            }
            RpcEvent::ThinkingChanged(level) => {
                if let Some(level) = level {
                    self.select_thinking(&level);
                } else if let Some(client) = &self.client {
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
                self.pending_submission.borrow_mut().take();
                self.show_error(&message);
            }
            RpcEvent::Other => {}
        }
    }

    fn handle_response(self: &Rc<Self>, response: RpcResponse) {
        if matches!(response.command.as_str(), "prompt" | "steer" | "follow_up") {
            self.finish_submission(&response);
        }
        if !response.success {
            if response.command == "new_session" {
                self.pending_delete.borrow_mut().take();
            }
            if matches!(
                response.command.as_str(),
                "set_steering_mode" | "set_follow_up_mode" | "set_interrupt_mode"
            ) {
                self.render_authoritative_queue_state();
            }
            self.show_error(
                response
                    .error
                    .as_deref()
                    .unwrap_or("omp rejected the request"),
            );
            return;
        }
        if matches!(
            response.command.as_str(),
            "set_steering_mode" | "set_follow_up_mode" | "set_interrupt_mode"
        ) && let Some(client) = &self.client
        {
            let _ = client.refresh_state();
        }
        let Some(data) = response.data else {
            if response.command == "new_session" {
                self.refresh_after_new_session();
            }
            return;
        };

        match response.command.as_str() {
            "get_state" => {
                if let Ok(state) = serde_json::from_value::<SessionState>(data) {
                    self.apply_state(state);
                }
            }
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
                let messages = data
                    .get("messages")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                self.hydrate_messages(&messages);
            }
            "get_subagents" => {
                if let Some(agents) = data.get("subagents").and_then(Value::as_array) {
                    for agent in agents {
                        self.subagent_updated(SubagentUpdate {
                            kind: SubagentUpdateKind::Lifecycle,
                            id: agent
                                .get("id")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            agent: agent
                                .get("agent")
                                .or_else(|| agent.get("name"))
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            status: agent
                                .get("status")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                            task: agent
                                .get("task")
                                .or_else(|| agent.get("activity"))
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned),
                        });
                    }
                }
            }
            "get_subagent_messages" => {
                let messages = data
                    .get("messages")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                self.hydrate_subagent_messages(&messages);
            }
            "prompt" => {
                if data.get("agentInvoked").and_then(Value::as_bool) == Some(false) {
                    self.pending_user_messages.borrow_mut().pop_front();
                }
            }
            "new_session" => self.refresh_after_new_session(),
            "switch_session" | "set_session_name" => self.refresh_after_session_change(),
            _ => {}
        }
    }
    fn finish_submission(&self, response: &RpcResponse) {
        let matches_pending = self
            .pending_submission
            .borrow()
            .as_ref()
            .is_some_and(|pending| {
                response
                    .id
                    .as_deref()
                    .map_or(true, |id| id == pending.request_id)
            });
        if !matches_pending {
            return;
        }
        let pending = self
            .pending_submission
            .borrow_mut()
            .take()
            .expect("matched pending submission exists");
        if response.success {
            self.goal_completed_this_run.set(false);
            self.goal_completion_calls.borrow_mut().clear();
            self.record_prompt_sound();
            self.set_window_status(WindowStatus::Working);
            self.remove_empty_state();
            self.ui
                .conversation
                .append_message(MessageRole::User, &pending.message);
            self.pending_user_messages
                .borrow_mut()
                .push_back(pending.message);
            if self.ui.composer.text() == pending.draft {
                self.ui.composer.set_text("");
            }
            self.hide_completions();
            self.scroll_to_bottom();
            if let Some(client) = &self.client {
                let _ = client.refresh_state();
            }
        }
        self.update_send_state();
    }

    fn refresh_after_new_session(self: &Rc<Self>) {
        if let Some(path) = self.pending_delete.borrow_mut().take() {
            if let Err(error) = session_catalog::delete_session_files(&path) {
                self.show_error(&format!("Could not delete conversation: {error}"));
            }
            self.active_sessions
                .borrow_mut()
                .retain(|entry| entry.path.as_deref() != Some(path.as_path()));
        }
        self.current_session_file.borrow_mut().take();
        self.current_session_title
            .replace("New conversation".to_owned());
        self.session_cost.set(0.0);
        self.goal_completed_this_run.set(false);
        self.goal_completion_calls.borrow_mut().clear();
        self.alerts.play(SoundEvent::SessionStart);
        self.set_window_status(WindowStatus::Ready);
        self.clear_messages();
        self.clear_subagents();
        self.set_session_title("New conversation");
        self.ui
            .conversation
            .append_notice("Starting a new conversation…", false);
        self.refresh_session_sidebar();
        self.refresh_after_session_change();
    }

    fn refresh_after_session_change(&self) {
        if let Some(client) = &self.client {
            let _ = client.refresh_state();
            let _ = client.refresh_messages();
            let _ = client.refresh_subagents();
        }
    }

    fn apply_state(self: &Rc<Self>, state: SessionState) {
        self.ready.set(true);
        self.running.set(state.is_streaming);
        self.ui.composer.set_input_sensitive(true);
        let composer_state = reconcile_composer_state(self.running_turn_action.get(), &state);
        self.running_turn_action
            .set(composer_state.running_turn_action);
        self.steering_mode.set(composer_state.steering_mode);
        self.follow_up_mode.set(composer_state.follow_up_mode);
        self.interrupt_mode.set(composer_state.interrupt_mode);
        self.queued_message_count
            .set(composer_state.queued_message_count);
        self.render_authoritative_queue_state();

        let session_file = state.session_file.as_deref().map(PathBuf::from);
        let disk_title = session_file
            .as_deref()
            .and_then(session_catalog::read_session_title);
        self.current_session_file.replace(session_file.clone());
        let resolved = session_catalog::authoritative_title(
            state.session_name.as_deref(),
            disk_title.as_deref(),
        );
        let title = session_catalog::authoritative_title(
            Some(&resolved),
            Some(&self.current_session_title.borrow()),
        );
        self.set_session_title(&title);
        let current_entry = session_catalog::session_entry(session_file.as_deref(), &title, true);
        if let Some(cwd) = current_entry.cwd.as_deref() {
            self.ui.telemetry.cwd.set_text(&compact_path(cwd));
            self.ui
                .telemetry
                .cwd
                .set_tooltip_text(Some(&cwd.to_string_lossy()));
        } else {
            self.ui.telemetry.cwd.set_text("No workspace");
            self.ui.telemetry.cwd.set_tooltip_text(None);
        }

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

    fn set_session_title(&self, title: &str) {
        let title = if title.trim().is_empty() {
            "New conversation"
        } else {
            title.trim()
        };
        self.current_session_title.replace(title.to_owned());
        if self.active_subagent.borrow().is_none() {
            self.ui.title.set_text(title);
        }
        self.refresh_window_title();
    }

    fn refresh_titles_from_disk(self: &Rc<Self>) {
        let current_path = self.current_session_file.borrow().clone();
        let current_title = self.current_session_title.borrow().clone();
        let entries = self
            .active_sessions
            .borrow()
            .iter()
            .map(|entry| {
                let current = entry.path == current_path;
                session_catalog::session_entry(
                    entry.path.as_deref(),
                    if current {
                        &current_title
                    } else {
                        &entry.title
                    },
                    current,
                )
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

    fn refresh_session_sidebar(self: &Rc<Self>) {
        let current = session_catalog::session_entry(
            self.current_session_file.borrow().as_deref(),
            &self.current_session_title.borrow(),
            true,
        );
        let mut entries = self.active_sessions.borrow_mut();
        if current.path.is_some() {
            entries.retain(|entry| entry.path.is_some());
        }
        for entry in entries.iter_mut() {
            entry.current = false;
        }
        if let Some(position) = entries.iter().position(|entry| entry.path == current.path) {
            entries.remove(position);
        }
        entries.insert(0, current);
        let rendered = entries.clone();
        drop(entries);
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

    fn present_workspace_picker(self: &Rc<Self>) {
        if !self.ready.get() || self.running.get() {
            return;
        }

        let current_workspace = session_catalog::session_entry(
            self.current_session_file.borrow().as_deref(),
            &self.current_session_title.borrow(),
            true,
        )
        .cwd
        .or_else(|| std::env::current_dir().ok());
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
                if current_workspace.as_deref() == Some(path.as_path()) {
                    return;
                }
                let Some(client) = &controller.client else {
                    return;
                };
                if let Err(error) = client.move_session(&path) {
                    controller.show_error(&error.to_string());
                }
            },
        );
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

    fn open_session(self: &Rc<Self>, entry: &SessionEntry) {
        if entry.current {
            return;
        }
        let Some(path) = entry.path.as_deref() else {
            return;
        };
        let Some(client) = &self.client else {
            return;
        };
        match client.switch_session(path) {
            Ok(()) => {
                let mut sessions = self.active_sessions.borrow_mut();
                if !sessions.iter().any(|active| active.path == entry.path) {
                    let mut opened = entry.clone();
                    opened.current = false;
                    sessions.insert(0, opened);
                }
                let entries = sessions.clone();
                drop(sessions);
                self.render_session_sidebar(entries);
                self.ui.chat_status.activity("Opening conversation");
                self.clear_messages();
                self.clear_subagents();
                self.set_session_title(&entry.title);
                self.ui
                    .conversation
                    .append_notice("Loading conversation…", false);
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn close_session(self: &Rc<Self>, entry: &SessionEntry) {
        if entry.current {
            self.alerts.play(SoundEvent::SessionEnd);
        }
        self.active_sessions
            .borrow_mut()
            .retain(|active| active.path != entry.path);
        let next = entry
            .current
            .then(|| self.active_sessions.borrow().first().cloned())
            .flatten();
        self.render_session_sidebar(self.active_sessions.borrow().clone());
        if let Some(next) = next {
            self.open_session(&next);
        } else if entry.current {
            self.start_new_session();
        }
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
                let Some(client) = &controller.client else {
                    return;
                };
                match client.new_session() {
                    Ok(()) => {
                        controller.pending_delete.replace(Some(path.clone()));
                        controller.ui.chat_status.activity("Deleting conversation");
                    }
                    Err(error) => controller.show_error(&error.to_string()),
                }
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
        let session_path = entry.path.clone();
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
            let Some(client) = &controller.client else {
                return;
            };
            if !was_current
                && let Some(path) = session_path.as_deref()
                && let Err(error) = client.switch_session(path)
            {
                controller.show_error(&error.to_string());
                return;
            }
            match client.set_session_name(&title) {
                Ok(()) => controller.set_session_title(&title),
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
        let Some(client) = &self.client else {
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
                let Some(client) = &controller.client else {
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
            if message.get("synthetic").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            match message_role(message) {
                Some("user") => {
                    let text = message_text(message);
                    if !text.is_empty() {
                        self.ui
                            .conversation
                            .append_message(MessageRole::User, &text);
                    }
                }
                Some("assistant") => {
                    let thinking = message_thinking(message);
                    if !thinking.is_empty() {
                        self.ui.conversation.append_thinking(&thinking, false);
                    }
                    let text = message_text(message);
                    if !text.is_empty() {
                        self.ui
                            .conversation
                            .append_message(MessageRole::Assistant, &text);
                    }
                    for tool in message_tool_calls(message) {
                        self.ensure_tool_card(&tool);
                    }
                    cost += message_cost(message);
                }
                Some("toolResult") => {
                    if let Some((id, name, result, is_error)) = tool_result_parts(message) {
                        self.tool_ended(ToolEnd {
                            id,
                            name,
                            result,
                            is_error,
                        });
                    }
                }
                Some("custom") | Some("developer") => {
                    let text = message_text(message);
                    if !text.is_empty() {
                        self.ui.conversation.append_notice(&text, false);
                    }
                }
                _ => {}
            }
        }
        self.session_cost.set(cost);
        self.ui.telemetry.set_cost(cost);
        if self.ui.conversation.is_empty() {
            self.show_empty_state();
        } else {
            self.remove_empty_state();
        }
        self.refresh_session_sidebar();
        self.scroll_to_bottom();
    }

    fn message_started(&self, message: &Value) {
        if matches!(message_role(message), Some("user" | "assistant")) {
            self.remove_empty_state();
        }
        match message_role(message) {
            Some("user") => {
                let text = message_text(message);
                let matches_pending = {
                    let mut pending = self.pending_user_messages.borrow_mut();
                    if let Some(index) = pending.iter().position(|pending| pending == &text) {
                        pending.remove(index);
                        true
                    } else {
                        false
                    }
                };
                if !matches_pending && !text.is_empty() {
                    self.ui
                        .conversation
                        .append_message(MessageRole::User, &text);
                    self.scroll_to_bottom();
                }
            }
            Some("assistant") => {
                let thinking = message_thinking(message);
                self.streaming_thinking.borrow_mut().take();
                if !thinking.is_empty() {
                    self.streaming_thinking
                        .replace(Some(self.ui.conversation.append_thinking(&thinking, true)));
                }
                let text = message_text(message);
                self.streaming_message.borrow_mut().take();
                if !text.is_empty() {
                    let body = self
                        .ui
                        .conversation
                        .append_message(MessageRole::Assistant, &text);
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
        self.remove_empty_state();
        let mut slot = self.streaming_message.borrow_mut();
        if slot.is_none() {
            let body = self
                .ui
                .conversation
                .append_message(MessageRole::Assistant, "");
            *slot = Some(StreamingMessage {
                body,
                text: String::new(),
            });
        }
        let streaming = slot.as_mut().expect("streaming message exists");
        streaming.text.push_str(delta);
        streaming.body.set_text(&streaming.text);
        drop(slot);
        self.scroll_to_bottom();
    }

    fn append_thinking_delta(&self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.remove_empty_state();
        self.ui.chat_status.activity("Reasoning");
        let mut slot = self.streaming_thinking.borrow_mut();
        if slot.is_none() {
            *slot = Some(self.ui.conversation.append_thinking("", true));
        }
        slot.as_ref()
            .expect("streaming thinking exists")
            .append(delta);
        drop(slot);
        self.scroll_to_bottom();
    }

    fn message_ended(&self, message: &Value) {
        match message_role(message) {
            Some("assistant") => {
                let final_thinking = message_thinking(message);
                if let Some(thinking) = self.streaming_thinking.borrow_mut().take() {
                    thinking.finish(Some(&final_thinking));
                } else if !final_thinking.is_empty() {
                    self.ui.conversation.append_thinking(&final_thinking, false);
                }
                let final_text = message_text(message);
                if let Some(mut streaming) = self.streaming_message.borrow_mut().take() {
                    if !final_text.is_empty() && final_text != streaming.text {
                        streaming.text = final_text;
                        streaming.body.set_text(&streaming.text);
                    }
                } else if !final_text.is_empty() {
                    self.ui
                        .conversation
                        .append_message(MessageRole::Assistant, &final_text);
                }
                for tool in message_tool_calls(message) {
                    self.ensure_tool_card(&tool);
                }
                let cost = self.session_cost.get() + message_cost(message);
                self.session_cost.set(cost);
                self.ui.telemetry.set_cost(cost);
            }
            Some("toolResult") => {
                if let Some((id, name, result, is_error)) = tool_result_parts(message) {
                    self.tool_ended(ToolEnd {
                        id,
                        name,
                        result,
                        is_error,
                    });
                }
            }
            Some("custom") | Some("developer") => {
                let text = message_text(message);
                if !text.is_empty() {
                    self.ui.conversation.append_notice(&text, false);
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
        let card = self.ensure_tool_card(&tool);
        card.status.set_text("Running");
        card.spinner.set_visible(true);
        card.spinner.start();
        self.ui
            .chat_status
            .activity(&format!("Using {}", card.title.text()));
        self.scroll_to_bottom();
    }

    fn tool_updated(&self, tool: ToolUpdate) {
        let existing = self.tool_cards.borrow().get(&tool.id).cloned();
        let card = existing.unwrap_or_else(|| {
            self.ensure_tool_card(&ToolStart {
                id: tool.id.clone(),
                name: tool.name.clone(),
                args: Value::Null,
                intent: None,
            })
        });
        card.update_partial(&tool.partial_result);
    }

    fn tool_ended(&self, tool: ToolEnd) {
        let existing = self.tool_cards.borrow().get(&tool.id).cloned();
        let card = existing.unwrap_or_else(|| {
            self.ensure_tool_card(&ToolStart {
                id: tool.id.clone(),
                name: tool.name.clone(),
                args: Value::Null,
                intent: None,
            })
        });
        card.complete(&tool.result, tool.is_error);
        if tool.is_error {
            self.alerts
                .play(alerts::sound_event_for_error(&tool.result.to_string()));
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

    fn ensure_tool_card(&self, tool: &ToolStart) -> ToolCard {
        if let Some(card) = self.tool_cards.borrow().get(&tool.id) {
            return card.clone();
        }
        let card = ToolCard::new(&tool.name, &tool.args, tool.intent.as_deref());
        self.ui.conversation.append(&card.root);
        self.tool_cards
            .borrow_mut()
            .insert(tool.id.clone(), card.clone());
        card
    }

    fn subagent_updated(self: &Rc<Self>, update: SubagentUpdate) {
        let key = update
            .id
            .clone()
            .or_else(|| update.agent.clone())
            .unwrap_or_else(|| "subagent".to_owned());
        let display_name = update
            .id
            .as_deref()
            .filter(|id| !looks_like_uuid(id))
            .or(update.agent.as_deref())
            .map(friendly_agent_name)
            .unwrap_or_else(|| "Subagent".to_owned());
        let status = update
            .status
            .as_deref()
            .unwrap_or(match update.kind {
                SubagentUpdateKind::Lifecycle => "active",
                SubagentUpdateKind::Progress => "working",
                SubagentUpdateKind::Event => "active",
            })
            .to_ascii_lowercase();
        let terminal = matches!(
            status.as_str(),
            "completed" | "complete" | "done" | "failed" | "aborted" | "removed"
        );
        let task = update
            .task
            .clone()
            .filter(|task| !task.trim().is_empty())
            .unwrap_or_else(|| "Working on delegated task".to_owned());
        self.subagents.borrow_mut().insert(
            key.clone(),
            SubagentState {
                id: key.clone(),
                display_name,
                status: title_case(&status),
                task,
                active: !terminal,
            },
        );
        self.refresh_subagent_chips();
        self.update_activity_counts();
        if self.active_subagent.borrow().as_deref() == Some(&key)
            && let Some(client) = &self.client
        {
            let _ = client.get_subagent_messages(&key);
        }
    }

    fn refresh_subagent_chips(self: &Rc<Self>) {
        self.ui.composer.clear_subagent_chips();
        self.subagent_buttons.borrow_mut().clear();
        let mut agents = self
            .subagents
            .borrow()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| {
            right
                .active
                .cmp(&left.active)
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        for agent in agents {
            let chip = composer::subagent_chip(&agent.display_name, &agent.status, agent.active);
            chip.set_tooltip_text(Some(&agent.task));
            let id = agent.id.clone();
            let weak = Rc::downgrade(self);
            chip.connect_clicked(move |_| {
                if let Some(controller) = weak.upgrade() {
                    controller.open_subagent_view(&id);
                }
            });
            self.ui.composer.append_subagent_chip(&chip);
            self.subagent_buttons.borrow_mut().insert(agent.id, chip);
        }
        self.ui
            .composer
            .set_subagents_visible(!self.subagents.borrow().is_empty());
    }

    fn open_subagent_view(&self, id: &str) {
        let Some(agent) = self.subagents.borrow().get(id).cloned() else {
            return;
        };
        self.active_subagent.replace(Some(id.to_owned()));
        self.ui.subagent_conversation.clear();
        self.ui.subagent_conversation.append_notice(
            &format!("Loading {}’s transcript…", agent.display_name),
            false,
        );
        self.ui
            .title
            .set_text(&format!("Agent · {}", agent.display_name));
        self.ui.back_button.set_visible(true);
        self.ui.composer_clamp.set_visible(false);
        self.ui.content_stack.set_visible_child_name("subagent");
        if let Some(client) = &self.client
            && let Err(error) = client.get_subagent_messages(id)
        {
            self.ui
                .subagent_conversation
                .append_notice(&error.to_string(), true);
        }
    }

    fn close_subagent_view(&self) {
        self.active_subagent.borrow_mut().take();
        self.ui.content_stack.set_visible_child_name("chat");
        self.ui.back_button.set_visible(false);
        self.ui.composer_clamp.set_visible(true);
        self.ui.title.set_text(&self.current_session_title.borrow());
    }

    fn hydrate_subagent_messages(&self, messages: &[Value]) {
        self.ui.subagent_conversation.clear();
        let mut cards = HashMap::<String, ToolCard>::new();
        for message in messages {
            match message_role(message) {
                Some("user") => {
                    let text = message_text(message);
                    if !text.is_empty() {
                        self.ui
                            .subagent_conversation
                            .append_message(MessageRole::User, &text);
                    }
                }
                Some("assistant") => {
                    let thinking = message_thinking(message);
                    if !thinking.is_empty() {
                        self.ui
                            .subagent_conversation
                            .append_thinking(&thinking, false);
                    }
                    let text = message_text(message);
                    if !text.is_empty() {
                        self.ui
                            .subagent_conversation
                            .append_message(MessageRole::Assistant, &text);
                    }
                    for tool in message_tool_calls(message) {
                        let card = ToolCard::new(&tool.name, &tool.args, tool.intent.as_deref());
                        self.ui.subagent_conversation.append(&card.root);
                        cards.insert(tool.id, card);
                    }
                }
                Some("toolResult") => {
                    if let Some((id, name, result, is_error)) = tool_result_parts(message) {
                        let card = cards.entry(id).or_insert_with(|| {
                            let card = ToolCard::new(&name, &Value::Null, None);
                            self.ui.subagent_conversation.append(&card.root);
                            card
                        });
                        card.complete(&result, is_error);
                    }
                }
                _ => {}
            }
        }
        if self.ui.subagent_conversation.is_empty() {
            self.ui.subagent_conversation.append_notice(
                "This subagent has not produced transcript messages yet.",
                false,
            );
        }
        self.ui.subagent_conversation.scroll_to_bottom();
    }

    fn clear_subagents(&self) {
        self.subagents.borrow_mut().clear();
        self.subagent_buttons.borrow_mut().clear();
        self.ui.composer.clear_subagent_chips();
        self.ui.composer.set_subagents_visible(false);
        self.update_activity_counts();
    }

    fn update_activity_counts(&self) {
        let active_agents = self
            .subagents
            .borrow()
            .values()
            .filter(|agent| agent.active)
            .count();
        let total_agents = self.subagents.borrow().len();
        self.ui
            .composer
            .set_subagent_count(&format!("{active_agents} active · {total_agents} total"));
        let active_items = active_agents + usize::from(self.running.get());
        self.ui.sidebar_activity_count.set_visible(active_items > 0);
        self.ui
            .sidebar_activity_count
            .set_text(&format!("{active_items} active"));
        for session in self.session_rows.borrow().iter() {
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
        if let Some(client) = &self.client {
            match client.abort() {
                Ok(()) => self.ui.chat_status.activity("Stopping"),
                Err(error) => self.show_error(&error.to_string()),
            }
        }
    }

    fn start_new_session(&self) {
        let Some(client) = &self.client else {
            return;
        };
        match client.new_session() {
            Ok(()) => {
                self.goal_completed_this_run.set(false);
                self.goal_completion_calls.borrow_mut().clear();
                self.ui.chat_status.activity("Starting conversation");
                self.clear_messages();
                self.clear_subagents();
                self.current_session_file.borrow_mut().take();
                self.set_session_title("New conversation");
                self.set_window_status(WindowStatus::Working);
                self.ui
                    .conversation
                    .append_notice("Starting a new conversation…", false);
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn submit_current(&self) {
        if !self.ready.get() || self.pending_submission.borrow().is_some() {
            return;
        }
        let draft = self.ui.composer.text();
        let message = draft.trim().to_owned();
        if message.is_empty() {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        let action = SubmissionAction::select(self.running.get(), self.running_turn_action.get());
        let request = match action {
            SubmissionAction::Prompt => client.prompt(&message),
            SubmissionAction::Steer => client.steer(&message),
            SubmissionAction::FollowUp => client.follow_up(&message),
        };
        match request {
            Ok(request_id) => {
                self.pending_submission.replace(Some(PendingSubmission {
                    request_id,
                    draft,
                    message,
                }));
                self.update_send_state();
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn request_steering_mode(&self, mode: QueueMode) {
        if self.reconciling_queue_state.get() || mode == self.steering_mode.get() {
            return;
        }
        let result = self
            .client
            .as_ref()
            .ok_or_else(|| "omp is disconnected".to_owned())
            .and_then(|client| client.set_steering_mode(mode).map_err(|error| error.to_string()));
        if let Err(error) = result {
            self.render_authoritative_queue_state();
            self.show_error(&error);
        }
    }

    fn request_follow_up_mode(&self, mode: QueueMode) {
        if self.reconciling_queue_state.get() || mode == self.follow_up_mode.get() {
            return;
        }
        let result = self
            .client
            .as_ref()
            .ok_or_else(|| "omp is disconnected".to_owned())
            .and_then(|client| client.set_follow_up_mode(mode).map_err(|error| error.to_string()));
        if let Err(error) = result {
            self.render_authoritative_queue_state();
            self.show_error(&error);
        }
    }

    fn request_interrupt_mode(&self, mode: InterruptMode) {
        if self.reconciling_queue_state.get() || mode == self.interrupt_mode.get() {
            return;
        }
        let result = self
            .client
            .as_ref()
            .ok_or_else(|| "omp is disconnected".to_owned())
            .and_then(|client| client.set_interrupt_mode(mode).map_err(|error| error.to_string()));
        if let Err(error) = result {
            self.render_authoritative_queue_state();
            self.show_error(&error);
        }
    }

    fn render_authoritative_queue_state(&self) {
        self.reconciling_queue_state.set(true);
        self.ui.composer.set_queue_state(
            self.steering_mode.get(),
            self.follow_up_mode.get(),
            self.interrupt_mode.get(),
            self.queued_message_count.get(),
        );
        self.reconciling_queue_state.set(false);
    }

    fn update_send_state(&self) {
        let ready = self.ready.get();
        let running = self.running.get();
        self.ui.composer.set_running_turn_action(matches!(
            self.running_turn_action.get(),
            RunningTurnAction::Steer
        ));
        self.ui.composer.set_primary_action(ready, running);
        self.ui
            .composer
            .set_submission_pending(self.pending_submission.borrow().is_some());
        self.ui
            .telemetry
            .cwd_button
            .set_sensitive(ready && !running);
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

    fn accept_completion(&self, submit_if_complete: bool) {
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
        if submit_if_complete && !completion.replacement.ends_with(' ') {
            self.hide_completions();
            self.submit_current();
        }
    }

    fn hide_completions(&self) {
        self.ui.composer.hide_completions();
    }

    fn clear_messages(&self) {
        self.ui.conversation.clear();
        self.streaming_message.borrow_mut().take();
        self.streaming_thinking.borrow_mut().take();
        self.tool_cards.borrow_mut().clear();
        self.pending_user_messages.borrow_mut().clear();
    }

    fn show_empty_state(&self) {
        self.ui.conversation.show_empty();
    }

    fn remove_empty_state(&self) {
        self.ui.conversation.hide_empty();
    }

    fn scroll_to_bottom(&self) {
        self.ui.conversation.scroll_to_bottom();
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
        self.ui.conversation.append_notice(message, true);
        self.scroll_to_bottom();
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

    fn present_alert_preferences(self: &Rc<Self>) {
        let preferences = self.alerts.preferences();
        let settings = sound_settings::SoundSettingsDialog::new(
            &preferences,
            &self.sound_pack_choices(),
            self.alerts.installed_pack_count(),
        );

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

    fn present_sound_pack_browser(self: &Rc<Self>, settings: &sound_settings::SoundSettingsDialog) {
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
        settings: sound_settings::SoundSettingsDialog,
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
        settings: sound_settings::SoundSettingsDialog,
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
                self.ui.conversation.append_notice(message, is_error);
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
            if let Some(client) = &controller.client {
                let _ = client.respond_to_extension(payload);
            }
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

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn looks_like_uuid(value: &str) -> bool {
    value.len() >= 32
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
}

fn friendly_agent_name(value: &str) -> String {
    let mut output = String::new();
    let mut previous_lowercase = false;
    for character in value.replace(['_', '-'], " ").chars() {
        if character.is_uppercase() && previous_lowercase {
            output.push(' ');
        }
        previous_lowercase = character.is_lowercase();
        output.push(character);
    }
    title_case(output.trim())
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

fn enter_inserts_newline(modifiers: gdk::ModifierType) -> bool {
    modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK)
}

#[cfg(test)]
mod tests {
    use super::{
        RunningTurnAction, SubmissionAction, enter_inserts_newline, gdk,
        reconcile_composer_state,
    };
    use crate::bridge::protocol::{InterruptMode, QueueMode, SessionState};
    use serde_json::json;

    #[test]
    fn ctrl_or_shift_enter_inserts_newline_while_plain_enter_submits() {
        assert!(enter_inserts_newline(gdk::ModifierType::CONTROL_MASK));
        assert!(enter_inserts_newline(gdk::ModifierType::SHIFT_MASK));
        assert!(!enter_inserts_newline(gdk::ModifierType::empty()));
    }

    #[test]
    fn submission_action_never_maps_running_text_to_abort() {
        assert_eq!(
            SubmissionAction::select(false, RunningTurnAction::FollowUp),
            SubmissionAction::Prompt
        );
        assert_eq!(
            SubmissionAction::select(true, RunningTurnAction::Steer),
            SubmissionAction::Steer
        );
        assert_eq!(
            SubmissionAction::select(true, RunningTurnAction::FollowUp),
            SubmissionAction::FollowUp
        );
    }

    #[test]
    fn state_refresh_reconciles_queue_settings_without_resetting_running_action() {
        let state: SessionState = serde_json::from_value(json!({
            "isStreaming": true,
            "steeringMode": "all",
            "followUpMode": "one-at-a-time",
            "interruptMode": "wait",
            "queuedMessageCount": 3
        }))
        .expect("deserialize session state");

        let reconciled = reconcile_composer_state(RunningTurnAction::FollowUp, &state);

        assert_eq!(
            reconciled.running_turn_action,
            RunningTurnAction::FollowUp
        );
        assert_eq!(reconciled.steering_mode, QueueMode::All);
        assert_eq!(reconciled.follow_up_mode, QueueMode::OneAtATime);
        assert_eq!(reconciled.interrupt_mode, InterruptMode::Wait);
        assert_eq!(reconciled.queued_message_count, 3);
    }
}
