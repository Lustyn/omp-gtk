use gtk::prelude::*;
use gtk4 as gtk;
use crate::bridge::protocol::{InterruptMode, QueueMode};

use super::icons;

#[derive(Clone)]
pub(crate) struct ComposerView {
    root: gtk::Box,
    input: gtk::TextView,
    send: gtk::Button,
    stop: gtk::Button,
    running_actions: gtk::Box,
    steer: gtk::ToggleButton,
    follow_up: gtk::ToggleButton,
    queue_count: gtk::Label,
    queue_settings: gtk::MenuButton,
    steering_mode: gtk::DropDown,
    follow_up_mode: gtk::DropDown,
    interrupt_mode: gtk::DropDown,
    model_button: gtk::Button,
    model_label: gtk::Label,
    model_icon: icons::ProviderIcon,
    model_popover: gtk::Popover,
    thinking_button: gtk::Button,
    thinking_label: gtk::Label,
    thinking_popover: gtk::Popover,
    thinking_list: gtk::Box,
    completion: gtk::Popover,
    completion_list: gtk::ListBox,
    extension_above: gtk::Box,
    extension_below: gtk::Box,
    extension_status: gtk::Label,
    subagent_bar: gtk::Box,
    subagent_count: gtk::Label,
    subagent_chips: gtk::Box,
}

