use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use crate::alerts::{Preferences, SoundEvent, SoundPackChoice};
use crate::bridge::protocol::{InterruptMode, QueueMode};
use crate::sound_registry::RegistryPack;

use super::icons;

pub(crate) const MESSAGES_ICON_NAME: &str = "omp-messages-symbolic";
pub(crate) const SOUNDS_ICON_NAME: &str = "omp-sounds-symbolic";

#[derive(Clone)]
pub(crate) struct EventSoundRow {
    pub event: SoundEvent,
    pub row: adw::ComboRow,
    pub preview: gtk::Button,
    choices: Rc<RefCell<Vec<SoundPackChoice>>>,
    updating: Rc<Cell<bool>>,
}

impl EventSoundRow {
    fn new(
        event: SoundEvent,
        preferences: &Preferences,
        choices: Vec<SoundPackChoice>,
        sounds_enabled: bool,
    ) -> Self {
        let row = adw::ComboRow::builder()
            .title(event.title())
            .subtitle(event.description())
            .build();
        let preview = icons::icon_button(icons::Icon::Play, &format!("Preview {}", event.title()));
        preview.set_valign(gtk::Align::Center);
        row.add_suffix(&preview);
        let sound_row = Self {
            event,
            row,
            preview,
            choices: Rc::new(RefCell::new(Vec::new())),
            updating: Rc::new(Cell::new(false)),
        };
        sound_row.refresh(preferences, choices, sounds_enabled);
        sound_row
    }

    pub fn connect_changed(&self, callback: impl Fn(SoundEvent, Option<String>) + 'static) {
        let event = self.event;
        let choices = self.choices.clone();
        let updating = self.updating.clone();
        let preview = self.preview.clone();
        self.row.connect_selected_notify(move |row| {
            if updating.get() {
                return;
            }
            let selected = row.selected();
            preview.set_sensitive(selected > 0 && row.is_sensitive());
            let pack_id = selected.checked_sub(1).and_then(|index| {
                choices
                    .borrow()
                    .get(index as usize)
                    .map(|choice| choice.id.clone())
            });
            callback(event, pack_id);
        });
    }

    pub fn connect_preview(&self, callback: impl Fn(SoundEvent, String) + 'static) {
        let event = self.event;
        let row = self.row.clone();
        let choices = self.choices.clone();
        self.preview.connect_clicked(move |_| {
            let Some(index) = row.selected().checked_sub(1) else {
                return;
            };
            if let Some(choice) = choices.borrow().get(index as usize) {
                callback(event, choice.id.clone());
            }
        });
    }

    fn refresh(
        &self,
        preferences: &Preferences,
        choices: Vec<SoundPackChoice>,
        sounds_enabled: bool,
    ) {
        self.updating.set(true);
        let mut labels = Vec::with_capacity(choices.len() + 1);
        labels.push("Off");
        labels.extend(choices.iter().map(|choice| choice.label.as_str()));
        self.row.set_model(Some(&gtk::StringList::new(&labels)));
        let selected = preferences
            .pack_for(self.event)
            .and_then(|pack_id| choices.iter().position(|choice| choice.id == pack_id))
            .map_or(0, |index| index as u32 + 1);
        self.choices.replace(choices);
        self.row.set_selected(selected);
        self.row.set_sensitive(sounds_enabled);
        self.preview.set_sensitive(sounds_enabled && selected > 0);
        self.updating.set(false);
    }

    fn set_sounds_enabled(&self, enabled: bool) {
        self.row.set_sensitive(enabled);
        self.preview
            .set_sensitive(enabled && self.row.selected() > 0);
    }
}

#[derive(Clone)]
pub(crate) struct SettingsDialog {
    pub dialog: adw::PreferencesDialog,
    pub desktop_notifications: adw::SwitchRow,
    pub sounds: adw::SwitchRow,
    pub volume: gtk::Scale,
    pub event_rows: Rc<Vec<EventSoundRow>>,
    pub browse_packs: gtk::Button,
    steering_delivery: adw::ComboRow,
    follow_up_delivery: adw::ComboRow,
    interrupt_timing: adw::ComboRow,
    installed_count: gtk::Label,
}

