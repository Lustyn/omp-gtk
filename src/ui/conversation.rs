use std::path::{Path, PathBuf};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;

#[cfg(feature = "ui-stories")]
use super::chat::ThinkingBlock;
use super::chat::{self, ChatHero, MessageBody, MessageRole};

#[derive(Clone)]
pub(crate) struct ConversationView {
    root: gtk::Overlay,
    items: gtk::Box,
    scroller: gtk::ScrolledWindow,
    hero: Option<ChatHero>,
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
        let hero = with_empty_state.then(|| {
            let hero = ChatHero::new();
            hero.root.set_can_target(false);
            hero.root.set_visible(false);
            root.add_overlay(&hero.root);
            hero
        });

        Self {
            root,
            items,
            scroller,
            hero,
        }
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn append_message(&self, role: MessageRole, text: &str) -> MessageBody {
        self.hide_empty();
        chat::append_message(&self.items, role, text)
    }

    pub(crate) fn append_streaming_message(&self, role: MessageRole, text: &str) -> MessageBody {
        self.hide_empty();
        chat::append_streaming_message(&self.items, role, text)
    }

    #[cfg(feature = "ui-stories")]
    pub(crate) fn append_thinking(&self, text: &str, streaming: bool) -> ThinkingBlock {
        self.hide_empty();
        chat::append_thinking(&self.items, text, streaming)
    }

    pub(crate) fn append_notice(&self, text: &str, error: bool) -> gtk::Box {
        self.hide_empty();
        chat::append_notice(&self.items, text, error)
    }

    pub(crate) fn append(&self, widget: &impl IsA<gtk::Widget>) {
        self.hide_empty();
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

    pub(crate) fn show_workspace_onboarding<F, G>(
        &self,
        recent_workspaces: &[PathBuf],
        current_workspace: Option<&Path>,
        on_select: F,
        on_browse: G,
    ) where
        F: Fn(PathBuf) + Clone + 'static,
        G: Fn() + 'static,
    {
        if let Some(hero) = &self.hero {
            hero.root.set_can_target(true);
            hero.show_workspace_onboarding(
                recent_workspaces,
                current_workspace,
                on_select,
                on_browse,
            );
            hero.root.set_visible(true);
        }
    }

    pub(crate) fn show_loading(&self, title: &str, detail: &str, activity: &str) {
        if let Some(hero) = &self.hero {
            hero.root.set_can_target(false);
            hero.show_loading(title, detail, activity);
            hero.root.set_visible(true);
        }
    }

    pub(crate) fn show_disconnected(&self, detail: &str) {
        if let Some(hero) = &self.hero {
            hero.root.set_can_target(false);
            hero.show_disconnected(detail);
            hero.root.set_visible(true);
        }
    }

    pub(crate) fn hide_empty(&self) {
        if let Some(hero) = &self.hero {
            hero.root.set_can_target(false);
            hero.hide();
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
