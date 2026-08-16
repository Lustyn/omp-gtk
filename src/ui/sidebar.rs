use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use libadwaita as adw;

use super::icons;
use crate::session_catalog::SessionEntry;

const INDICATOR_CYCLE_MICROS: i64 = 2_400_000;

fn indicator_gradient_phase(frame_time_micros: i64) -> f64 {
    frame_time_micros.rem_euclid(INDICATOR_CYCLE_MICROS) as f64 / INDICATOR_CYCLE_MICROS as f64
}

#[derive(Clone)]
pub struct SidebarWidgets {
    pub root: gtk::Box,
    pub list: gtk::ListBox,
    pub new_chat: gtk::Button,
    pub history: gtk::Button,
    pub collapse: gtk::Button,
    pub preferences: gtk::Button,
    pub active_count: gtk::Label,
    pub unread_count: gtk::Label,
}

#[derive(Clone)]
pub struct SessionRow {
    pub row: gtk::ListBoxRow,
    pub badge: gtk::Label,
    pub indicator: gtk::DrawingArea,
    pub open_action: gtk::Button,
    pub rename_action: gtk::Button,
    pub close_action: gtk::Button,
    pub delete_action: gtk::Button,
    pub entry: SessionEntry,
}

pub fn build() -> SidebarWidgets {
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.set_size_request(286, -1);
    sidebar.add_css_class("sidebar");

    let brand_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    brand_row.set_margin_top(12);
    brand_row.set_margin_bottom(10);
    brand_row.set_margin_start(14);
    brand_row.set_margin_end(10);
    brand_row.append(&icons::omp_logo(29));
    let brand_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    brand_spacer.set_hexpand(true);
    let session_counts = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    session_counts.add_css_class("sidebar-session-counts");
    let active_count = gtk::Label::new(Some("0 active"));
    active_count.add_css_class("sidebar-session-count");
    active_count.add_css_class("active");
    active_count.set_tooltip_text(Some("No active sessions"));
    let unread_count = gtk::Label::new(Some("0 unread"));
    unread_count.add_css_class("sidebar-session-count");
    unread_count.add_css_class("unread");
    unread_count.set_tooltip_text(Some("No unread sessions"));
    session_counts.append(&active_count);
    session_counts.append(&unread_count);
    let collapse = icons::icon_button(icons::Icon::PanelLeftClose, "Collapse sidebar");
    collapse.add_css_class("sidebar-toggle");
    brand_row.append(&brand_spacer);
    brand_row.append(&session_counts);
    brand_row.append(&collapse);
    let brand_handle = gtk::WindowHandle::new();
    brand_handle.set_child(Some(&brand_row));

    let new_chat = gtk::Button::new();
    let new_chat_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    new_chat_content.append(&icons::icon(icons::Icon::Plus, 16));
    let label = gtk::Label::new(Some("New conversation"));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    new_chat_content.append(&label);
    new_chat_content.append(&gtk::Label::new(Some("Ctrl+N")));
    new_chat.set_child(Some(&new_chat_content));
    new_chat.set_margin_start(12);
    new_chat.set_margin_end(12);
    new_chat.set_tooltip_text(Some("Start a new omp conversation"));
    new_chat.add_css_class("new-chat-button");

    let section_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    section_row.set_margin_top(20);
    section_row.set_margin_bottom(7);
    section_row.set_margin_start(16);
    section_row.set_margin_end(10);
    let section = gtk::Label::new(Some("RECENT"));
    section.set_xalign(0.0);
    section.set_hexpand(true);
    section.add_css_class("section-label");
    let history = icons::icon_button(icons::Icon::History, "Search conversation history");
    history.add_css_class("history-button");
    section_row.append(&section);
    section_row.append(&history);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("session-list");
    list.set_margin_start(8);
    list.set_margin_end(8);
    let empty = gtk::Label::new(Some("No open conversations"));
    empty.add_css_class("session-list-empty");
    list.set_placeholder(Some(&empty));
    let session_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    let preferences = icons::icon_button(icons::Icon::Settings, "Settings");
    preferences.set_halign(gtk::Align::End);
    preferences.set_margin_top(6);
    preferences.set_margin_bottom(10);
    preferences.set_margin_end(10);
    preferences.set_tooltip_text(Some("Settings"));
    preferences.add_css_class("sidebar-preferences");
    sidebar.append(&brand_handle);
    sidebar.append(&new_chat);
    sidebar.append(&section_row);
    sidebar.append(&session_scroll);
    sidebar.append(&preferences);

    SidebarWidgets {
        root: sidebar,
        list,
        new_chat,
        history,
        collapse,
        preferences,
        active_count,
        unread_count,
    }
}

