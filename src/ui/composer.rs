use gtk::prelude::*;
use gtk4 as gtk;

use super::icons;

#[derive(Clone)]
pub struct ComposerWidgets {
    pub root: gtk::Box,
    pub input: gtk::TextView,
    pub send: gtk::Button,
    pub model_button: gtk::Button,
    pub model_label: gtk::Label,
    pub model_icon: icons::ProviderIcon,
    pub thinking_button: gtk::Button,
    pub thinking_label: gtk::Label,
    pub thinking_popover: gtk::Popover,
    pub thinking_list: gtk::Box,
    pub completion: gtk::Popover,
    pub completion_list: gtk::ListBox,
    pub extension_above: gtk::Box,
    pub extension_below: gtk::Box,
    pub extension_status: gtk::Label,
    pub subagent_bar: gtk::Box,
    pub subagent_count: gtk::Label,
    pub subagent_chips: gtk::Box,
}

pub fn build() -> ComposerWidgets {
    let composer = gtk::Box::new(gtk::Orientation::Vertical, 4);
    composer.add_css_class("composer");

    let subagent_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    subagent_bar.set_visible(false);
    subagent_bar.set_margin_top(7);
    subagent_bar.set_margin_start(10);
    subagent_bar.set_margin_end(10);
    subagent_bar.add_css_class("subagent-bar");
    let agents_icon = icons::icon(icons::Icon::Users, 15);
    let subagent_count = gtk::Label::new(Some("Agents"));
    subagent_count.add_css_class("subagent-count");
    let subagent_chips = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    subagent_chips.set_hexpand(true);
    subagent_bar.append(&agents_icon);
    subagent_bar.append(&subagent_count);
    subagent_bar.append(&subagent_chips);

    let input = gtk::TextView::new();
    input.set_wrap_mode(gtk::WrapMode::WordChar);
    input.set_top_margin(12);
    input.set_bottom_margin(8);
    input.set_left_margin(14);
    input.set_right_margin(14);
    input.set_height_request(78);
    input.set_sensitive(false);
    input.set_accepts_tab(false);
    input.add_css_class("composer-input");

    let input_overlay = gtk::Overlay::new();
    input_overlay.set_child(Some(&input));
    let placeholder = gtk::Label::new(Some(
        "Describe what you want omp to accomplish, or type / for commands…",
    ));
    placeholder.set_halign(gtk::Align::Start);
    placeholder.set_valign(gtk::Align::Start);
    placeholder.set_margin_top(12);
    placeholder.set_margin_start(14);
    placeholder.set_can_target(false);
    placeholder.add_css_class("composer-placeholder");
    input_overlay.add_overlay(&placeholder);
    let placeholder_for_change = placeholder.clone();
    input.buffer().connect_changed(move |buffer| {
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        placeholder_for_change.set_visible(text.is_empty());
    });

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    controls.set_margin_top(2);
    controls.set_margin_bottom(8);
    controls.set_margin_start(8);
    controls.set_margin_end(8);

    let model_icon = icons::provider_icon("", 15);
    model_icon.root.add_css_class("provider-icon");
    let model_label = gtk::Label::new(Some("Loading models…"));
    model_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    model_label.set_max_width_chars(26);
    let model_content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    model_content.append(&model_icon.root);
    model_content.append(&model_label);
    model_content.append(&icons::icon(icons::Icon::ChevronDown, 12));
    let model_button = gtk::Button::new();
    model_button.set_child(Some(&model_content));
    model_button.set_sensitive(false);
    model_button.set_tooltip_text(Some("Choose a model"));
    model_button.add_css_class("composer-affordance");

    let thinking_label = gtk::Label::new(Some("Off"));
    let thinking_content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    let thinking_icon = icons::icon(icons::Icon::BrainCircuit, 15);
    thinking_icon.add_css_class("thinking-level-icon");
    thinking_content.append(&thinking_icon);
    thinking_content.append(&thinking_label);
    thinking_content.append(&icons::icon(icons::Icon::ChevronDown, 12));
    let thinking_button = gtk::Button::new();
    thinking_button.set_child(Some(&thinking_content));
    thinking_button.set_sensitive(false);
    thinking_button.set_tooltip_text(Some("Set reasoning effort"));
    thinking_button.add_css_class("composer-affordance");

    let thinking_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let thinking_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    thinking_content.set_margin_top(8);
    thinking_content.set_margin_bottom(8);
    thinking_content.set_margin_start(8);
    thinking_content.set_margin_end(8);
    let thinking_heading = gtk::Label::new(Some("Reasoning effort"));
    thinking_heading.set_xalign(0.0);
    thinking_heading.add_css_class("thinking-popover-heading");
    thinking_content.append(&thinking_heading);
    thinking_content.append(&thinking_list);
    let thinking_popover = gtk::Popover::builder()
        .has_arrow(false)
        .autohide(true)
        .position(gtk::PositionType::Top)
        .child(&thinking_content)
        .build();
    thinking_popover.add_css_class("thinking-popover");
    thinking_popover.set_parent(&thinking_button);

    let extension_status = gtk::Label::new(None);
    extension_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    extension_status.set_visible(false);
    extension_status.add_css_class("extension-status");
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let send = icons::icon_button(icons::Icon::SendHorizontal, "Send · Enter");
    send.add_css_class("send-button");
    send.set_sensitive(false);
    send.set_tooltip_text(Some("Send · Enter"));

    let extension_above = gtk::Box::new(gtk::Orientation::Vertical, 4);
    extension_above.set_visible(false);
    extension_above.add_css_class("extension-widgets");
    let extension_below = gtk::Box::new(gtk::Orientation::Vertical, 4);
    extension_below.set_visible(false);
    extension_below.add_css_class("extension-widgets");
    controls.append(&model_button);
    controls.append(&thinking_button);
    controls.append(&extension_status);
    controls.append(&spacer);
    controls.append(&send);
    composer.append(&subagent_bar);
    composer.append(&extension_above);
    composer.append(&input_overlay);
    composer.append(&extension_below);
    composer.append(&controls);

    let completion_list = gtk::ListBox::new();
    completion_list.set_selection_mode(gtk::SelectionMode::Single);
    completion_list.add_css_class("completion-list");
    let completion_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(380)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .min_content_width(580)
        .child(&completion_list)
        .build();
    let completion = gtk::Popover::builder()
        .has_arrow(false)
        .autohide(false)
        .position(gtk::PositionType::Top)
        .child(&completion_scroll)
        .build();
    completion.add_css_class("command-popover");
    completion.set_halign(gtk::Align::Fill);
    completion.set_hexpand(true);
    completion.set_parent(&composer);
    let completion_for_unrealize = completion.clone();
    let thinking_for_unrealize = thinking_popover.clone();
    composer.connect_unrealize(move |_| {
        completion_for_unrealize.unparent();
        thinking_for_unrealize.unparent();
    });

    ComposerWidgets {
        root: composer,
        input,
        send,
        model_button,
        model_label,
        model_icon,
        thinking_button,
        thinking_label,
        thinking_popover,
        thinking_list,
        completion,
        completion_list,
        extension_above,
        extension_below,
        extension_status,
        subagent_bar,
        subagent_count,
        subagent_chips,
    }
}

