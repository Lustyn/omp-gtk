use gtk::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::cell::RefCell;

use super::agent_hub::AgentHubView;
use super::chat::{ChatStatus, TelemetryWidgets};
use super::conversation::ConversationView;
use super::{agent_hub, composer, icons, sidebar, todos};

pub(crate) struct WorkspaceView {
    pub(crate) window: gtk::ApplicationWindow,
    pub(crate) title: gtk::Label,
    pub(crate) session_list: gtk::ListBox,
    pub(crate) sidebar_activity_count: gtk::Label,
    pub(crate) sidebar_unread_count: gtk::Label,
    pub(crate) sidebar_root: gtk::Box,
    pub(crate) show_sidebar_button: gtk::Button,
    pub(crate) hide_sidebar_button: gtk::Button,
    pub(crate) history_button: gtk::Button,
    pub(crate) preferences_button: gtk::Button,
    pub(crate) agent_hub_button: gtk::Button,
    agent_hub_button_badge: gtk::Label,
    pub(crate) back_button: gtk::Button,
    pub(crate) content_stack: gtk::Stack,
    pub(crate) agent_hub: AgentHubView,
    pub(crate) subagent_conversation: ConversationView,
    pub(crate) composer_clamp: adw::Clamp,
    pub(crate) chat_status: ChatStatus,
    pub(crate) telemetry: TelemetryWidgets,
    conversation_stack: gtk::Stack,
    conversation: RefCell<ConversationView>,
    pub(crate) todos: todos::TodoPanel,
    pub(crate) composer: composer::ComposerView,
    pub(crate) new_chat_button: gtk::Button,
}

