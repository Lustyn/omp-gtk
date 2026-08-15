use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;

use super::chat::{self, MessageBody, MessageRole, ThinkingBlock};

#[derive(Clone)]
pub(crate) struct ConversationView {
    root: gtk::Overlay,
    items: gtk::Box,
    scroller: gtk::ScrolledWindow,
    empty_state: Option<gtk::Box>,
}

impl ConversationView {
    pub(crate) fn main() -> Self {
        Self::new(24, 32, 28, true)
    }

    pub(crate) fn transcript() -> Self {
        Self::new(20, 28, 28, false)
    }

    fn new(spacing: i32, margin_top: i32, margin_bottom: i32, with_empty_state: bool) -> Self {
        let items = gtk::Box::new(gtk::Orientation::Vertical, spacing);
        items.set_margin_top(margin_top);
        items.set_margin_bottom(margin_bottom);
        items.set_margin_start(22);
        items.set_margin_end(22);

        let clamp = adw::Clamp::builder()
            .maximum_size(900)
            .tightening_threshold(720)
            .child(&items)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&clamp)
            .build();
        scroller.add_css_class("message-scroll");

        let root = gtk::Overlay::new();
        root.set_child(Some(&scroller));
        let empty_state = with_empty_state.then(|| {
            let empty = chat::empty_chat_hero();
            empty.set_can_target(false);
            empty.set_visible(false);
            root.add_overlay(&empty);
            empty
        });

        Self {
            root,
            items,
            scroller,
            empty_state,
        }
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn append_message(&self, role: MessageRole, text: &str) -> MessageBody {
        chat::append_message(&self.items, role, text)
    }

    pub(crate) fn append_thinking(&self, text: &str, streaming: bool) -> ThinkingBlock {
        chat::append_thinking(&self.items, text, streaming)
    }

    pub(crate) fn append_notice(&self, text: &str, error: bool) -> gtk::Box {
        chat::append_notice(&self.items, text, error)
    }

    pub(crate) fn append(&self, widget: &impl IsA<gtk::Widget>) {
        self.items.append(widget);
    }

    pub(crate) fn clear(&self) {
        while let Some(child) = self.items.first_child() {
            self.items.remove(&child);
        }
        self.hide_empty();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.first_child().is_none()
    }

    pub(crate) fn show_empty(&self) {
        if let Some(empty) = &self.empty_state {
            empty.set_visible(true);
        }
    }

    pub(crate) fn hide_empty(&self) {
        if let Some(empty) = &self.empty_state {
            empty.set_visible(false);
        }
    }

    pub(crate) fn scroll_to_bottom(&self) {
        let adjustment = self.scroller.vadjustment();
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
}
