use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;

#[cfg(feature = "ui-stories")]
use super::chat::ThinkingBlock;
use super::chat::{self, ChatHero, MarkdownHeading, MessageBody, MessageRole};

#[derive(Clone)]
pub(crate) struct ConversationView {
    root: gtk::Overlay,
    items: gtk::Box,
    scroller: gtk::ScrolledWindow,
    hero: Option<ChatHero>,
    outline: Rc<ConversationOutline>,
}

struct ConversationOutline {
    button: gtk::MenuButton,
    list: gtk::Box,
    scroller: glib::WeakRef<gtk::ScrolledWindow>,
    items: glib::WeakRef<gtk::Box>,
    messages: RefCell<Vec<MessageBody>>,
}

impl ConversationOutline {
    fn new(items: &gtk::Box, scroller: &gtk::ScrolledWindow) -> Rc<Self> {
        let button = gtk::MenuButton::new();
        button.set_child(Some(&super::icons::icon(
            super::icons::Icon::TableOfContents,
            16,
        )));
        button.set_tooltip_text(Some("Outline for the message in view"));
        button.update_property(&[gtk::accessible::Property::Label("Message outline")]);
        button.set_halign(gtk::Align::End);
        button.set_valign(gtk::Align::Start);
        button.set_margin_top(12);
        button.set_margin_end(12);
        button.add_css_class("message-outline");
        button.set_visible(false);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        list.add_css_class("message-outline-list");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_max_content_height(420);
        scroll.set_min_content_width(280);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&list));
        let popover = gtk::Popover::new();
        popover.set_autohide(true);
        popover.add_css_class("message-outline-popover");
        popover.set_child(Some(&scroll));
        button.set_popover(Some(&popover));

        let outline = Rc::new(Self {
            button,
            list,
            scroller: scroller.downgrade(),
            items: items.downgrade(),
            messages: RefCell::new(Vec::new()),
        });
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&outline);
        motion.connect_enter(move |_, _, _| {
            if let Some(outline) = weak.upgrade()
                && outline.button.is_visible()
            {
                outline.button.popup();
            }
        });
        outline.button.add_controller(motion);
        outline
    }

    fn track(self: &Rc<Self>, body: MessageBody) {
        self.messages.borrow_mut().push(body.clone());
        let weak = Rc::downgrade(self);
        body.connect_headings_changed(move || {
            if let Some(outline) = weak.upgrade() {
                outline.schedule_refresh();
            }
        });
        self.refresh();
    }

    fn clear(&self) {
        self.messages.borrow_mut().clear();
        self.button.set_visible(false);
        self.button.popdown();
    }

    fn schedule_refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(outline) = weak.upgrade() {
                outline.refresh();
            }
        });
    }

    fn refresh(&self) {
        let (Some(scroller), Some(items)) = (self.scroller.upgrade(), self.items.upgrade()) else {
            return;
        };
        let adjustment = scroller.vadjustment();
        let viewport_start = adjustment.value();
        let viewport_end = viewport_start + adjustment.page_size();
        let active = self
            .messages
            .borrow()
            .iter()
            .filter_map(|body| {
                let headings = body.outline_headings();
                if headings.is_empty() {
                    return None;
                }
                let overlap = body
                    .row()
                    .compute_bounds(&items)
                    .map(|bounds| {
                        let y = f64::from(bounds.y());
                        let height = f64::from(bounds.height());
                        ((y + height).min(viewport_end) - y.max(viewport_start)).max(0.0)
                    })
                    .unwrap_or_default();
                Some((overlap, body.clone(), headings))
            })
            .max_by(|left, right| left.0.total_cmp(&right.0));

        let Some((_, body, headings)) = active else {
            self.button.set_visible(false);
            self.button.popdown();
            return;
        };
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let title = gtk::Label::new(Some("On this message"));
        title.set_xalign(0.0);
        title.add_css_class("message-outline-title");
        self.list.append(&title);
        let minimum_level = headings
            .iter()
            .map(|heading| heading.level)
            .min()
            .unwrap_or(1);
        for heading in headings {
            self.append_heading(&body, &heading, minimum_level);
        }
        self.button.set_visible(true);
    }

    fn append_heading(&self, body: &MessageBody, heading: &MarkdownHeading, minimum_level: u8) {
        let button = gtk::Button::new();
        button.set_halign(gtk::Align::Fill);
        button.add_css_class("message-outline-entry");
        let label = gtk::Label::new(Some(&heading.title));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_margin_start(i32::from(heading.level.saturating_sub(minimum_level)) * 14);
        button.set_child(Some(&label));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Jump to {}",
            heading.title
        ))]);
        let body = body.clone();
        let heading = heading.clone();
        let scroller = self.scroller.clone();
        let menu = self.button.clone();
        button.connect_clicked(move |_| {
            if let Some(scroller) = scroller.upgrade() {
                body.scroll_to_heading(&heading, &scroller);
            }
            menu.popdown();
        });
        self.list.append(&button);
    }
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
        let outline = ConversationOutline::new(&items, &scroller);
        root.add_overlay(&outline.button);
        let weak_outline = Rc::downgrade(&outline);
        scroller.vadjustment().connect_value_changed(move |_| {
            if let Some(outline) = weak_outline.upgrade() {
                outline.refresh();
            }
        });
        let weak_outline = Rc::downgrade(&outline);
        scroller.vadjustment().connect_page_size_notify(move |_| {
            if let Some(outline) = weak_outline.upgrade() {
                outline.refresh();
            }
        });
        let weak_outline = Rc::downgrade(&outline);
        scroller.vadjustment().connect_upper_notify(move |_| {
            if let Some(outline) = weak_outline.upgrade() {
                outline.schedule_refresh();
            }
        });
        let weak_outline = Rc::downgrade(&outline);
        scroller.connect_map(move |_| {
            if let Some(outline) = weak_outline.upgrade() {
                outline.schedule_refresh();
            }
        });
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
            outline,
        }
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn append_message(&self, role: MessageRole, text: &str) -> MessageBody {
        self.hide_empty();
        let body = chat::append_message(&self.items, role, text);
        if matches!(role, MessageRole::Assistant) {
            self.outline.track(body.clone());
        }
        body
    }

    pub(crate) fn append_streaming_message(&self, role: MessageRole, text: &str) -> MessageBody {
        self.hide_empty();
        let body = chat::append_streaming_message(&self.items, role, text);
        if matches!(role, MessageRole::Assistant) {
            self.outline.track(body.clone());
        }
        body
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
        self.outline.clear();
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