pub(crate) fn build(app: &adw::Application, initial_runtime_id: u64) -> WorkspaceView {
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
    sidebar
        .root
        .bind_property("visible", &assistant_mark, "visible")
        .flags(gtk::glib::BindingFlags::SYNC_CREATE | gtk::glib::BindingFlags::INVERT_BOOLEAN)
        .build();
    let title = gtk::Label::new(Some("New conversation"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("chat-title");
    let chat_status = ChatStatus::new();
    let agent_hub_button = gtk::Button::new();
    agent_hub_button.set_visible(false);
    agent_hub_button.set_tooltip_text(Some("Open Agent Hub"));
    agent_hub_button.update_property(&[gtk::accessible::Property::Label("Open Agent Hub")]);
    agent_hub_button.add_css_class("agent-hub-button");
    let agent_hub_button_content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    agent_hub_button_content.append(&icons::icon(icons::Icon::Users, 15));
    agent_hub_button_content.append(&gtk::Label::new(Some("Agents")));
    let agent_hub_button_badge = gtk::Label::new(None);
    agent_hub_button_badge.set_accessible_role(gtk::AccessibleRole::Presentation);
    agent_hub_button_badge.add_css_class("agent-hub-button-badge");
    agent_hub_button_content.append(&agent_hub_button_badge);
    agent_hub_button.set_child(Some(&agent_hub_button_content));
    header.append(&show_sidebar_button);
    header.append(&back_button);
    header.append(&assistant_mark);
    header.append(&title);
    header.append(&agent_hub_button);
    header.append(&chat_status.root);
    header.append(&window_controls());

    let telemetry = TelemetryWidgets::new("No workspace");
    header_box.append(&header);
    header_box.append(&telemetry.root);
    let header_handle = gtk::WindowHandle::new();
    header_handle.set_child(Some(&header_box));

    let conversation = ConversationView::main();
    conversation.show_loading(
        "Connecting to omp",
        "Starting the local runtime and restoring your workspace.",
        "Establishing a secure local connection",
    );
    let agent_hub = agent_hub::build();
    let subagent_conversation = agent_hub.transcript.clone();
    let conversation_stack = gtk::Stack::new();
    conversation_stack.add_named(
        conversation.widget(),
        Some(&format!("runtime-{initial_runtime_id}")),
    );
    let content_stack = gtk::Stack::new();
    content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    content_stack.set_transition_duration(180);
    content_stack.add_named(&conversation_stack, Some("chat"));
    content_stack.add_named(agent_hub.widget(), Some("agent-hub"));
    content_stack.set_visible_child_name("chat");

    let todos = todos::TodoPanel::new();
    let conversation_overlay = gtk::Overlay::new();
    conversation_overlay.set_vexpand(true);
    conversation_overlay.set_child(Some(&content_stack));
    conversation_overlay.add_overlay(&todos.root);
    let composer = composer::build();
    let composer_clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(720)
        .child(composer.widget())
        .build();
    composer_clamp.set_margin_start(24);
    composer_clamp.set_margin_end(24);
    composer_clamp.set_margin_bottom(18);

    root.append(&header_handle);
    root.append(&conversation_overlay);
    root.append(&composer_clamp);
    let split = gtk::Paned::new(gtk::Orientation::Horizontal);
    split.set_start_child(Some(&sidebar.root));
    split.set_end_child(Some(&root));
    split.set_position(286);
    split.set_resize_start_child(false);
    split.set_shrink_start_child(false);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("omp native")
        .default_width(1260)
        .default_height(800)
        .child(&split)
        .build();
    window.add_css_class("app-window");
    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    titlebar.set_size_request(-1, 0);
    window.set_titlebar(Some(&titlebar));

    WorkspaceView {
        window,
        title,
        session_list: sidebar.list,
        sidebar_activity_count: sidebar.active_count,
        sidebar_unread_count: sidebar.unread_count,
        sidebar_root: sidebar.root,
        show_sidebar_button,
        hide_sidebar_button: sidebar.collapse,
        history_button: sidebar.history,
        preferences_button: sidebar.preferences,
        agent_hub_button,
        agent_hub_button_badge,
        back_button,
        content_stack,
        agent_hub,
        subagent_conversation,
        composer_clamp,
        chat_status,
        telemetry,
        conversation_stack,
        conversation: RefCell::new(conversation),
        todos,
        composer,
        new_chat_button: sidebar.new_chat,
    }
}

impl WorkspaceView {
    pub(crate) fn conversation(&self) -> ConversationView {
        self.conversation.borrow().clone()
    }

    pub(crate) fn show_session_conversation(
        &self,
        runtime_id: u64,
        conversation: &ConversationView,
    ) {
        let name = format!("runtime-{runtime_id}");
        if self.conversation_stack.child_by_name(&name).is_none() {
            self.conversation_stack
                .add_named(conversation.widget(), Some(&name));
        }
        self.conversation_stack.set_visible_child_name(&name);
        self.conversation.replace(conversation.clone());
    }

    pub(crate) fn remove_session_conversation(&self, runtime_id: u64) {
        let name = format!("runtime-{runtime_id}");
        if let Some(conversation) = self.conversation_stack.child_by_name(&name) {
            self.conversation_stack.remove(&conversation);
        }
    }

    pub(crate) fn set_agent_hub_activity(&self, active: usize, total: usize) {
        self.agent_hub_button.set_visible(total > 0);
        if total == 0 {
            self.agent_hub_button.remove_css_class("active");
            return;
        }

        let agent_word = if total == 1 { "agent" } else { "agents" };
        if active > 0 {
            self.agent_hub_button.add_css_class("active");
            self.agent_hub_button_badge
                .set_text(&format!("{active} active"));
            self.agent_hub_button.set_tooltip_text(Some(&format!(
                "Open Agent Hub — {active} active of {total} {agent_word}"
            )));
            self.agent_hub_button
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "Open Agent Hub, {active} active, {total} total"
                ))]);
        } else {
            self.agent_hub_button.remove_css_class("active");
            self.agent_hub_button_badge.set_text(&format!(
                "{total} {}",
                if total == 1 { "record" } else { "records" }
            ));
            self.agent_hub_button.set_tooltip_text(Some(&format!(
                "Open Agent Hub — {total} {agent_word} available to inspect"
            )));
            self.agent_hub_button
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "Open Agent Hub, {total} {agent_word} available, none active"
                ))]);
        }
    }
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
