mod chat;
mod composer;
mod icons;
mod model_picker;
mod sidebar;
mod tool_components;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use serde_json::{Value, json};

use self::chat::{
    ChatStatus, MessageRole, TelemetryWidgets, ThinkingBlock, append_message, append_notice,
    append_thinking, empty_chat_hero,
};
use self::sidebar::SessionRow;
use self::tool_components::ToolCard;
use crate::bridge::protocol::{
    ModelSummary, RpcEvent, RpcResponse, SessionState, SlashCommand, SubagentUpdate,
    SubagentUpdateKind, ToolEnd, ToolStart, ToolUpdate, message_cost, message_role, message_text,
    message_thinking, message_tool_calls, tool_result_parts,
};
use crate::bridge::{BridgeClient, OmpBridge};
use crate::commands::{CommandCompletion, completions};

pub fn build(app: &adw::Application) {
    icons::initialize_lucide_font().expect("failed to load bundled Lucide icon font");
    load_styles();
    let ui = build_shell(app);

    let (bridge, client, events) = match OmpBridge::spawn() {
        Ok(bridge) => {
            let client = bridge.client.clone();
            let events = bridge.events.clone();
            (Some(bridge), Some(client), Some(events))
        }
        Err(error) => (None, None, {
            ui.input.set_sensitive(false);
            append_notice(&ui.messages, &format!("Could not start omp: {error}"), true);
            None
        }),
    };

    let controller = Rc::new(AppController {
        ui,
        bridge: RefCell::new(bridge),
        client,
        models: RefCell::new(Vec::new()),
        commands: RefCell::new(Vec::new()),
        thinking_levels: RefCell::new(Vec::new()),
        thinking_buttons: RefCell::new(Vec::new()),
        completion_items: RefCell::new(Vec::new()),
        completion_index: Cell::new(0),
        pending_user_messages: RefCell::new(VecDeque::new()),
        streaming_message: RefCell::new(None),
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
    });

    controller.wire_interactions();
    if let Some(events) = events {
        AppController::run_event_loop(&controller, events);
    }
    let weak = Rc::downgrade(&controller);
    glib::timeout_add_local(Duration::from_millis(750), move || {
        let Some(controller) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        controller.refresh_titles_from_disk();
        glib::ControlFlow::Continue
    });
    controller.ui.window.present();
    eprintln!("omp native bridge UI ready");
}

struct Ui {
    window: adw::ApplicationWindow,
    title: gtk::Label,
    session_list: gtk::ListBox,
    sidebar_activity_count: gtk::Label,
    sidebar_root: gtk::Box,
    show_sidebar_button: gtk::Button,
    hide_sidebar_button: gtk::Button,
    history_button: gtk::Button,
    back_button: gtk::Button,
    content_stack: gtk::Stack,
    subagent_messages: gtk::Box,
    subagent_scroller: gtk::ScrolledWindow,
    composer_clamp: adw::Clamp,
    chat_status: ChatStatus,
    telemetry: TelemetryWidgets,
    messages: gtk::Box,
    empty_chat_hero: gtk::Box,
    message_scroller: gtk::ScrolledWindow,
    input: gtk::TextView,
    send_button: gtk::Button,
    new_chat_button: gtk::Button,
    model_button: gtk::Button,
    model_label: gtk::Label,
    model_icon: icons::ProviderIcon,
    thinking_button: gtk::Button,
    thinking_label: gtk::Label,
    thinking_popover: gtk::Popover,
    thinking_list: gtk::Box,
    completion_popover: gtk::Popover,
    completion_list: gtk::ListBox,
    extension_above: gtk::Box,
    extension_below: gtk::Box,
    extension_status: gtk::Label,
    subagent_bar: gtk::Box,
    subagent_count: gtk::Label,
    subagent_chips: gtk::Box,
}

