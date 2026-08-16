use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use super::icons;
use crate::bridge::protocol::BranchMessage;

#[derive(Clone)]
pub(crate) struct BranchPickerView {
    root: gtk::Box,
    close: gtk::Button,
    stack: gtk::Stack,
    list: gtk::ListBox,
    state_spinner: gtk::Spinner,
    state_icon: gtk::Label,
    state_title: gtk::Label,
    state_detail: gtk::Label,
    candidates: Rc<RefCell<Vec<BranchMessage>>>,
    on_select: Rc<dyn Fn(String)>,
}

impl BranchPickerView {
    pub(crate) fn new(on_select: impl Fn(String) + 'static) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_size_request(680, 600);
        root.set_accessible_role(gtk::AccessibleRole::Dialog);
        root.update_property(&[gtk::accessible::Property::Label(
            "Start an independent conversation from a user message",
        )]);
        root.add_css_class("branch-picker");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        header.add_css_class("branch-picker-header");
        let heading_copy = gtk::Box::new(gtk::Orientation::Vertical, 3);
        heading_copy.set_hexpand(true);
        let heading = gtk::Label::new(Some("Start an independent branch"));
        heading.set_xalign(0.0);
        heading.add_css_class("branch-picker-heading");
        let subtitle = gtk::Label::new(Some(
            "Choose a user message. The new conversation copies history through that point, while this conversation stays unchanged.",
        ));
        subtitle.set_xalign(0.0);
        subtitle.set_wrap(true);
        subtitle.add_css_class("branch-picker-subtitle");
        heading_copy.append(&heading);
        heading_copy.append(&subtitle);
        let close = icons::icon_button(icons::Icon::X, "Close branch conversation picker");
        close.add_css_class("branch-picker-close");
        header.append(&heading_copy);
        header.append(&close);
        root.append(&header);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("branch-picker-list");
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        scroll.add_css_class("branch-picker-scroll");

        let state = gtk::Box::new(gtk::Orientation::Vertical, 8);
        state.set_halign(gtk::Align::Center);
        state.set_valign(gtk::Align::Center);
        state.set_margin_start(32);
        state.set_margin_end(32);
        state.add_css_class("branch-picker-state");
        let state_spinner = gtk::Spinner::new();
        state_spinner.set_size_request(24, 24);
        let state_icon = icons::icon(icons::Icon::GitBranch, 24);
        state_icon.add_css_class("branch-picker-state-icon");
        let state_title = gtk::Label::new(None);
        state_title.add_css_class("branch-picker-state-title");
        let state_detail = gtk::Label::new(None);
        state_detail.set_wrap(true);
        state_detail.set_justify(gtk::Justification::Center);
        state_detail.add_css_class("branch-picker-state-detail");
        state.append(&state_spinner);
        state.append(&state_icon);
        state.append(&state_title);
        state.append(&state_detail);

        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.add_named(&scroll, Some("candidates"));
        stack.add_named(&state, Some("state"));
        root.append(&stack);

        let view = Self {
            root,
            close,
            stack,
            list,
            state_spinner,
            state_icon,
            state_title,
            state_detail,
            candidates: Rc::new(RefCell::new(Vec::new())),
            on_select: Rc::new(on_select),
        };
        view.show_loading();
        view
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn close_button(&self) -> &gtk::Button {
        &self.close
    }

    pub(crate) fn show_loading(&self) {
        self.show_state(
            "Finding branch points…",
            "Loading user messages from this conversation.",
            true,
        );
    }

    pub(crate) fn show_branching(&self) {
        self.show_state(
            "Starting an independent conversation…",
            "Copying history through the selected message. This conversation will stay unchanged.",
            true,
        );
    }

    pub(crate) fn show_error(&self, message: &str) {
        self.show_state(
            "Branch could not continue",
            if message.trim().is_empty() {
                "omp could not complete the branch request. This conversation was not changed."
            } else {
                message
            },
            false,
        );
        self.state_icon.add_css_class("error");
    }

    pub(crate) fn set_candidates(&self, candidates: Vec<BranchMessage>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.candidates.replace(candidates.clone());
        if candidates.is_empty() {
            self.show_state(
                "Nothing to branch from yet",
                "Send a user message first. You can then choose where the independent conversation begins.",
                false,
            );
            return;
        }

        for candidate in &candidates {
            let row = branch_row(candidate);
            let entry_id = candidate.entry_id.clone();
            let candidates = self.candidates.clone();
            let on_select = self.on_select.clone();
            row.1.connect_clicked(move |_| {
                if let Some(selection) = selection_for_entry(&candidates.borrow(), &entry_id) {
                    on_select(selection);
                }
            });
            self.list.append(&row.0);
        }
        self.state_spinner.stop();
        self.list.set_sensitive(true);
        self.stack.set_visible_child_name("candidates");
    }

    fn show_state(&self, title: &str, detail: &str, spinning: bool) {
        self.state_title.set_text(title);
        self.state_detail.set_text(detail);
        self.state_icon.remove_css_class("error");
        self.state_spinner.set_visible(spinning);
        self.state_icon.set_visible(!spinning);
        if spinning {
            self.state_spinner.start();
        } else {
            self.state_spinner.stop();
        }
        self.list.set_sensitive(false);
        self.stack.set_visible_child_name("state");
    }
}

#[derive(Clone)]
pub(crate) struct BranchPickerDialog {
    dialog: adw::Dialog,
    view: BranchPickerView,
}

impl BranchPickerDialog {
    pub(crate) fn new(on_select: impl Fn(String) + 'static) -> Self {
        let view = BranchPickerView::new(on_select);
        let dialog = adw::Dialog::builder()
            .title("Branch from a message")
            .content_width(680)
            .content_height(600)
            .child(view.widget())
            .build();
        let weak_dialog = dialog.downgrade();
        view.close_button().connect_clicked(move |_| {
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
        Self { dialog, view }
    }

    pub(crate) fn present(&self, parent: &impl IsA<gtk::Widget>) {
        self.dialog.present(Some(parent));
    }

    pub(crate) fn close(&self) {
        self.dialog.close();
    }

    pub(crate) fn show_loading(&self) {
        self.view.show_loading();
    }

    pub(crate) fn show_branching(&self) {
        self.view.show_branching();
    }

    pub(crate) fn show_error(&self, message: &str) {
        self.view.show_error(message);
    }

    pub(crate) fn set_candidates(&self, candidates: Vec<BranchMessage>) {
        self.view.set_candidates(candidates);
    }
}

#[derive(Clone)]
pub(crate) struct HandoffView {
    root: gtk::Box,
    instructions: gtk::TextView,
    cancel: gtk::Button,
    submit: gtk::Button,
}

impl HandoffView {
    pub(crate) fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
        root.set_size_request(580, 460);
        root.set_margin_top(24);
        root.set_margin_bottom(20);
        root.set_margin_start(24);
        root.set_margin_end(24);
        root.set_accessible_role(gtk::AccessibleRole::Dialog);
        root.update_property(&[gtk::accessible::Property::Label(
            "Continue in a new conversation with a summarized handoff",
        )]);
        root.add_css_class("handoff-dialog");

        let heading = gtk::Label::new(Some("Continue with a focused handoff"));
        heading.set_xalign(0.0);
        heading.add_css_class("handoff-heading");
        let detail = gtk::Label::new(Some(
            "omp will summarize this conversation for a fresh one. The conversation here remains unchanged and available in Recent.",
        ));
        detail.set_xalign(0.0);
        detail.set_wrap(true);
        detail.add_css_class("handoff-detail");
        root.append(&heading);
        root.append(&detail);

        let summary = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        summary.add_css_class("handoff-summary");
        let summary_icon = icons::icon(icons::Icon::MessageSquareShare, 18);
        summary_icon.add_css_class("handoff-summary-icon");
        let summary_copy = gtk::Box::new(gtk::Orientation::Vertical, 3);
        summary_copy.set_hexpand(true);
        let summary_title = gtk::Label::new(Some("What moves forward"));
        summary_title.set_xalign(0.0);
        summary_title.add_css_class("handoff-summary-title");
        let summary_detail = gtk::Label::new(Some(
            "Key context, decisions, and open work, distilled into a concise summary.",
        ));
        summary_detail.set_xalign(0.0);
        summary_detail.set_wrap(true);
        summary_detail.add_css_class("handoff-summary-detail");
        summary_copy.append(&summary_title);
        summary_copy.append(&summary_detail);
        summary.append(&summary_icon);
        summary.append(&summary_copy);
        root.append(&summary);

        let difference = gtk::Label::new(Some(
            "Unlike a branch, a handoff does not copy the full message history through a selected point.",
        ));
        difference.set_xalign(0.0);
        difference.set_wrap(true);
        difference.add_css_class("handoff-difference");
        root.append(&difference);

        let instructions_copy = gtk::Box::new(gtk::Orientation::Vertical, 3);
        let instructions_label = gtk::Label::new(Some("Guide the summary (optional)"));
        instructions_label.set_xalign(0.0);
        instructions_label.add_css_class("handoff-instructions-label");
        let instructions_help = gtk::Label::new(Some(
            "Call out what the next conversation should prioritize.",
        ));
        instructions_help.set_xalign(0.0);
        instructions_help.set_wrap(true);
        instructions_help.add_css_class("handoff-instructions-help");
        instructions_copy.append(&instructions_label);
        instructions_copy.append(&instructions_help);
        let instructions = gtk::TextView::new();
        instructions.set_wrap_mode(gtk::WrapMode::WordChar);
        instructions.set_top_margin(9);
        instructions.set_bottom_margin(9);
        instructions.set_left_margin(10);
        instructions.set_right_margin(10);
        instructions.update_property(&[gtk::accessible::Property::Label(
            "Optional guidance for the handoff summary",
        )]);
        instructions.add_css_class("handoff-instructions");
        let instructions_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(88)
            .vexpand(true)
            .child(&instructions)
            .build();
        instructions_scroll.add_css_class("handoff-instructions-scroll");
        let instructions_overlay = gtk::Overlay::new();
        instructions_overlay.set_child(Some(&instructions_scroll));
        let placeholder = gtk::Label::new(Some(
            "For example: Focus on the release plan and unresolved test failures.",
        ));
        placeholder.set_halign(gtk::Align::Start);
        placeholder.set_valign(gtk::Align::Start);
        placeholder.set_margin_top(10);
        placeholder.set_margin_start(11);
        placeholder.set_margin_end(11);
        placeholder.set_wrap(true);
        placeholder.set_can_target(false);
        placeholder.add_css_class("handoff-instructions-placeholder");
        instructions_overlay.add_overlay(&placeholder);
        let placeholder_for_change = placeholder.clone();
        instructions.buffer().connect_changed(move |buffer| {
            placeholder_for_change.set_visible(
                buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .is_empty(),
            );
        });
        root.append(&instructions_copy);
        root.append(&instructions_overlay);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Stay here");
        cancel.update_property(&[gtk::accessible::Property::Label(
            "Stay in this conversation",
        )]);
        let submit = gtk::Button::with_label("Continue with summary");
        submit.update_property(&[gtk::accessible::Property::Label(
            "Continue in a new conversation with a summary",
        )]);
        submit.add_css_class("suggested-action");
        actions.append(&cancel);
        actions.append(&submit);
        root.append(&actions);

        Self {
            root,
            instructions,
            cancel,
            submit,
        }
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn focus_instructions(&self) {
        self.instructions.grab_focus();
    }

    fn instructions(&self) -> String {
        let buffer = self.instructions.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .trim()
            .to_owned()
    }
}

pub(crate) fn present_handoff(
    parent: &impl IsA<gtk::Widget>,
    on_handoff: impl Fn(String) + 'static,
) {
    let view = HandoffView::new();
    let dialog = adw::Dialog::builder()
        .title("Continue with a handoff")
        .content_width(580)
        .content_height(460)
        .child(view.widget())
        .build();

    let weak_dialog = dialog.downgrade();
    view.cancel.connect_clicked(move |_| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });

    let weak_dialog = dialog.downgrade();
    let view_for_submit = view.clone();
    view.submit.connect_clicked(move |_| {
        on_handoff(view_for_submit.instructions());
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });

    dialog.present(Some(parent));
    view.focus_instructions();
}

fn branch_row(candidate: &BranchMessage) -> (gtk::ListBoxRow, gtk::Button) {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);
    row.add_css_class("branch-picker-row");