impl SettingsDialog {
    pub fn new(
        preferences: &Preferences,
        choices: &HashMap<SoundEvent, Vec<SoundPackChoice>>,
        installed_count: usize,
    ) -> Self {
        let dialog = adw::PreferencesDialog::builder()
            .title("Settings")
            .content_width(620)
            .content_height(720)
            .build();
        let messages_page = adw::PreferencesPage::builder()
            .title("Messages")
            .icon_name(MESSAGES_ICON_NAME)
            .build();
        let delivery_group = adw::PreferencesGroup::builder()
            .title("Message delivery")
            .description(
                "Global defaults for every conversation, including conversations already open.",
            )
            .build();
        let steering_delivery = delivery_row(
            "Steering messages",
            "How messages sent with Enter are released while omp is responding",
            &["One at a time", "All at once"],
            queue_mode_selected(preferences.steering_mode),
        );
        let follow_up_delivery = delivery_row(
            "Follow-up messages",
            "How messages sent with Ctrl+Enter or Ctrl+Q are released after the active turn",
            &["One at a time", "All at once"],
            queue_mode_selected(preferences.follow_up_mode),
        );
        let interrupt_timing = delivery_row(
            "When steering",
            "Choose whether steering interrupts active tools immediately",
            &["Interrupt immediately", "Wait for the current turn"],
            match preferences.interrupt_mode {
                InterruptMode::Immediate => 0,
                InterruptMode::Wait => 1,
            },
        );
        delivery_group.add(&steering_delivery);
        delivery_group.add(&follow_up_delivery);
        delivery_group.add(&interrupt_timing);
        messages_page.add(&delivery_group);
        dialog.add(&messages_page);

        let page = adw::PreferencesPage::builder()
            .title("Sounds")
            .icon_name(SOUNDS_ICON_NAME)
            .build();

        let general = adw::PreferencesGroup::builder().title("General").build();
        let desktop_notifications = adw::SwitchRow::builder()
            .title("Desktop notifications")
            .subtitle("Show an alert when omp finishes in the background")
            .active(preferences.desktop_notifications)
            .build();
        let sounds = adw::SwitchRow::builder()
            .title("Sounds")
            .subtitle("Temporarily mute or restore every event sound")
            .active(preferences.sounds)
            .build();
        let volume_row = adw::ActionRow::builder()
            .title("Volume")
            .subtitle("Applies to event sounds and previews")
            .build();
        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 5.0);
        volume.set_value(preferences.volume * 100.0);
        volume.set_draw_value(true);
        volume.set_digits(0);
        volume.set_width_request(190);
        volume.set_valign(gtk::Align::Center);
        volume.set_sensitive(preferences.sounds);
        volume.update_property(&[gtk::accessible::Property::Label("Sound volume")]);
        volume_row.add_suffix(&volume);
        general.add(&desktop_notifications);
        general.add(&sounds);
        general.add(&volume_row);
        page.add(&general);

        let events = adw::PreferencesGroup::builder()
            .title("Event sounds")
            .description("Choose a pack for each event, or choose Off to silence only that event.")
            .build();
        let event_rows = SoundEvent::ALL
            .iter()
            .map(|event| {
                EventSoundRow::new(
                    *event,
                    preferences,
                    choices.get(event).cloned().unwrap_or_default(),
                    preferences.sounds,
                )
            })
            .collect::<Vec<_>>();
        for event_row in &event_rows {
            events.add(&event_row.row);
        }
        page.add(&events);

        let packs = adw::PreferencesGroup::builder()
            .title("Sound packs")
            .build();
        let pack_row = adw::ActionRow::builder()
            .title("Get more sound packs")
            .subtitle("Browse packs and install them without leaving omp")
            .activatable(true)
            .build();
        let installed_count = gtk::Label::new(Some(&installed_label(installed_count)));
        installed_count.add_css_class("dim-label");
        let browse_packs = gtk::Button::with_label("Browse");
        browse_packs.set_valign(gtk::Align::Center);
        browse_packs.add_css_class("suggested-action");
        pack_row.add_suffix(&installed_count);
        pack_row.add_suffix(&browse_packs);
        pack_row.set_activatable_widget(Some(&browse_packs));
        packs.add(&pack_row);
        page.add(&packs);