struct StreamingMessage {
    label: gtk::Label,
    text: String,
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
    ui: Ui,
    bridge: RefCell<Option<OmpBridge>>,
    client: Option<BridgeClient>,
    models: RefCell<Vec<ModelSummary>>,
    commands: RefCell<Vec<SlashCommand>>,
    thinking_levels: RefCell<Vec<String>>,
    thinking_buttons: RefCell<Vec<gtk::Button>>,
    completion_items: RefCell<Vec<CommandCompletion>>,
    completion_index: Cell<usize>,
    pending_user_messages: RefCell<VecDeque<String>>,
    streaming_message: RefCell<Option<StreamingMessage>>,
    streaming_thinking: RefCell<Option<ThinkingBlock>>,
    tool_cards: RefCell<HashMap<String, ToolCard>>,
    subagents: RefCell<HashMap<String, SubagentState>>,
    subagent_buttons: RefCell<HashMap<String, gtk::Button>>,
    session_rows: RefCell<Vec<SessionRow>>,
    active_sessions: RefCell<Vec<sidebar::SessionEntry>>,
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
}

impl AppController {
    fn wire_interactions(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ui.input.buffer().connect_changed(move |_| {
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
                if controller.ui.completion_popover.is_visible() {
                    controller.accept_completion(true);
                } else {
                    controller.submit_current();
                }
                return glib::Propagation::Stop;
            }
            if !controller.ui.completion_popover.is_visible() {
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
        self.ui.input.add_controller(key_controller);

        let weak = Rc::downgrade(self);
        self.ui.send_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.activate_primary_action();
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
        self.ui.model_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.present_model_picker();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui.thinking_button.connect_clicked(move |_| {
            if let Some(controller) = weak.upgrade() {
                controller.ui.thinking_popover.popup();
            }
        });

        let weak = Rc::downgrade(self);
        self.ui
            .completion_list
            .connect_row_activated(move |_, row| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                controller.completion_index.set(row.index() as usize);
                controller.accept_completion(false);
            });

        let weak = Rc::downgrade(self);
        self.ui.window.connect_close_request(move |_| {
            if let Some(controller) = weak.upgrade()
                && let Some(bridge) = controller.bridge.borrow().as_ref()
            {
                bridge.shutdown();
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
                self.ui.chat_status.activity("Thinking");
                self.update_activity_counts();
                self.update_send_state();
            }
            RpcEvent::AgentEnd => {
                self.running.set(false);
                if let Some(thinking) = self.streaming_thinking.borrow_mut().take() {
                    thinking.finish(None);
                }
                self.streaming_message.borrow_mut().take();
                self.ui.chat_status.idle();
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
                    append_message(&self.ui.messages, MessageRole::Assistant, &text);
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
                append_notice(&self.ui.messages, &message, level == "error");
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
                self.ui.chat_status.disconnected();
                self.ui.input.set_sensitive(false);
                self.update_send_state();
                self.show_error(&message);
            }
            RpcEvent::Other => {}
        }
    }

    fn handle_response(self: &Rc<Self>, response: RpcResponse) {
        if !response.success {
            if response.command == "prompt" {
                self.pending_user_messages.borrow_mut().pop_front();
            }
            if response.command == "new_session" {
                self.pending_delete.borrow_mut().take();
            }
            self.show_error(
                response
                    .error
                    .as_deref()
                    .unwrap_or("omp rejected the request"),
            );
            return;
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

    fn refresh_after_new_session(self: &Rc<Self>) {
        if let Some(path) = self.pending_delete.borrow_mut().take() {
            if let Err(error) = sidebar::delete_session_files(&path) {
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
        self.clear_messages();
        self.clear_subagents();
        self.set_session_title("New conversation");
        append_notice(&self.ui.messages, "Starting a new conversation…", false);
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
        self.ui.input.set_sensitive(true);

        let session_file = state.session_file.as_deref().map(PathBuf::from);
        let disk_title = session_file
            .as_deref()
            .and_then(sidebar::read_session_title);
        self.current_session_file.replace(session_file.clone());
        let resolved =
            sidebar::authoritative_title(state.session_name.as_deref(), disk_title.as_deref());
        let title = sidebar::authoritative_title(
            Some(&resolved),
            Some(&self.current_session_title.borrow()),
        );
        self.set_session_title(&title);
        let current_entry = sidebar::session_entry(session_file.as_deref(), &title, true);
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
            icons::set_provider_icon(&self.ui.model_icon, &model.provider);
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
        } else {
            self.ui.chat_status.idle();
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
                sidebar::session_entry(
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
        let current = sidebar::session_entry(
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

    fn render_session_sidebar(self: &Rc<Self>, entries: Vec<sidebar::SessionEntry>) {
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

    fn present_history(self: &Rc<Self>) {
        let sessions =
            sidebar::discover_all_sessions(self.current_session_file.borrow().as_deref());
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

    fn open_session(self: &Rc<Self>, entry: &sidebar::SessionEntry) {
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
                append_notice(&self.ui.messages, "Loading conversation…", false);
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn close_session(self: &Rc<Self>, entry: &sidebar::SessionEntry) {
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

    fn present_delete_dialog(self: &Rc<Self>, entry: &sidebar::SessionEntry) {
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
        match sidebar::delete_session_files(path) {
            Ok(()) => {
                self.active_sessions
                    .borrow_mut()
                    .retain(|entry| entry.path.as_deref() != Some(path));
                self.render_session_sidebar(self.active_sessions.borrow().clone());
            }
            Err(error) => self.show_error(&format!("Could not delete conversation: {error}")),
        }
    }

    fn present_rename_dialog(self: &Rc<Self>, entry: &sidebar::SessionEntry) {
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
        let models = self.models.borrow().clone();
        if models.is_empty() {
            return;
        }
        let selected = self.current_model.borrow().clone();
        let weak = Rc::downgrade(self);
        model_picker::present(&self.ui.window, models, selected, move |model| {
            if let Some(controller) = weak.upgrade() {
                controller.choose_model(model);
            }
        });
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
            .model_button
            .set_sensitive(!self.models.borrow().is_empty());
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
        icons::set_provider_icon(&self.ui.model_icon, &model.provider);
        self.ui.model_label.set_text(model.display_name());
        self.ui.model_button.set_tooltip_text(Some(&format!(
            "{} · {}",
            model.display_name(),
            icons::provider_label(&model.provider)
        )));
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
        while let Some(child) = self.ui.thinking_list.first_child() {
            self.ui.thinking_list.remove(&child);
        }
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
                        controller.ui.thinking_popover.popdown();
                    }
                    Err(error) => controller.show_error(&error.to_string()),
                }
            });
            self.ui.thinking_list.append(&button);
            self.thinking_buttons.borrow_mut().push(button);
        }
        self.ui.thinking_button.set_sensitive(!efforts.is_empty());
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
        self.ui.thinking_label.set_text(&title_case(level));
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
                        append_message(&self.ui.messages, MessageRole::User, &text);
                    }
                }
                Some("assistant") => {
                    let thinking = message_thinking(message);
                    if !thinking.is_empty() {
                        append_thinking(&self.ui.messages, &thinking, false);
                    }
                    let text = message_text(message);
                    if !text.is_empty() {
                        append_message(&self.ui.messages, MessageRole::Assistant, &text);
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
                        append_notice(&self.ui.messages, &text, false);
                    }
                }
                _ => {}
            }
        }
        self.session_cost.set(cost);
        self.ui.telemetry.set_cost(cost);
        if self.ui.messages.first_child().is_none() {
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
                    append_message(&self.ui.messages, MessageRole::User, &text);
                    self.scroll_to_bottom();
                }
            }
            Some("assistant") => {
                let thinking = message_thinking(message);
                self.streaming_thinking.borrow_mut().take();
                if !thinking.is_empty() {
                    self.streaming_thinking.replace(Some(append_thinking(
                        &self.ui.messages,
                        &thinking,
                        true,
                    )));
                }
                let text = message_text(message);
                self.streaming_message.borrow_mut().take();
                if !text.is_empty() {
                    let label = append_message(&self.ui.messages, MessageRole::Assistant, &text);
                    self.streaming_message
                        .replace(Some(StreamingMessage { label, text }));
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
            let label = append_message(&self.ui.messages, MessageRole::Assistant, "");
            *slot = Some(StreamingMessage {
                label,
                text: String::new(),
            });
        }
        let streaming = slot.as_mut().expect("streaming message exists");
        streaming.text.push_str(delta);
        streaming.label.set_text(&streaming.text);
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
            *slot = Some(append_thinking(&self.ui.messages, "", true));
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
                    append_thinking(&self.ui.messages, &final_thinking, false);
                }
                let final_text = message_text(message);
                if let Some(mut streaming) = self.streaming_message.borrow_mut().take() {
                    if !final_text.is_empty() && final_text != streaming.text {
                        streaming.text = final_text;
                        streaming.label.set_text(&streaming.text);
                    }
                } else if !final_text.is_empty() {
                    append_message(&self.ui.messages, MessageRole::Assistant, &final_text);
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
                    append_notice(&self.ui.messages, &text, false);
                }
            }
            _ => {}
        }
        self.scroll_to_bottom();
    }

    fn tool_started(&self, tool: ToolStart) {
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
        self.ui.messages.append(&card.root);
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
        while let Some(child) = self.ui.subagent_chips.first_child() {
            self.ui.subagent_chips.remove(&child);
        }
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
            self.ui.subagent_chips.append(&chip);
            self.subagent_buttons.borrow_mut().insert(agent.id, chip);
        }
        self.ui
            .subagent_bar
            .set_visible(!self.subagents.borrow().is_empty());
    }