pub fn session_row(entry: SessionEntry) -> SessionRow {
    let row = gtk::ListBoxRow::new();
    row.update_property(&[gtk::accessible::Property::Label(&entry.title)]);
    row.set_activatable(true);
    row.set_selectable(true);
    if entry.current {
        row.add_css_class("current-session");
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(9);
    content.set_margin_end(4);
    let indicator = session_indicator(&row, entry.current, entry.running);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(&entry.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("session-title");
    let subtitle = gtk::Label::new(Some(&entry.subtitle));
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    subtitle.add_css_class("session-subtitle");
    if let Some(cwd) = entry.cwd.as_deref() {
        subtitle.set_tooltip_text(Some(&cwd.to_string_lossy()));
    }
    text.append(&title);
    text.append(&subtitle);
    let badge = gtk::Label::new(None);
    badge.set_visible(false);
    badge.add_css_class("session-badge");
    let close_action = icons::icon_button(icons::Icon::X, "Close conversation");
    close_action.add_css_class("session-close");
    content.append(&indicator);
    content.append(&text);
    content.append(&badge);
    content.append(&close_action);
    row.set_child(Some(&content));

    let popover = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.add_css_class("context-menu");
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let open_action = context_button(icons::Icon::FolderOpen, "Open conversation");
    let rename_action = context_button(icons::Icon::Pencil, "Rename conversation");
    let close_menu_action = context_button(icons::Icon::X, "Close conversation");
    let copy_action = context_button(icons::Icon::Copy, "Copy title");
    let reveal_action = context_button(icons::Icon::Folder, "Reveal transcript");
    let delete_action = context_button(icons::Icon::Trash2, "Delete conversation");
    delete_action.add_css_class("destructive-action");
    menu.append(&open_action);
    menu.append(&rename_action);
    menu.append(&close_menu_action);
    menu.append(&copy_action);
    menu.append(&reveal_action);
    menu.append(&delete_action);
    popover.set_child(Some(&menu));
    popover.set_parent(&row);

    let close_from_menu = close_action.clone();
    let popover_for_close = popover.clone();
    close_menu_action.connect_clicked(move |_| {
        close_from_menu.emit_clicked();
        popover_for_close.popdown();
    });
    let title_for_copy = entry.title.clone();
    let popover_for_copy = popover.clone();
    copy_action.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(&title_for_copy);
        }
        popover_for_copy.popdown();
    });
    if let Some(path) = entry.path.clone() {
        reveal_action.connect_clicked(move |_| {
            let uri = gio::File::for_path(&path).uri();
            let _ = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE);
        });
    } else {
        reveal_action.set_sensitive(false);
        delete_action.set_sensitive(false);
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    let popover_for_click = popover.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        popover_for_click.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover_for_click.popup();
    });
    row.add_controller(gesture);
    row.connect_unrealize(move |_| popover.unparent());

    SessionRow {
        row,
        indicator,
        badge,
        open_action,
        rename_action,
        close_action,
        delete_action,
        entry,
    }
}