        dialog.add(&page);
        Self {
            dialog,
            desktop_notifications,
            sounds,
            volume,
            event_rows: Rc::new(event_rows),
            browse_packs,
            steering_delivery,
            follow_up_delivery,
            interrupt_timing,
            installed_count,
        }
    }

    pub fn connect_steering_mode_changed(&self, callback: impl Fn(QueueMode) + 'static) {
        self.steering_delivery
            .connect_selected_notify(move |row| callback(queue_mode_from_selected(row.selected())));
    }

    pub fn connect_follow_up_mode_changed(&self, callback: impl Fn(QueueMode) + 'static) {
        self.follow_up_delivery
            .connect_selected_notify(move |row| callback(queue_mode_from_selected(row.selected())));
    }

    pub fn connect_interrupt_mode_changed(&self, callback: impl Fn(InterruptMode) + 'static) {
        self.interrupt_timing.connect_selected_notify(move |row| {
            callback(match row.selected() {
                1 => InterruptMode::Wait,
                _ => InterruptMode::Immediate,
            });
        });
    }

    pub fn present(&self, parent: &gtk::ApplicationWindow) {
        self.dialog.present(Some(parent));
    }

    pub fn set_sounds_enabled(&self, enabled: bool) {
        self.volume.set_sensitive(enabled);
        for event_row in self.event_rows.iter() {
            event_row.set_sounds_enabled(enabled);
        }
    }

    pub fn refresh_packs(
        &self,
        preferences: &Preferences,
        choices: &HashMap<SoundEvent, Vec<SoundPackChoice>>,
        installed_count: usize,
    ) {
        for event_row in self.event_rows.iter() {
            event_row.refresh(
                preferences,
                choices.get(&event_row.event).cloned().unwrap_or_default(),
                preferences.sounds,
            );
        }
        self.installed_count
            .set_text(&installed_label(installed_count));
    }
}