    fn open_subagent_view(&self, id: &str) {
        let Some(agent) = self.subagents.borrow().get(id).cloned() else {
            return;
        };
        self.active_subagent.replace(Some(id.to_owned()));
        while let Some(child) = self.ui.subagent_messages.first_child() {
            self.ui.subagent_messages.remove(&child);
        }
        append_notice(
            &self.ui.subagent_messages,
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
            append_notice(&self.ui.subagent_messages, &error.to_string(), true);
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
        while let Some(child) = self.ui.subagent_messages.first_child() {
            self.ui.subagent_messages.remove(&child);
        }
        let mut cards = HashMap::<String, ToolCard>::new();
        for message in messages {
            match message_role(message) {
                Some("user") => {
                    let text = message_text(message);
                    if !text.is_empty() {
                        append_message(&self.ui.subagent_messages, MessageRole::User, &text);
                    }
                }
                Some("assistant") => {
                    let thinking = message_thinking(message);
                    if !thinking.is_empty() {
                        append_thinking(&self.ui.subagent_messages, &thinking, false);
                    }
                    let text = message_text(message);
                    if !text.is_empty() {
                        append_message(&self.ui.subagent_messages, MessageRole::Assistant, &text);
                    }
                    for tool in message_tool_calls(message) {
                        let card = ToolCard::new(&tool.name, &tool.args, tool.intent.as_deref());
                        self.ui.subagent_messages.append(&card.root);
                        cards.insert(tool.id, card);
                    }
                }
                Some("toolResult") => {
                    if let Some((id, name, result, is_error)) = tool_result_parts(message) {
                        let card = cards.entry(id).or_insert_with(|| {
                            let card = ToolCard::new(&name, &Value::Null, None);
                            self.ui.subagent_messages.append(&card.root);
                            card
                        });
                        card.complete(&result, is_error);
                    }
                }
                _ => {}
            }
        }
        if self.ui.subagent_messages.first_child().is_none() {
            append_notice(
                &self.ui.subagent_messages,
                "This subagent has not produced transcript messages yet.",
                false,
            );
        }
        let adjustment = self.ui.subagent_scroller.vadjustment();
        glib::idle_add_local_once(move || {
            adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(0.0));
        });
    }