    let button = gtk::Button::new();
    button.add_css_class("branch-picker-row-action");
    let accessible_preview = one_line_preview(&candidate.text);
    let accessible_label = format!("Start an independent branch after: {accessible_preview}");
    button.update_property(&[gtk::accessible::Property::Label(&accessible_label)]);
    button.set_tooltip_text(Some(
        "Copy history through this message into a new conversation",
    ));

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let marker = icons::icon(icons::Icon::UserRound, 16);
    marker.set_size_request(30, 30);
    marker.add_css_class("branch-picker-marker");
    content.append(&marker);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text.set_hexpand(true);
    let preview = gtk::Label::new(Some(message_preview(&candidate.text)));
    preview.set_xalign(0.0);
    preview.set_wrap(true);
    preview.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    preview.set_lines(3);
    preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
    preview.add_css_class("branch-picker-preview");
    let label = gtk::Label::new(Some("Copy history through this message"));
    label.set_xalign(0.0);
    label.add_css_class("branch-picker-message-label");
    text.append(&preview);
    text.append(&label);
    content.append(&text);
    let select_icon = icons::icon(icons::Icon::GitBranchPlus, 17);
    select_icon.add_css_class("branch-picker-select-icon");
    content.append(&select_icon);
    button.set_child(Some(&content));
    row.set_child(Some(&button));
    (row, button)
}

fn message_preview(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "(Empty user message)"
    } else {
        trimmed
    }
}