fn delivery_row(title: &str, subtitle: &str, choices: &[&str], selected: u32) -> adw::ComboRow {
    adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .model(&gtk::StringList::new(choices))
        .selected(selected)
        .build()
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

fn installed_label(count: usize) -> String {
    match count {
        0 => "None installed".to_owned(),
        1 => "1 installed".to_owned(),
        count => format!("{count} installed"),
    }
}

#[derive(Clone)]
struct PackRow {
    root: adw::ActionRow,
    install: gtk::Button,
    search_text: String,
}

impl PackRow {
    fn new(pack: &RegistryPack, installed: bool) -> Self {
        let subtitle = if pack.description.trim().is_empty() {
            format!("{}\n{}", pack.source_label(), pack.metadata())
        } else {
            format!(
                "{}\n{} · {}",
                pack.description,
                pack.source_label(),
                pack.metadata()
            )
        };
        let display_name = gtk::glib::markup_escape_text(&pack.display_name);
        let subtitle = gtk::glib::markup_escape_text(&subtitle);
        let root = adw::ActionRow::builder()
            .title(display_name)
            .subtitle(subtitle)
            .subtitle_lines(3)
            .build();
        let icon_box = gtk::CenterBox::new();
        icon_box.set_size_request(36, 36);
        icon_box.set_valign(gtk::Align::Center);
        icon_box.add_css_class("sound-pack-icon");
        icon_box.set_center_widget(Some(&icons::icon(icons::Icon::AudioLines, 18)));
        root.add_prefix(&icon_box);
        let install = gtk::Button::with_label(if installed { "Installed" } else { "Install" });
        install.set_valign(gtk::Align::Center);
        install.set_sensitive(!installed);
        if !installed {
            install.add_css_class("suggested-action");
        }
        root.add_suffix(&install);
        let search_text = format!(
            "{} {} {} {} {} {}",
            pack.display_name,
            pack.description,
            pack.author.name,
            pack.language,
            pack.license,
            pack.categories.join(" ")
        )
        .to_ascii_lowercase();
        Self {
            root,
            install,
            search_text,
        }
    }

    fn set_installing(&self) {
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        let spinner = gtk::Spinner::new();
        spinner.start();
        content.append(&spinner);
        content.append(&gtk::Label::new(Some("Installing")));
        self.install.set_child(Some(&content));
        self.install.set_sensitive(false);
    }

    fn set_installed(&self) {
        self.install.set_child(None::<&gtk::Widget>);
        self.install.set_label("Installed");
        self.install.set_sensitive(false);
        self.install.remove_css_class("suggested-action");
        self.install.set_tooltip_text(None);
    }

    fn set_error(&self, error: &str) {
        self.install.set_child(None::<&gtk::Widget>);
        self.install.set_label("Retry");
        self.install.set_sensitive(true);
        self.install.set_tooltip_text(Some(error));
    }
}

#[derive(Clone)]
pub(crate) struct PackBrowserDialog {
    pub dialog: adw::PreferencesDialog,
    search: gtk::SearchEntry,
    stack: gtk::Stack,
    list: gtk::ListBox,
    loading: adw::StatusPage,
    error: adw::StatusPage,
    retry: gtk::Button,
    rows: Rc<RefCell<Vec<PackRow>>>,
}

impl PackBrowserDialog {
    pub fn new() -> Self {
        let dialog = adw::PreferencesDialog::builder()
            .title("Sound Packs")
            .content_width(760)
            .content_height(760)
            .build();
        let page = adw::PreferencesPage::new();
        let search_group = adw::PreferencesGroup::new();
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search by name, creator, language, or license"));
        search.update_property(&[gtk::accessible::Property::Label("Search sound packs")]);
        search_group.add(&search);
        page.add(&search_group);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("sound-pack-list");

        let loading = adw::StatusPage::builder()
            .title("Loading sound packs")
            .description("Fetching the latest catalog…")
            .build();
        let spinner = gtk::Spinner::new();
        spinner.start();
        spinner.set_size_request(32, 32);
        loading.set_child(Some(&spinner));

        let error = adw::StatusPage::builder()
            .title("Couldn’t load sound packs")
            .description("Check your connection and try again.")
            .build();
        let retry = gtk::Button::with_label("Try again");
        retry.add_css_class("pill");
        retry.add_css_class("suggested-action");
        retry.set_halign(gtk::Align::Center);
        error.set_child(Some(&retry));

        let empty = adw::StatusPage::builder()
            .title("No matching sound packs")
            .description("Try a different search.")
            .icon_name("edit-find-symbolic")
            .build();
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.add_named(&loading, Some("loading"));
        stack.add_named(&error, Some("error"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&list, Some("list"));
        stack.set_visible_child_name("loading");
        let results = adw::PreferencesGroup::new();
        results.add(&stack);
        page.add(&results);
        dialog.add(&page);

        let rows = Rc::new(RefCell::new(Vec::<PackRow>::new()));
        let rows_for_search = rows.clone();
        let stack_for_search = stack.clone();
        search.connect_search_changed(move |search| {
            let query = search.text().trim().to_ascii_lowercase();
            let mut visible = 0;
            for row in rows_for_search.borrow().iter() {
                let matches = query.is_empty() || row.search_text.contains(&query);
                row.root.set_visible(matches);
                visible += usize::from(matches);
            }
            stack_for_search.set_visible_child_name(if visible == 0 { "empty" } else { "list" });
        });

        Self {
            dialog,
            search,
            stack,
            list,
            loading,
            error,
            retry,
            rows,
        }
    }

    pub fn present(&self, parent: &gtk::ApplicationWindow) {
        self.dialog.present(Some(parent));
        self.search.grab_focus();
    }

    pub fn show_loading(&self) {
        self.search.set_sensitive(false);
        self.stack.set_visible_child(&self.loading);
    }

    pub fn show_error(&self, message: &str) {
        self.error.set_description(Some(message));
        self.search.set_sensitive(false);
        self.stack.set_visible_child(&self.error);
    }

    pub fn connect_retry(&self, callback: impl Fn() + 'static) {
        self.retry.connect_clicked(move |_| callback());
    }

    pub fn set_packs(
        &self,
        packs: &[RegistryPack],
        installed: &HashSet<String>,
        on_install: impl Fn(RegistryPack, gtk::Button) + Clone + 'static,
    ) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.rows.borrow_mut().clear();
        for pack in packs {
            let row = PackRow::new(pack, installed.contains(&pack.name));
            let requested = pack.clone();
            let button = row.install.clone();
            let callback = on_install.clone();
            row.install.connect_clicked(move |_| {
                callback(requested.clone(), button.clone());
            });
            self.list.append(&row.root);
            self.rows.borrow_mut().push(row);
        }
        self.search.set_sensitive(true);
        self.stack
            .set_visible_child_name(if packs.is_empty() { "empty" } else { "list" });
    }

    pub fn set_installing(button: &gtk::Button) {
        let row = PackRow {
            root: adw::ActionRow::new(),
            install: button.clone(),
            search_text: String::new(),
        };
        row.set_installing();
    }

    pub fn set_installed(button: &gtk::Button) {
        let row = PackRow {
            root: adw::ActionRow::new(),
            install: button.clone(),
            search_text: String::new(),
        };
        row.set_installed();
    }

    pub fn set_install_error(&self, button: &gtk::Button, error: &str) {
        let row = PackRow {
            root: adw::ActionRow::new(),
            install: button.clone(),
            search_text: String::new(),
        };
        row.set_error(error);
        self.dialog.add_toast(adw::Toast::new(error));
    }
}
