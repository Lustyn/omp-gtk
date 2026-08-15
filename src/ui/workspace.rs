use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use super::agent_hub::AgentHubView;
use super::chat::{ChatStatus, TelemetryWidgets};
use super::conversation::ConversationView;
use super::{agent_hub, composer, icons, sidebar, todos};

pub(crate) struct WorkspaceView {
    pub(crate) window: gtk::ApplicationWindow,
    pub(crate) title: gtk::Label,
    pub(crate) session_list: gtk::ListBox,
    pub(crate) sidebar_activity_count: gtk::Label,
    pub(crate) sidebar_root: gtk::Box,
    pub(crate) show_sidebar_button: gtk::Button,
    pub(crate) hide_sidebar_button: gtk::Button,
    pub(crate) history_button: gtk::Button,
    pub(crate) preferences_button: gtk::Button,
    pub(crate) agent_hub_button: gtk::Button,
    pub(crate) back_button: gtk::Button,
    pub(crate) content_stack: gtk::Stack,
    pub(crate) agent_hub: AgentHubView,
    pub(crate) subagent_conversation: ConversationView,
    pub(crate) composer_clamp: adw::Clamp,
    pub(crate) chat_status: ChatStatus,
    pub(crate) telemetry: TelemetryWidgets,
    pub(crate) conversation: ConversationView,
    pub(crate) todos: todos::TodoPanel,
    pub(crate) composer: composer::ComposerView,
    pub(crate) new_chat_button: gtk::Button,
}

pub(crate) fn build(app: &adw::Application) -> WorkspaceView {
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
    let agent_hub_button = icons::labeled_button(icons::Icon::Users, "Agents");
    agent_hub_button.set_tooltip_text(Some("Open runtime agent hub"));
    agent_hub_button.update_property(&[gtk::accessible::Property::Label(
        "Open runtime agent hub, 0 active agents",
    )]);
    agent_hub_button.add_css_class("agent-hub-button");
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
    conversation.append_notice("Connecting to the omp runtime…", false);
    let agent_hub = agent_hub::build();
    let subagent_conversation = agent_hub.transcript.clone();
    let content_stack = gtk::Stack::new();
    content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    content_stack.set_transition_duration(180);
    content_stack.add_named(conversation.widget(), Some("chat"));
    content_stack.add_named(agent_hub.widget(), Some("agent-hub"));
    content_stack.set_visible_child_name("chat");

    let todos = todos::TodoPanel::new();
    let todos_clamp = adw::Clamp::builder()
        .maximum_size(900)
        .tightening_threshold(720)
        .child(&todos.root)
        .build();
    todos_clamp.set_margin_start(24);
    todos_clamp.set_margin_end(24);
    todos_clamp.set_margin_bottom(10);

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
    root.append(&content_stack);
    root.append(&todos_clamp);
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
    window.set_decorated(false);

    WorkspaceView {
        window,
        title,
        session_list: sidebar.list,
        sidebar_activity_count: sidebar.active_count,
        sidebar_root: sidebar.root,
        show_sidebar_button,
        hide_sidebar_button: sidebar.collapse,
        history_button: sidebar.history,
        preferences_button: sidebar.preferences,
        agent_hub_button,
        back_button,
        content_stack,
        agent_hub,
        subagent_conversation,
        composer_clamp,
        chat_status,
        telemetry,
        conversation,
        todos,
        composer,
        new_chat_button: sidebar.new_chat,
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
