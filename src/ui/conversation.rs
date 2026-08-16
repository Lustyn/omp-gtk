use std::cell::{Cell, RefCell};
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

const OUTLINE_VIEWPORT_LEAD: f64 = 64.0;

fn active_heading_index(heading_positions: &[f64], viewport_marker: f64) -> usize {
    heading_positions
        .iter()
        .rposition(|position| *position <= viewport_marker)
        .unwrap_or(0)
}

struct ConversationOutline {
    root: gtk::Box,
    revealer: gtk::Revealer,
    list: gtk::Box,
    list_scroll: gtk::ScrolledWindow,
    rail: gtk::ToggleButton,
    progress: gtk::ProgressBar,
    count: gtk::Label,
    scroller: glib::WeakRef<gtk::ScrolledWindow>,
    items: glib::WeakRef<gtk::Box>,
    messages: RefCell<Vec<MessageBody>>,
    active_message: Cell<Option<usize>>,
    active_heading: Cell<usize>,
    heading_buttons: RefCell<Vec<gtk::Button>>,
    rebuild_pending: Cell<bool>,
}

impl ConversationOutline {
    fn new(items: &gtk::Box, scroller: &gtk::ScrolledWindow) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        // The workspace's todo and agent rails own the right overlay edge.
        // Keeping this compact rail on the left prevents competing hover cards.
        root.set_halign(gtk::Align::Start);
        root.set_valign(gtk::Align::Center);
        root.set_margin_start(0);
        root.set_visible(false);
        root.add_css_class("message-outline-surface");

        let progress = gtk::ProgressBar::new();
        progress.set_orientation(gtk::Orientation::Vertical);
        progress.set_inverted(false);
        progress.set_valign(gtk::Align::Fill);
        progress.set_vexpand(true);
        progress.add_css_class("message-outline-rail-progress");
        let count = gtk::Label::new(None);
        count.add_css_class("message-outline-rail-count");
        let rail_content = gtk::Box::new(gtk::Orientation::Vertical, 5);
        rail_content.append(&super::icons::icon(super::icons::Icon::TableOfContents, 13));
        rail_content.append(&progress);
        rail_content.append(&count);

        let rail = gtk::ToggleButton::new();
        rail.set_child(Some(&rail_content));
        rail.set_tooltip_text(Some("Hover to show nearby headings"));
        rail.add_css_class("message-outline-rail");
        root.append(&rail);

        let pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
        pane.set_size_request(320, -1);
        pane.add_css_class("message-outline-pane");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        header.add_css_class("message-outline-header");
        header.append(&super::icons::icon(super::icons::Icon::TableOfContents, 14));
        let title = gtk::Label::new(Some("On this message"));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.add_css_class("message-outline-title");
        header.append(&title);

        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        list.add_css_class("message-outline-list");
        let list_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .max_content_height(300)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        list_scroll.add_css_class("message-outline-scroll");

        pane.append(&header);
        pane.append(&list_scroll);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
        revealer.set_transition_duration(180);
        revealer.set_child(Some(&pane));
        root.append(&revealer);

        let pinned = Rc::new(Cell::new(false));
        let revealer_for_toggle = revealer.clone();
        let pinned_for_toggle = pinned.clone();
        rail.connect_toggled(move |button| {
            pinned_for_toggle.set(button.is_active());
            revealer_for_toggle.set_reveal_child(button.is_active());
        });
        let hovered = Rc::new(Cell::new(false));
        let focused = Rc::new(Cell::new(false));
        let motion = gtk::EventControllerMotion::new();
        let hovered_for_enter = hovered.clone();
        let revealer_for_enter = revealer.clone();
        motion.connect_enter(move |_, _, _| {
            hovered_for_enter.set(true);
            revealer_for_enter.set_reveal_child(true);
        });
        let hovered_for_leave = hovered.clone();
        let focused_for_leave = focused.clone();
        let pinned_for_leave = pinned.clone();
        let revealer_for_leave = revealer.clone();
        motion.connect_leave(move |_| {
            hovered_for_leave.set(false);
            if !focused_for_leave.get() && !pinned_for_leave.get() {
                revealer_for_leave.set_reveal_child(false);
            }
        });
        root.add_controller(motion);