fn one_line_preview(text: &str) -> String {
    let preview = message_preview(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    const MAX_CHARS: usize = 120;
    if preview.chars().count() <= MAX_CHARS {
        return preview;
    }
    let mut shortened = preview.chars().take(MAX_CHARS - 1).collect::<String>();
    shortened.push('…');
    shortened
}

fn selection_for_entry(candidates: &[BranchMessage], entry_id: &str) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| candidate.entry_id == entry_id)
        .map(|candidate| candidate.entry_id.clone())
}

#[cfg(test)]
mod tests {
    use super::{one_line_preview, selection_for_entry};
    use crate::bridge::protocol::BranchMessage;

    #[test]
    fn selection_uses_entry_id_when_text_and_order_are_ambiguous() {
        let candidates = vec![
            BranchMessage {
                entry_id: "entry-newer".to_owned(),
                text: "Repeat this request".to_owned(),
            },
            BranchMessage {
                entry_id: "entry-older".to_owned(),
                text: "Repeat this request".to_owned(),
            },
        ];

        assert_eq!(
            selection_for_entry(&candidates, "entry-older").as_deref(),
            Some("entry-older")
        );
        assert_eq!(selection_for_entry(&candidates, "1"), None);
        assert_eq!(
            selection_for_entry(&candidates, "Repeat this request"),
            None
        );
    }

    #[test]
    fn accessible_preview_is_bounded_without_changing_selection_identity() {
        let preview = one_line_preview(&"word ".repeat(80));
        assert_eq!(preview.chars().count(), 120);
        assert!(preview.ends_with('…'));
    }
}