pub(crate) fn build() -> ComposerView {
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
    input.update_property(&[gtk::accessible::Property::Label("Prompt")]);
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
    let model_popover = gtk::Popover::builder()
        .has_arrow(false)
        .autohide(true)
        .position(gtk::PositionType::Top)
        .build();
    model_popover.add_css_class("model-picker-popover");
    model_popover.set_parent(&model_button);

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
    thinking_button.set_tooltip_text(Some("Choose reasoning depth"));
    thinking_button.add_css_class("composer-affordance");

    let thinking_list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let thinking_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    thinking_content.set_margin_top(10);
    thinking_content.set_margin_bottom(10);
    thinking_content.set_margin_start(10);
    thinking_content.set_margin_end(10);
    let thinking_heading = gtk::Label::new(Some("Reasoning depth"));
    thinking_heading.set_xalign(0.0);
    thinking_heading.add_css_class("thinking-popover-heading");
    let thinking_help = gtk::Label::new(Some(
        "More depth can improve complex work, but may take longer.",
    ));
    thinking_help.set_xalign(0.0);
    thinking_help.set_wrap(true);
    thinking_help.add_css_class("thinking-popover-help");
    thinking_content.append(&thinking_heading);
    thinking_content.append(&thinking_help);
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

    let running_actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    running_actions.add_css_class("linked");
    running_actions.set_visible(false);
    let steer = gtk::ToggleButton::with_label("Steer");
    steer.set_active(true);
    steer.set_tooltip_text(Some("Deliver during the active turn"));
    steer.update_property(&[gtk::accessible::Property::Label("Steer active turn")]);
    let follow_up = gtk::ToggleButton::with_label("Follow up");
    follow_up.set_group(Some(&steer));
    follow_up.set_tooltip_text(Some("Queue until the active turn finishes"));
    follow_up.update_property(&[gtk::accessible::Property::Label(
        "Follow up after active turn",
    )]);
    steer.add_css_class("composer-affordance");
    follow_up.add_css_class("composer-affordance");
    running_actions.append(&steer);
    running_actions.append(&follow_up);

    let queue_count = gtk::Label::new(None);
    queue_count.set_visible(false);
    queue_count.add_css_class("queue-count");
    queue_count.update_property(&[gtk::accessible::Property::Label("Queued messages")]);

    let steering_mode = queue_mode_dropdown("Steering delivery");
    let follow_up_mode = queue_mode_dropdown("Follow-up delivery");
    let interrupt_mode = gtk::DropDown::from_strings(&["Immediate", "Wait for turn"]);
    interrupt_mode.update_property(&[gtk::accessible::Property::Label(
        "Steering interrupt timing",
    )]);
    let settings_content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    settings_content.set_margin_top(12);
    settings_content.set_margin_bottom(12);
    settings_content.set_margin_start(12);
    settings_content.set_margin_end(12);
    append_setting_row(&settings_content, "Steering messages", &steering_mode);
    append_setting_row(&settings_content, "Follow-up messages", &follow_up_mode);
    append_setting_row(&settings_content, "Interrupt tools", &interrupt_mode);
    let settings_popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Top)
        .child(&settings_content)
        .build();
    let queue_settings = gtk::MenuButton::new();
    queue_settings.set_label("Queue settings");
    queue_settings.set_popover(Some(&settings_popover));
    queue_settings.set_tooltip_text(Some("Configure queued message delivery"));
    queue_settings.add_css_class("composer-affordance");
    queue_settings.update_property(&[gtk::accessible::Property::Label(
        "Queue settings",
    )]);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let send = icons::icon_button(icons::Icon::SendHorizontal, "Send · Enter");
    send.add_css_class("send-button");
    send.set_sensitive(false);
    send.set_tooltip_text(Some("Send · Enter"));
    let stop = icons::icon_button(icons::Icon::Square, "Stop response");
    stop.add_css_class("stop-button");
    stop.set_visible(false);
    stop.set_sensitive(false);

    let extension_above = gtk::Box::new(gtk::Orientation::Vertical, 4);
    extension_above.set_visible(false);
    extension_above.add_css_class("extension-widgets");
    let extension_below = gtk::Box::new(gtk::Orientation::Vertical, 4);
    extension_below.set_visible(false);
    extension_below.add_css_class("extension-widgets");
    controls.append(&model_button);
    controls.append(&thinking_button);
    controls.append(&extension_status);
    controls.append(&running_actions);
    controls.append(&queue_count);
    controls.append(&queue_settings);
    controls.append(&spacer);
    controls.append(&stop);
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
    let model_for_unrealize = model_popover.clone();
    let thinking_for_unrealize = thinking_popover.clone();
    composer.connect_unrealize(move |_| {
        completion_for_unrealize.unparent();
        model_for_unrealize.unparent();
        thinking_for_unrealize.unparent();
    });

    ComposerView {
        root: composer,
        input,
        send,
        stop,
        running_actions,
        steer,
        follow_up,
        queue_count,
        queue_settings,
        steering_mode,
        follow_up_mode,
        interrupt_mode,
        model_button,
        model_label,
        model_icon,
        model_popover,
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

impl ComposerView {
    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn connect_changed(&self, callback: impl Fn() + 'static) {
        self.input.buffer().connect_changed(move |_| callback());
    }

    pub(crate) fn add_key_controller(&self, controller: gtk::EventControllerKey) {
        self.input.add_controller(controller);
    }

    pub(crate) fn connect_send_clicked(&self, callback: impl Fn() + 'static) {
        self.send.connect_clicked(move |_| callback());
    }
    pub(crate) fn connect_stop_clicked(&self, callback: impl Fn() + 'static) {
        self.stop.connect_clicked(move |_| callback());
    }

    pub(crate) fn connect_steer_selected(&self, callback: impl Fn() + 'static) {
        self.steer.connect_toggled(move |button| {
            if button.is_active() {
                callback();
            }
        });
    }

    pub(crate) fn connect_follow_up_selected(&self, callback: impl Fn() + 'static) {
        self.follow_up.connect_toggled(move |button| {
            if button.is_active() {
                callback();
            }
        });
    }

    pub(crate) fn connect_steering_mode_changed(
        &self,
        callback: impl Fn(QueueMode) + 'static,
    ) {
        self.steering_mode.connect_selected_notify(move |dropdown| {
            callback(queue_mode_from_selected(dropdown.selected()));
        });
    }

    pub(crate) fn connect_follow_up_mode_changed(
        &self,
        callback: impl Fn(QueueMode) + 'static,
    ) {
        self.follow_up_mode.connect_selected_notify(move |dropdown| {
            callback(queue_mode_from_selected(dropdown.selected()));
        });
    }

    pub(crate) fn connect_interrupt_mode_changed(
        &self,
        callback: impl Fn(InterruptMode) + 'static,
    ) {
        self.interrupt_mode.connect_selected_notify(move |dropdown| {
            callback(match dropdown.selected() {
                1 => InterruptMode::Wait,
                _ => InterruptMode::Immediate,
            });
        });
    }


    pub(crate) fn connect_model_clicked(&self, callback: impl Fn() + 'static) {
        self.model_button.connect_clicked(move |_| callback());
    }

    pub(crate) fn connect_thinking_clicked(&self, callback: impl Fn() + 'static) {
        self.thinking_button.connect_clicked(move |_| callback());
    }

    pub(crate) fn connect_completion_activated(&self, callback: impl Fn(i32) + 'static) {
        self.completion_list
            .connect_row_activated(move |_, row| callback(row.index()));
    }

    pub(crate) fn set_input_sensitive(&self, sensitive: bool) {
        self.input.set_sensitive(sensitive);
    }

    pub(crate) fn text(&self) -> String {
        let buffer = self.input.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }

    pub(crate) fn set_text(&self, text: &str) {
        self.input.buffer().set_text(text);
    }

    pub(crate) fn focus(&self) {
        self.input.grab_focus();
    }

    pub(crate) fn set_primary_action(&self, ready: bool, running: bool) {
        self.running_actions.set_visible(running);
        self.stop.set_visible(running);
        self.stop.set_sensitive(ready && running);
        self.queue_settings.set_sensitive(ready);
        icons::set_button_icon(&self.send, icons::Icon::SendHorizontal);
        let action = if running {
            if self.steer.is_active() {
                "Steer active turn · Enter"
            } else {
                "Queue follow-up · Enter"
            }
        } else {
            "Send · Enter"
        };
        self.send.set_tooltip_text(Some(action));
        self.send
            .update_property(&[gtk::accessible::Property::Label(action)]);
        self.send
            .set_sensitive(ready && !self.text().trim().is_empty());
    }

    pub(crate) fn set_running_turn_action(&self, steer_selected: bool) {
        if steer_selected {
            self.steer.set_active(true);
        } else {
            self.follow_up.set_active(true);
        }
    }

    pub(crate) fn set_submission_pending(&self, pending: bool) {
        if pending {
            self.send.set_sensitive(false);
        }
    }

    pub(crate) fn set_queue_state(
        &self,
        steering: QueueMode,
        follow_up: QueueMode,
        interrupt: InterruptMode,
        queued: usize,
    ) {
        self.steering_mode.set_selected(queue_mode_selected(steering));
        self.follow_up_mode
            .set_selected(queue_mode_selected(follow_up));
        self.interrupt_mode.set_selected(match interrupt {
            InterruptMode::Immediate => 0,
            InterruptMode::Wait => 1,
        });
        self.queue_count.set_text(&format!("{queued} queued"));
        self.queue_count.set_visible(queued > 0);
        self.queue_count
            .set_tooltip_text(Some(&format!("{queued} queued messages")));
    }

    pub(crate) fn set_model(&self, provider: &str, display_name: &str) {
        icons::set_provider_icon(&self.model_icon, provider);
        self.model_label.set_text(display_name);
        self.model_button.set_tooltip_text(Some(&format!(
            "{display_name} · {}",
            icons::provider_label(provider)
        )));
    }

    pub(crate) fn set_model_provider(&self, provider: &str) {
        icons::set_provider_icon(&self.model_icon, provider);
    }

    pub(crate) fn set_model_sensitive(&self, sensitive: bool) {
        self.model_button.set_sensitive(sensitive);
    }

    pub(crate) fn model_picker_visible(&self) -> bool {
        self.model_popover.is_visible()
    }

    pub(crate) fn show_model_picker(&self, child: &gtk::Widget) {
        self.thinking_popover.popdown();
        self.model_popover.set_child(Some(child));
        self.model_popover.popup();
    }

    pub(crate) fn close_model_picker(&self) {
        self.model_popover.popdown();
    }

    pub(crate) fn show_thinking_popover(&self) {
        self.model_popover.popdown();
        self.thinking_popover.popup();
    }

    pub(crate) fn clear_thinking_options(&self) {
        while let Some(child) = self.thinking_list.first_child() {
            self.thinking_list.remove(&child);
        }
    }

    pub(crate) fn append_thinking_option(&self, option: &gtk::Button) {
        self.thinking_list.append(option);
    }

    pub(crate) fn set_thinking_sensitive(&self, sensitive: bool) {
        self.thinking_button.set_sensitive(sensitive);
    }

    pub(crate) fn set_thinking_label(&self, level: &str) {
        self.thinking_label.set_text(&thinking_title(level));
    }

    pub(crate) fn close_thinking_popover(&self) {
        self.thinking_popover.popdown();
    }

    pub(crate) fn clear_completion_rows(&self) {
        while let Some(child) = self.completion_list.first_child() {
            self.completion_list.remove(&child);
        }
    }

    pub(crate) fn append_completion_row(&self, row: &gtk::ListBoxRow) {
        self.completion_list.append(row);
    }

    pub(crate) fn select_completion(&self, index: i32) -> bool {
        let Some(row) = self.completion_list.row_at_index(index) else {
            return false;
        };
        self.completion_list.select_row(Some(&row));
        true
    }

    pub(crate) fn completions_visible(&self) -> bool {
        self.completion.is_visible()
    }
    pub(crate) fn show_completions(&self) {
        if let Some(parent) = self.completion.parent() {
            self.completion.set_width_request(parent.width());
        }
        self.completion.popup();
    }

    pub(crate) fn hide_completions(&self) {
        self.completion.popdown();
    }

    pub(crate) fn clear_subagent_chips(&self) {
        while let Some(child) = self.subagent_chips.first_child() {
            self.subagent_chips.remove(&child);
        }
    }

    pub(crate) fn append_subagent_chip(&self, chip: &gtk::Button) {
        self.subagent_chips.append(chip);
    }

    pub(crate) fn set_subagents_visible(&self, visible: bool) {
        self.subagent_bar.set_visible(visible);
    }

    pub(crate) fn set_subagent_count(&self, text: &str) {
        self.subagent_count.set_text(text);
    }

    pub(crate) fn set_extension_status(&self, text: &str) {
        self.extension_status.set_text(text);
        self.extension_status.set_visible(!text.is_empty());
    }

    pub(crate) fn remove_extension_widget(&self, label: &gtk::Label) {
        if let Some(parent) = label.parent().and_downcast::<gtk::Box>() {
            parent.remove(label);
            parent.set_visible(parent.first_child().is_some());
        }
    }

    pub(crate) fn append_extension_widget(&self, label: &gtk::Label, below_editor: bool) {
        let container = if below_editor {
            &self.extension_below
        } else {
            &self.extension_above
        };
        container.append(label);
        container.set_visible(true);
    }
}

fn queue_mode_dropdown(accessible_label: &str) -> gtk::DropDown {
    let dropdown = gtk::DropDown::from_strings(&["One at a time", "All at once"]);
    dropdown.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    dropdown
}

fn append_setting_row(container: &gtk::Box, title: &str, dropdown: &gtk::DropDown) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);
    row.append(dropdown);
    container.append(&row);
}