pub fn make_session_draggable(
    row: &gtk::ListBoxRow,
    runtime_id: u64,
    on_drop: impl Fn(u64, bool) + 'static,
) {
    let drag_source = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    drag_source.connect_prepare(move |_, _, _| {
        Some(gdk::ContentProvider::for_value(&runtime_id.to_value()))
    });
    row.add_controller(drag_source);

    let drop_target = gtk::DropTarget::new(u64::static_type(), gdk::DragAction::MOVE);
    let row_for_motion = row.clone();
    drop_target.connect_motion(move |_, _, y| {
        let insert_after = y >= f64::from(row_for_motion.height()) / 2.0;
        row_for_motion.remove_css_class(if insert_after {
            "session-drop-before"
        } else {
            "session-drop-after"
        });
        row_for_motion.add_css_class(if insert_after {
            "session-drop-after"
        } else {
            "session-drop-before"
        });
        gdk::DragAction::MOVE
    });
    let row_for_leave = row.clone();
    drop_target.connect_leave(move |_| {
        row_for_leave.remove_css_class("session-drop-before");
        row_for_leave.remove_css_class("session-drop-after");
    });
    let row_for_drop = row.clone();
    drop_target.connect_drop(move |_, value, _, y| {
        row_for_drop.remove_css_class("session-drop-before");
        row_for_drop.remove_css_class("session-drop-after");
        let Ok(source_runtime_id) = value.get::<u64>() else {
            return false;
        };
        if source_runtime_id == runtime_id {
            return false;
        }
        let insert_after = y >= f64::from(row_for_drop.height()) / 2.0;
        on_drop(source_runtime_id, insert_after);
        true
    });
    row.add_controller(drop_target);
}

fn session_indicator(row: &gtk::ListBoxRow, current: bool, running: bool) -> gtk::DrawingArea {
    // Use GTK's stable frame timeline so rebuilding a row does not restart its
    // gradient and make an unrelated render look like a jump.
    const PINK: (f64, f64, f64) = (
        0xed as f64 / 255.0,
        0x4a as f64 / 255.0,
        0xbf as f64 / 255.0,
    );
    const PURPLE: (f64, f64, f64) = (0x9b as f64 / 255.0, 0x4d as f64 / 255.0, 1.0);
    const CYAN: (f64, f64, f64) = (
        0x5a as f64 / 255.0,
        0xd8 as f64 / 255.0,
        0xe6 as f64 / 255.0,
    );
    const BLUE: (f64, f64, f64) = (
        0x79 as f64 / 255.0,
        0xa5 as f64 / 255.0,
        0xe3 as f64 / 255.0,
    );
    const IDLE: (f64, f64, f64) = (
        0x3a as f64 / 255.0,
        0x40 as f64 / 255.0,
        0x4d as f64 / 255.0,
    );
    let indicator = gtk::DrawingArea::new();
    indicator.set_content_width(3);
    indicator.add_css_class("session-indicator");
    let row = row.downgrade();

    indicator.set_draw_func(move |indicator, context, width, height| {
        if width <= 0 || height <= 0 {
            return;
        }
        let width = f64::from(width);
        let height = f64::from(height);
        let stroke_width = width.min(height);
        context.set_line_width(stroke_width);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        context.move_to(width / 2.0, stroke_width / 2.0);
        context.line_to(width / 2.0, height - stroke_width / 2.0);

        if running {
            let phase = indicator
                .frame_clock()
                .map_or(0.0, |clock| indicator_gradient_phase(clock.frame_time()));
            let start = -phase * height;
            let gradient = gtk::cairo::LinearGradient::new(0.0, start, 0.0, start + height);
            gradient.add_color_stop_rgb(0.0, PINK.0, PINK.1, PINK.2);
            gradient.add_color_stop_rgb(1.0 / 3.0, PURPLE.0, PURPLE.1, PURPLE.2);
            gradient.add_color_stop_rgb(2.0 / 3.0, CYAN.0, CYAN.1, CYAN.2);
            gradient.add_color_stop_rgb(1.0, PINK.0, PINK.1, PINK.2);
            gradient.set_extend(gtk::cairo::Extend::Repeat);
            let _ = context.set_source(&gradient);
        } else if current || row.upgrade().is_some_and(|row| row.is_selected()) {
            context.set_source_rgb(BLUE.0, BLUE.1, BLUE.2);
        } else if row
            .upgrade()
            .is_some_and(|row| row.has_css_class("unread-session"))
        {
            context.set_source_rgb(PURPLE.0, PURPLE.1, PURPLE.2);
        } else {
            context.set_source_rgb(IDLE.0, IDLE.1, IDLE.2);
        }
        let _ = context.stroke();
    });

    if running {
        indicator.add_tick_callback(|indicator, _| {
            indicator.queue_draw();
            glib::ControlFlow::Continue
        });
    }
    indicator
}

