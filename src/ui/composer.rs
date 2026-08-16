use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use super::icons;

pub(crate) fn texture_from_pixbuf(pixbuf: &gdk_pixbuf::Pixbuf) -> gtk::gdk::Texture {
    let format = if pixbuf.has_alpha() {
        gtk::gdk::MemoryFormat::R8g8b8a8
    } else {
        gtk::gdk::MemoryFormat::R8g8b8
    };
    gtk::gdk::MemoryTexture::new(
        pixbuf.width(),
        pixbuf.height(),
        format,
        &pixbuf.read_pixel_bytes(),
        pixbuf.rowstride() as usize,
    )
    .upcast()
}

#[derive(Clone)]
pub(crate) struct ComposerView {
    root: gtk::Box,
    input: gtk::TextView,
    attach: gtk::Button,
    session_actions: gtk::MenuButton,
    branch: gtk::Button,
    handoff: gtk::Button,
    attachment_strip: gtk::ScrolledWindow,
    attachment_list: gtk::Box,
    attachment_previews: Rc<RefCell<HashMap<u64, gtk::Box>>>,
    send: gtk::Button,
    stop: gtk::Button,
    queue_count: gtk::Label,
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
}

pub(crate) fn build() -> ComposerView {
    let composer = gtk::Box::new(gtk::Orientation::Vertical, 4);
    composer.add_css_class("composer");

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
    let attachment_list = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    attachment_list.update_property(&[gtk::accessible::Property::Label("Attached images")]);
    let attachment_strip = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(false)
        .child(&attachment_list)
        .build();
    attachment_strip.set_size_request(-1, 96);
    attachment_strip.set_margin_top(7);
    attachment_strip.set_margin_start(10);
    attachment_strip.set_margin_end(10);
    attachment_strip.set_visible(false);
    attachment_strip.add_css_class("attachment-strip");
    let attachment_previews = Rc::new(RefCell::new(HashMap::new()));

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    controls.set_margin_top(2);
    controls.set_margin_bottom(8);
    controls.set_margin_start(8);
    controls.set_margin_end(8);
    let attach = icons::icon_button(icons::Icon::Paperclip, "Attach PNG or JPEG images");
    attach.update_property(&[gtk::accessible::Property::Label("Attach images")]);
    attach.set_sensitive(false);
    attach.add_css_class("composer-affordance");
    attach.add_css_class("attachment-button");
    let (session_actions, branch, handoff) = build_session_actions();

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

    let queue_count = gtk::Label::new(None);
    queue_count.set_visible(false);
    queue_count.add_css_class("queue-count");
    queue_count.update_property(&[gtk::accessible::Property::Label("Queued messages")]);

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
    controls.append(&queue_count);
    controls.append(&spacer);
    controls.append(&attach);
    controls.append(&session_actions);
    controls.append(&stop);
    controls.append(&send);
    composer.append(&extension_above);
    composer.append(&attachment_strip);
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
        attach,
        session_actions,
        branch,
        handoff,
        attachment_strip,
        attachment_list,
        attachment_previews,
        send,
        stop,
        queue_count,
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

    pub(crate) fn connect_attach_clicked(&self, callback: impl Fn() + 'static) {
        self.attach.connect_clicked(move |_| callback());
    }

    pub(crate) fn connect_branch_clicked(&self, callback: impl Fn() + 'static) {
        self.branch.connect_clicked(move |_| callback());
    }

    pub(crate) fn connect_handoff_clicked(&self, callback: impl Fn() + 'static) {
        self.handoff.connect_clicked(move |_| callback());
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
        self.stop.set_visible(running);
        self.stop.set_sensitive(ready && running);
        self.send.set_visible(!running);
        self.send.set_tooltip_text(Some("Send · Enter"));
        self.send
            .update_property(&[gtk::accessible::Property::Label("Send · Enter")]);
        self.send.set_sensitive(
            ready && !running && (!self.text().trim().is_empty() || self.has_attachments()),
        );
    }

    pub(crate) fn set_submission_pending(&self, pending: bool) {
        if pending {
            self.send.set_sensitive(false);
        }
    }

    pub(crate) fn set_queued_message_count(&self, queued: usize) {
        self.queue_count.set_text(&format!("{queued} queued"));
        self.queue_count.set_visible(queued > 0);
        self.queue_count.set_tooltip_text(Some(&format!(
            "{queued} queued messages · Ctrl+Enter or Ctrl+Q queues a follow-up"
        )));
    }

    pub(crate) fn set_attachment_sensitive(&self, sensitive: bool) {
        self.attach.set_sensitive(sensitive);
    }

    pub(crate) fn set_session_actions_visible(&self, visible: bool) {
        self.session_actions.set_visible(visible);
        self.branch.set_visible(visible);
        self.handoff.set_visible(visible);
    }

    pub(crate) fn append_attachment_preview(
        &self,
        id: u64,
        name: &str,
        texture: &gtk::gdk::Texture,
        callback: impl Fn(u64) + 'static,
    ) {
        let texture = Self::attachment_preview_texture(texture);
        let picture = gtk::Picture::for_paintable(&texture);
        picture.set_can_shrink(true);
        picture.set_hexpand(false);
        picture.set_vexpand(false);
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_size_request(76, 64);
        picture.update_property(&[gtk::accessible::Property::Description(&format!(
            "Preview of {name}"
        ))]);

        let remove = icons::icon_button(icons::Icon::X, &format!("Remove {name}"));
        remove.update_property(&[gtk::accessible::Property::Label(&format!("Remove {name}"))]);
        remove.set_halign(gtk::Align::End);
        remove.set_valign(gtk::Align::Start);
        remove.add_css_class("attachment-remove");
        remove.connect_clicked(move |_| callback(id));

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&picture));
        overlay.add_overlay(&remove);
        let label = gtk::Label::new(Some(name));
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_max_width_chars(14);
        label.set_tooltip_text(Some(name));
        let preview = gtk::Box::new(gtk::Orientation::Vertical, 3);
        preview.append(&overlay);
        preview.append(&label);
        preview.add_css_class("attachment-preview");

        self.attachment_list.append(&preview);
        self.attachment_previews.borrow_mut().insert(id, preview);
        self.attachment_strip.set_visible(true);
    }

    pub(crate) fn remove_attachment_preview(&self, id: u64) {
        if let Some(preview) = self.attachment_previews.borrow_mut().remove(&id) {
            self.attachment_list.remove(&preview);
        }
        self.attachment_strip
            .set_visible(!self.attachment_previews.borrow().is_empty());
    }

    pub(crate) fn clear_attachment_previews(&self) {
        while let Some(child) = self.attachment_list.first_child() {
            self.attachment_list.remove(&child);
        }
        self.attachment_previews.borrow_mut().clear();
        self.attachment_strip.set_visible(false);
    }

    fn attachment_preview_texture(texture: &gtk::gdk::Texture) -> gtk::gdk::Texture {
        if texture.width() <= 76 && texture.height() <= 64 {
            return texture.clone();
        }
        let bytes = texture.save_to_png_bytes();
        let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes);
        gdk_pixbuf::Pixbuf::from_stream_at_scale(
            &stream,
            76,
            64,
            true,
            None::<&gtk::gio::Cancellable>,
        )
        .map(|preview| texture_from_pixbuf(&preview))
        .unwrap_or_else(|_| texture.clone())
    }

    pub(crate) fn has_attachments(&self) -> bool {
        !self.attachment_previews.borrow().is_empty()
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

fn build_session_actions() -> (gtk::MenuButton, gtk::Button, gtk::Button) {
    let root = gtk::MenuButton::new();
    root.set_visible(false);
    root.set_tooltip_text(Some("Choose how to continue in a new conversation"));
    root.update_property(&[gtk::accessible::Property::Label(
        "Continue in a new conversation",
    )]);
    root.add_css_class("session-actions");
    root.set_child(Some(&icons::icon(icons::Icon::MessageSquareShare, 16)));

    let popover = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .position(gtk::PositionType::Top)
        .build();
    popover.add_css_class("session-actions-popover");
    let actions = gtk::Box::new(gtk::Orientation::Vertical, 2);
    actions.add_css_class("session-actions-menu");
    let heading = gtk::Label::new(Some("Continue in a new conversation"));
    heading.set_xalign(0.0);
    heading.add_css_class("session-actions-heading");
    let help = gtk::Label::new(Some("Choose how much of this conversation moves forward."));
    help.set_xalign(0.0);
    help.set_wrap(true);
    help.add_css_class("session-actions-help");
    actions.append(&heading);
    actions.append(&help);

    let branch = gtk::Button::new();
    branch.set_tooltip_text(Some(
        "Copy history through a user message into an independent conversation",
    ));
    branch.update_property(&[gtk::accessible::Property::Label(
        "Branch into an independent conversation from a user message",
    )]);
    branch.add_css_class("session-action");
    let branch_content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    branch_content.append(&icons::icon(icons::Icon::GitBranchPlus, 17));
    let branch_copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
    branch_copy.set_hexpand(true);
    let branch_title = gtk::Label::new(Some("Branch from a message"));
    branch_title.set_xalign(0.0);
    branch_title.add_css_class("session-action-title");
    let branch_detail = gtk::Label::new(Some("Copy full history through a point"));
    branch_detail.set_xalign(0.0);
    branch_detail.add_css_class("session-action-detail");
    branch_copy.append(&branch_title);
    branch_copy.append(&branch_detail);
    branch_content.append(&branch_copy);
    branch.set_child(Some(&branch_content));

    let handoff = gtk::Button::new();
    handoff.set_tooltip_text(Some(
        "Summarize this conversation and continue in a fresh one",
    ));
    handoff.update_property(&[gtk::accessible::Property::Label(
        "Continue in a new conversation with a focused summary",
    )]);
    handoff.add_css_class("session-action");
    let handoff_content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    handoff_content.append(&icons::icon(icons::Icon::MessageSquareShare, 17));
    let handoff_copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
    handoff_copy.set_hexpand(true);
    let handoff_title = gtk::Label::new(Some("Summarize and continue"));
    handoff_title.set_xalign(0.0);
    handoff_title.add_css_class("session-action-title");
    let handoff_detail = gtk::Label::new(Some("Carry a focused summary into a fresh conversation"));
    handoff_detail.set_xalign(0.0);
    handoff_detail.set_wrap(true);
    handoff_detail.add_css_class("session-action-detail");
    handoff_copy.append(&handoff_title);
    handoff_copy.append(&handoff_detail);
    handoff_content.append(&handoff_copy);
    handoff.set_child(Some(&handoff_content));

    actions.append(&branch);
    actions.append(&handoff);
    popover.set_child(Some(&actions));
    root.set_popover(Some(&popover));
    let popover_for_branch = popover.clone();
    branch.connect_clicked(move |_| popover_for_branch.popdown());
    handoff.connect_clicked(move |_| popover.popdown());

    (root, branch, handoff)
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
        "xhigh" => "Extended".to_owned(),
        "max" => "Maximum".to_owned(),
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
        "xhigh" => "Extended reasoning for demanding work",
        "max" => "Maximum reasoning the model supports",
        _ => "Uses this model's recommended reasoning depth",
    }
}

#[cfg(test)]
mod tests {
    use super::{thinking_description, thinking_title};

    #[test]
    fn distinguishes_top_reasoning_effort_tiers() {
        assert_eq!(thinking_title("xhigh"), "Extended");
        assert_eq!(
            thinking_description("xhigh"),
            "Extended reasoning for demanding work"
        );
        assert_eq!(thinking_title("max"), "Maximum");
        assert_eq!(
            thinking_description("max"),
            "Maximum reasoning the model supports"
        );
    }
}