fn queue_mode_selected(mode: QueueMode) -> u32 {
    match mode {
        QueueMode::OneAtATime => 0,
        QueueMode::All => 1,
    }
}

fn queue_mode_from_selected(selected: u32) -> QueueMode {
    match selected {
        1 => QueueMode::All,
        _ => QueueMode::OneAtATime,
    }
}

pub fn thinking_option(level: &str) -> gtk::Button {
    let title_text = thinking_title(level);
    let detail_text = thinking_description(level);
    let button = gtk::Button::new();
    button.add_css_class("thinking-option");
    button.set_tooltip_text(Some(detail_text));
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    let title = gtk::Label::new(Some(&title_text));
    title.set_xalign(0.0);
    title.add_css_class("thinking-option-title");
    let detail = gtk::Label::new(Some(detail_text));
    detail.set_xalign(0.0);
    detail.add_css_class("thinking-option-detail");
    labels.append(&title);
    labels.append(&detail);
    content.append(&labels);
    let check = icons::icon(icons::Icon::Check, 15);
    check.add_css_class("thinking-option-check");
    content.append(&check);
    button.set_child(Some(&content));
    button
}

fn thinking_title(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "" | "off" | "inherit" | "none" => "Off".to_owned(),
        "minimal" => "Minimal".to_owned(),
        "low" => "Light".to_owned(),
        "medium" => "Balanced".to_owned(),
        "high" => "Deep".to_owned(),
        "xhigh" | "max" => "Maximum".to_owned(),
        _ => {
            let normalized = level.replace(['-', '_'], " ");
            let normalized = normalized.trim();
            let mut characters = normalized.chars();
            characters.next().map_or_else(
                || "Off".to_owned(),
                |first| first.to_uppercase().collect::<String>() + characters.as_str(),
            )
        }
    }
}

fn thinking_description(level: &str) -> &'static str {
    match level {
        "off" | "inherit" | "none" => "Quick answers for simple requests",
        "minimal" => "A fast answer with a light check",
        "low" => "Quick reasoning for routine work",
        "medium" => "Balanced for most tasks",
        "high" => "Deeper reasoning for complex work",
        "xhigh" | "max" => "Most thorough; may take longer",
        _ => "Uses this model's recommended reasoning depth",
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