pub fn present_history(
    parent: &gtk::ApplicationWindow,
    sessions: Vec<SessionEntry>,
    active_paths: &HashSet<PathBuf>,
    on_open: impl Fn(SessionEntry) + 'static,
) {
    let sessions = sessions
        .into_iter()
        .filter(|entry| {
            entry
                .path
                .as_ref()
                .is_some_and(|path| !active_paths.contains(path))
        })
        .collect::<Vec<_>>();

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("history-picker");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("history-picker-header");
    let heading = gtk::Label::new(Some("Conversation history"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("history-picker-heading");
    let close = icons::icon_button(icons::Icon::X, "Close conversation history");
    close.add_css_class("history-picker-close");
    header.append(&heading);
    header.append(&close);
    root.append(&header);

    let search = gtk::SearchEntry::new();
    search.update_property(&[gtk::accessible::Property::Label(
        "Search conversation history",
    )]);
    search.set_placeholder_text(Some("Search titles and workspaces"));
    search.add_css_class("history-picker-search");
    root.append(&search);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("history-picker-list");
    let empty = gtk::Label::new(Some("No closed conversations match."));
    empty.add_css_class("history-picker-empty");
    list.set_placeholder(Some(&empty));

    let rows = sessions
        .into_iter()
        .map(|entry| {
            let row = gtk::ListBoxRow::new();
            row.set_selectable(false);
            let button = gtk::Button::new();
            button.add_css_class("history-session");
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            content.append(&icons::icon(icons::Icon::MessageSquare, 16));
            let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
            text.set_hexpand(true);
            let title = gtk::Label::new(Some(&entry.title));
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.add_css_class("history-session-title");
            let subtitle = gtk::Label::new(Some(&entry.subtitle));
            subtitle.set_xalign(0.0);
            subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
            subtitle.add_css_class("history-session-subtitle");
            text.append(&title);
            text.append(&subtitle);
            content.append(&text);
            content.append(&icons::icon(icons::Icon::ArrowUpRight, 14));
            button.set_child(Some(&content));
            row.set_child(Some(&button));
            list.append(&row);
            (row, button, entry)
        })
        .collect::<Vec<_>>();
    let rows = Rc::new(rows);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    scroll.add_css_class("history-picker-scroll");
    root.append(&scroll);

    let dialog = adw::Dialog::builder()
        .title("Conversation history")
        .content_width(680)
        .content_height(620)
        .child(&root)
        .build();
    let weak_dialog = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });

    let rows_for_search = rows.clone();
    search.connect_search_changed(move |search| {
        let query = search.text().trim().to_ascii_lowercase();
        for (row, _, entry) in rows_for_search.iter() {
            let cwd = entry
                .cwd
                .as_deref()
                .map(|path| path.to_string_lossy())
                .unwrap_or_default();
            let matches = query.is_empty()
                || entry.title.to_ascii_lowercase().contains(&query)
                || entry.subtitle.to_ascii_lowercase().contains(&query)
                || cwd.to_ascii_lowercase().contains(&query);
            row.set_visible(matches);
        }
    });

    let on_open = Rc::new(on_open);
    for (_, button, entry) in rows.iter() {
        let entry = entry.clone();
        let on_open = on_open.clone();
        let weak_dialog = dialog.downgrade();
        button.connect_clicked(move |_| {
            on_open(entry.clone());
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
    }
    dialog.present(Some(parent));
    search.grab_focus();
}

fn context_button(icon: icons::Icon, text: &str) -> gtk::Button {
    let button = icons::labeled_button(icon, text);
    button.add_css_class("context-action");
    button
}
#[cfg(test)]
mod tests {
    use super::{INDICATOR_CYCLE_MICROS, indicator_gradient_phase};

    #[test]
    fn indicator_gradient_phase_loops_on_the_frame_timeline() {
        let quarter_cycle = INDICATOR_CYCLE_MICROS / 4;

        assert_eq!(indicator_gradient_phase(quarter_cycle), 0.25);
        assert_eq!(
            indicator_gradient_phase(INDICATOR_CYCLE_MICROS + quarter_cycle),
            0.25
        );
    }
}