    fn clear_subagents(&self) {
        self.subagents.borrow_mut().clear();
        self.subagent_buttons.borrow_mut().clear();
        while let Some(child) = self.ui.subagent_chips.first_child() {
            self.ui.subagent_chips.remove(&child);
        }
        self.ui.subagent_bar.set_visible(false);
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
            .subagent_count
            .set_text(&format!("{active_agents} active · {total_agents} total"));
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
    }

    fn activate_primary_action(&self) {
        if self.running.get() {
            if let Some(client) = &self.client {
                match client.abort() {
                    Ok(()) => self.ui.chat_status.activity("Stopping"),
                    Err(error) => self.show_error(&error.to_string()),
                }
            }
        } else {
            self.submit_current();
        }
    }

    fn start_new_session(&self) {
        let Some(client) = &self.client else {
            return;
        };
        match client.new_session() {
            Ok(()) => {
                self.ui.chat_status.activity("Starting conversation");
                self.clear_messages();
                self.clear_subagents();
                self.current_session_file.borrow_mut().take();
                self.set_session_title("New conversation");
                append_notice(&self.ui.messages, "Starting a new conversation…", false);
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn submit_current(&self) {
        if !self.ready.get() {
            return;
        }
        let buffer = self.ui.input.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_owned();
        if text.is_empty() {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        match client.prompt(&text) {
            Ok(()) => {
                self.remove_empty_state();
                append_message(&self.ui.messages, MessageRole::User, &text);
                self.pending_user_messages.borrow_mut().push_back(text);
                buffer.set_text("");
                self.hide_completions();
                self.scroll_to_bottom();
            }
            Err(error) => self.show_error(&error.to_string()),
        }
    }

    fn update_send_state(&self) {
        let buffer = self.ui.input.buffer();
        let has_text = !buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .is_empty();
        let running = self.running.get();
        icons::set_button_icon(
            &self.ui.send_button,
            if running {
                icons::Icon::Square
            } else {
                icons::Icon::SendHorizontal
            },
        );
        self.ui.send_button.set_tooltip_text(Some(if running {
            "Stop response"
        } else {
            "Send · Enter"
        }));
        self.ui
            .send_button
            .set_sensitive(self.ready.get() && (running || has_text));
    }

    fn update_completions(&self) {
        let buffer = self.ui.input.buffer();
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        let candidates = completions(text.as_str(), &self.commands.borrow());
        self.completion_items.replace(candidates);
        self.completion_index.set(0);
        while let Some(child) = self.ui.completion_list.first_child() {
            self.ui.completion_list.remove(&child);
        }
        for completion in self.completion_items.borrow().iter() {
            self.ui.completion_list.append(&completion_row(completion));
        }
        if let Some(first) = self.ui.completion_list.row_at_index(0) {
            self.ui.completion_list.select_row(Some(&first));
            if let Some(parent) = self.ui.completion_popover.parent() {
                self.ui.completion_popover.set_width_request(parent.width());
            }
            self.ui.completion_popover.popup();
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
        if let Some(row) = self.ui.completion_list.row_at_index(index as i32) {
            self.ui.completion_list.select_row(Some(&row));
        }
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
        self.ui.input.buffer().set_text(&completion.replacement);
        self.ui.input.grab_focus();
        if submit_if_complete && !completion.replacement.ends_with(' ') {
            self.hide_completions();
            self.submit_current();
        }
    }

    fn hide_completions(&self) {
        self.ui.completion_popover.popdown();
    }

    fn clear_messages(&self) {
        self.ui.empty_chat_hero.set_visible(false);
        while let Some(child) = self.ui.messages.first_child() {
            self.ui.messages.remove(&child);
        }
        self.streaming_message.borrow_mut().take();
        self.streaming_thinking.borrow_mut().take();
        self.tool_cards.borrow_mut().clear();
        self.pending_user_messages.borrow_mut().clear();
    }

    fn show_empty_state(&self) {
        self.ui.empty_chat_hero.set_visible(true);
    }

    fn remove_empty_state(&self) {
        self.ui.empty_chat_hero.set_visible(false);
    }

    fn scroll_to_bottom(&self) {
        let adjustment = self.ui.message_scroller.vadjustment();
        let adjustment_after_layout = adjustment.clone();
        glib::idle_add_local_once(move || {
            adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(0.0));
        });
        glib::timeout_add_local_once(Duration::from_millis(50), move || {
            adjustment_after_layout.set_value(
                (adjustment_after_layout.upper() - adjustment_after_layout.page_size()).max(0.0),
            );
        });
    }

    fn show_error(&self, message: &str) {
        append_notice(&self.ui.messages, message, true);
        self.scroll_to_bottom();
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
                append_notice(&self.ui.messages, message, is_error);
            }
            "setTitle" => {
                if let Some(title) = request.get("title").and_then(Value::as_str) {
                    self.ui.title.set_text(title);
                }
            }
            "set_editor_text" => {
                if let Some(text) = request.get("text").and_then(Value::as_str) {
                    self.ui.input.buffer().set_text(text);
                    self.ui.input.grab_focus();
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
        self.ui.extension_status.set_text(&text);
        self.ui.extension_status.set_visible(!text.is_empty());
    }

    fn update_extension_widget(&self, request: &Value) {
        let Some(key) = request.get("widgetKey").and_then(Value::as_str) else {
            return;
        };
        if let Some(label) = self.extension_widgets.borrow_mut().remove(key)
            && let Some(parent) = label.parent().and_downcast::<gtk::Box>()
        {
            parent.remove(&label);
            parent.set_visible(parent.first_child().is_some());
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
        let container =
            if request.get("widgetPlacement").and_then(Value::as_str) == Some("belowEditor") {
                &self.ui.extension_below
            } else {
                &self.ui.extension_above
            };
        container.append(&label);
        container.set_visible(true);
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

fn build_shell(app: &adw::Application) -> Ui {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    let sidebar = sidebar::build();

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("workspace");
    let header_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    header_box.add_css_class("header-box");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("chat-header");
    let show_sidebar_button =
        icons::icon_button(icons::Icon::PanelLeftOpen, "Show recent conversations");
    show_sidebar_button.set_visible(false);
    show_sidebar_button.add_css_class("sidebar-toggle");
    let back_button = icons::icon_button(icons::Icon::ArrowLeft, "Back to main conversation");
    back_button.set_visible(false);
    back_button.set_tooltip_text(Some("Back to main conversation"));
    back_button.add_css_class("back-button");
    let assistant_mark = icons::omp_logo(19);
    assistant_mark.add_css_class("header-logo");
    let title = gtk::Label::new(Some("New conversation"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("chat-title");
    let chat_status = ChatStatus::new();
    header.append(&show_sidebar_button);
    header.append(&back_button);
    header.append(&assistant_mark);
    header.append(&title);
    header.append(&chat_status.root);
    header.append(&window_controls());

    let telemetry = TelemetryWidgets::new("No workspace");
    header_box.append(&header);
    header_box.append(&telemetry.root);
    let header_handle = gtk::WindowHandle::new();
    header_handle.set_child(Some(&header_box));

    let messages = gtk::Box::new(gtk::Orientation::Vertical, 24);
    messages.set_margin_top(32);
    messages.set_margin_bottom(28);
    messages.set_margin_start(22);
    messages.set_margin_end(22);
    append_notice(&messages, "Connecting to the omp runtime…", false);
    let message_clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(720)
        .child(&messages)
        .build();
    let message_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&message_clamp)
        .build();
    message_scroller.add_css_class("message-scroll");
    let empty_chat_hero = empty_chat_hero();
    empty_chat_hero.set_can_target(false);
    empty_chat_hero.set_visible(false);
    let message_overlay = gtk::Overlay::new();
    message_overlay.set_child(Some(&message_scroller));
    message_overlay.add_overlay(&empty_chat_hero);

    let subagent_messages = gtk::Box::new(gtk::Orientation::Vertical, 20);
    subagent_messages.set_margin_top(28);
    subagent_messages.set_margin_bottom(28);
    subagent_messages.set_margin_start(22);
    subagent_messages.set_margin_end(22);
    let subagent_clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(720)
        .child(&subagent_messages)
        .build();
    let subagent_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&subagent_clamp)
        .build();
    subagent_scroller.add_css_class("message-scroll");
    let content_stack = gtk::Stack::new();
    content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    content_stack.set_transition_duration(180);
    content_stack.add_named(&message_overlay, Some("chat"));
    content_stack.add_named(&subagent_scroller, Some("subagent"));
    content_stack.set_visible_child_name("chat");

    let composer = composer::build();
    let composer_clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(720)
        .child(&composer.root)
        .build();
    composer_clamp.set_margin_start(24);
    composer_clamp.set_margin_end(24);
    composer_clamp.set_margin_bottom(18);

    root.append(&header_handle);
    root.append(&content_stack);
    root.append(&composer_clamp);
    let split = gtk::Paned::new(gtk::Orientation::Horizontal);
    split.set_start_child(Some(&sidebar.root));
    split.set_end_child(Some(&root));
    split.set_position(286);
    split.set_resize_start_child(false);
    split.set_shrink_start_child(false);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("omp native")
        .default_width(1260)
        .default_height(800)
        .content(&split)
        .build();
    window.set_decorated(false);

    Ui {
        window,
        title,
        session_list: sidebar.list,
        sidebar_activity_count: sidebar.active_count,
        sidebar_root: sidebar.root,
        show_sidebar_button,
        hide_sidebar_button: sidebar.collapse,
        history_button: sidebar.history,
        back_button,
        content_stack,
        subagent_messages,
        subagent_scroller,
        composer_clamp,
        chat_status,
        telemetry,
        messages,
        empty_chat_hero,
        message_scroller,
        input: composer.input,
        send_button: composer.send,
        new_chat_button: sidebar.new_chat,
        model_button: composer.model_button,
        model_label: composer.model_label,
        model_icon: composer.model_icon,
        thinking_button: composer.thinking_button,
        thinking_label: composer.thinking_label,
        thinking_popover: composer.thinking_popover,
        thinking_list: composer.thinking_list,
        completion_popover: composer.completion,
        completion_list: composer.completion_list,
        extension_above: composer.extension_above,
        extension_below: composer.extension_below,
        extension_status: composer.extension_status,
        subagent_bar: composer.subagent_bar,
        subagent_count: composer.subagent_count,
        subagent_chips: composer.subagent_chips,
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

fn window_controls() -> gtk::Box {
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("window-controls");
    for (icon, action, tooltip) in [
        (icons::Icon::Minus, "window.minimize", "Minimize"),
        (
            icons::Icon::Maximize2,
            "window.toggle-maximized",
            "Maximize",
        ),
        (icons::Icon::XCircle, "window.close", "Close"),
    ] {
        let button = icons::icon_button(icon, tooltip);
        button.set_action_name(Some(action));
        controls.append(&button);
    }
    controls
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

fn load_styles() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    let display = gdk::Display::default().expect("a graphical display");
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::{enter_inserts_newline, gdk};

    #[test]
    fn ctrl_or_shift_enter_inserts_newline_while_plain_enter_submits() {
        assert!(enter_inserts_newline(gdk::ModifierType::CONTROL_MASK));
        assert!(enter_inserts_newline(gdk::ModifierType::SHIFT_MASK));
        assert!(!enter_inserts_newline(gdk::ModifierType::empty()));
    }
}