pub fn thinking_option(level: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("thinking-option");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title = gtk::Label::new(Some(&thinking_title(level)));
    title.set_xalign(0.0);
    title.add_css_class("thinking-option-title");
    let detail = gtk::Label::new(Some(thinking_description(level)));
    detail.set_xalign(0.0);
    detail.add_css_class("thinking-option-detail");
    content.append(&title);
    content.append(&detail);
    button.set_child(Some(&content));
    button
}

fn thinking_title(level: &str) -> String {
    let normalized = level.replace(['-', '_'], " ");
    let normalized = normalized.trim();
    let mut characters = normalized.chars();
    characters.next().map_or_else(
        || "Off".to_owned(),
        |first| first.to_uppercase().collect::<String>() + characters.as_str(),
    )
}

fn thinking_description(level: &str) -> &'static str {
    match level {
        "off" | "inherit" => "Respond without extended reasoning",
        "minimal" => "Fastest pass for straightforward work",
        "low" => "Light reasoning with low latency",
        "medium" => "Balanced depth and response time",
        "high" => "Thorough reasoning for complex work",
        "xhigh" | "max" => "Maximum available reasoning depth",
        _ => "Use this model's configured reasoning effort",
    }
}

pub fn subagent_chip(name: &str, status: &str, active: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("subagent-chip");
    if active {
        button.add_css_class("subagent-chip-active");
    } else {
        button.add_css_class("subagent-chip-done");
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    let spinner = gtk::Spinner::new();
    spinner.set_visible(active);
    if active {
        spinner.start();
    }
    let icon = icons::icon(icons::Icon::Users, 14);
    icon.set_visible(!active);
    let label = gtk::Label::new(Some(name));
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(18);
    let status = gtk::Label::new(Some(status));
    status.add_css_class("subagent-chip-status");
    content.append(&spinner);
    content.append(&icon);
    content.append(&label);
    content.append(&status);
    button.set_child(Some(&content));
    button.set_tooltip_text(Some("Open subagent transcript"));
    button
}