        let focus = gtk::EventControllerFocus::new();
        let focused_for_enter = focused.clone();
        let revealer_for_focus_enter = revealer.clone();
        focus.connect_enter(move |_| {
            focused_for_enter.set(true);
            revealer_for_focus_enter.set_reveal_child(true);
        });
        let focused_for_leave = focused;
        let hovered_for_focus_leave = hovered;
        let pinned_for_focus_leave = pinned;
        let revealer_for_focus_leave = revealer.clone();
        focus.connect_leave(move |_| {
            focused_for_leave.set(false);
            if !hovered_for_focus_leave.get() && !pinned_for_focus_leave.get() {
                revealer_for_focus_leave.set_reveal_child(false);
            }
        });
        root.add_controller(focus);

        let outline = Rc::new(Self {
            root,
            revealer,
            list,
            list_scroll,
            rail,
            progress,
            count,
            scroller: scroller.downgrade(),
            items: items.downgrade(),
            messages: RefCell::new(Vec::new()),
            active_message: Cell::new(None),
            active_heading: Cell::new(0),
            heading_buttons: RefCell::new(Vec::new()),
            rebuild_pending: Cell::new(true),
        });
        let weak = Rc::downgrade(&outline);
        outline
            .revealer
            .connect_child_revealed_notify(move |revealer| {
                if revealer.is_child_revealed()
                    && let Some(outline) = weak.upgrade()
                {
                    outline.reveal_heading(outline.active_heading.get());
                }
            });
        outline
    }

    fn track(self: &Rc<Self>, body: MessageBody) {
        self.messages.borrow_mut().push(body.clone());
        self.rebuild_pending.set(true);
        let weak = Rc::downgrade(self);
        body.connect_headings_changed(move || {
            if let Some(outline) = weak.upgrade() {
                outline.rebuild_pending.set(true);
                outline.schedule_refresh();
            }
        });
        self.schedule_refresh();
    }

    fn clear(&self) {
        self.messages.borrow_mut().clear();
        self.active_message.set(None);
        self.active_heading.set(0);
        self.heading_buttons.borrow_mut().clear();
        self.rail.set_active(false);
        self.revealer.set_reveal_child(false);
        self.root.set_visible(false);
        self.rebuild_pending.set(true);
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
            .enumerate()
            .filter_map(|(message_index, body)| {
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
                (overlap > 0.0).then_some((overlap, message_index, body.clone(), headings))
            })
            .max_by(|left, right| left.0.total_cmp(&right.0));

        let Some((_, message_index, body, headings)) = active else {
            self.active_message.set(None);
            self.rail.set_active(false);
            self.revealer.set_reveal_child(false);
            self.root.set_visible(false);
            return;
        };

        let rebuild =
            self.active_message.get() != Some(message_index) || self.rebuild_pending.replace(false);
        if rebuild {
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            self.heading_buttons.borrow_mut().clear();
            let minimum_level = headings
                .iter()
                .map(|heading| heading.level)
                .min()
                .unwrap_or(1);
            for heading in &headings {
                let button = self.append_heading(&body, heading, minimum_level);
                self.list.append(&button);
                self.heading_buttons.borrow_mut().push(button);
            }
            self.active_message.set(Some(message_index));
        }

        let positions = headings
            .iter()
            .map(|heading| body.heading_y_in(heading, &items).unwrap_or(f64::INFINITY))
            .collect::<Vec<_>>();
        let heading_index =
            active_heading_index(&positions, viewport_start + OUTLINE_VIEWPORT_LEAD);
        self.update_heading_progress(heading_index, &headings, rebuild);
        self.root.set_visible(true);
    }

    fn update_heading_progress(
        &self,
        heading_index: usize,
        headings: &[MarkdownHeading],
        force_reveal: bool,
    ) {
        let heading_index = heading_index.min(headings.len().saturating_sub(1));
        let changed = self.active_heading.replace(heading_index) != heading_index;
        let fraction = if headings.is_empty() {
            0.0
        } else {
            (heading_index + 1) as f64 / headings.len() as f64
        };
        self.progress.set_fraction(fraction);
        self.count
            .set_label(&format!("{}/{}", heading_index + 1, headings.len()));
        let title = headings
            .get(heading_index)
            .map(|heading| heading.title.as_str())
            .unwrap_or("Unknown heading");
        self.rail
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Message outline, heading {} of {}: {title}",
                heading_index + 1,
                headings.len()
            ))]);
        for (index, button) in self.heading_buttons.borrow().iter().enumerate() {
            if index == heading_index {
                button.add_css_class("current");
            } else {
                button.remove_css_class("current");
            }
        }
        if changed || force_reveal {
            self.reveal_heading(heading_index);
        }
    }

    fn reveal_heading(&self, heading_index: usize) {
        let Some(button) = self.heading_buttons.borrow().get(heading_index).cloned() else {
            return;
        };
        let list = self.list.clone();
        let scroll = self.list_scroll.clone();
        glib::idle_add_local_once(move || {
            let Some(bounds) = button.compute_bounds(&list) else {
                return;
            };
            let adjustment = scroll.vadjustment();
            let top = f64::from(bounds.y());
            let bottom = top + f64::from(bounds.height());
            let viewport_top = adjustment.value();
            let viewport_bottom = viewport_top + adjustment.page_size();
            let target = if top < viewport_top {
                top
            } else if bottom > viewport_bottom {
                bottom - adjustment.page_size()
            } else {
                viewport_top
            };
            let limit = (adjustment.upper() - adjustment.page_size()).max(0.0);
            adjustment.set_value(target.clamp(0.0, limit));
        });
    }

    fn append_heading(
        &self,
        body: &MessageBody,
        heading: &MarkdownHeading,
        minimum_level: u8,
    ) -> gtk::Button {
        let button = gtk::Button::new();
        button.set_halign(gtk::Align::Fill);
        button.add_css_class("message-outline-entry");
        let label = gtk::Label::new(Some(&heading.title));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_max_width_chars(36);
        label.set_margin_start(i32::from(heading.level.saturating_sub(minimum_level)) * 12);
        button.set_child(Some(&label));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Jump to {}",
            heading.title
        ))]);
        let body = body.clone();
        let heading = heading.clone();
        let scroller = self.scroller.clone();
        button.connect_clicked(move |_| {
            if let Some(scroller) = scroller.upgrade() {
                body.scroll_to_heading(&heading, &scroller);
            }
        });
        button
    }
}

impl ConversationView {
    pub(crate) fn main() -> Self {
        Self::new(24, 32, 28, true)
    }

    pub(crate) fn transcript() -> Self {
        Self::new(20, 28, 28, false)
    }

    #[cfg(feature = "ui-stories")]
    pub(crate) fn set_outline_revealed(&self, revealed: bool) {
        self.outline.rail.set_active(revealed);
        let outline_for_scroll = self.outline.clone();
        self.scroller
            .vadjustment()
            .connect_value_changed(move |_| outline_for_scroll.refresh());
        let outline = self.outline.clone();
        glib::idle_add_local_once(move || outline.refresh());
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
            .hexpand(true)
            .vexpand(true)
            .child(&clamp)
            .build();
        scroller.add_css_class("message-scroll");

        let outline = ConversationOutline::new(&items, &scroller);
        let root = gtk::Overlay::new();
        root.set_child(Some(&scroller));
        root.add_overlay(&outline.root);
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

    pub(crate) fn remove_message(&self, body: &MessageBody) {
        if body.row().parent().as_ref() == Some(self.items.upcast_ref::<gtk::Widget>()) {
            self.items.remove(body.row());
        }
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

#[cfg(test)]
mod tests {
    use super::active_heading_index;

    #[test]
    fn heading_tracker_starts_at_the_first_heading_before_it_reaches_the_viewport() {
        assert_eq!(active_heading_index(&[120.0, 240.0, 360.0], 40.0), 0);
    }

    #[test]
    fn heading_tracker_uses_the_last_heading_above_the_viewport_marker() {
        let positions = [40.0, 180.0, 320.0, 460.0];

        assert_eq!(active_heading_index(&positions, 319.0), 1);
        assert_eq!(active_heading_index(&positions, 900.0), 3);
    }
}
