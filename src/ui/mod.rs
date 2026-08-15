pub(crate) mod agent_hub;
pub(crate) mod attachments;
pub(crate) mod chat;
pub(crate) mod composer;
pub(crate) mod conversation;
#[cfg(feature = "ui-stories")]
pub(crate) mod gallery;
pub(crate) mod icons;
pub(crate) mod model_picker;
pub(crate) mod sidebar;
pub(crate) mod sound_settings;
#[cfg(feature = "ui-stories")]
pub(crate) mod stories;
pub(crate) mod tool_components;
pub(crate) mod todos;
pub(crate) mod workspace;

use gtk::gdk;
use gtk4 as gtk;

pub(crate) fn initialize() {
    icons::initialize_lucide_font().expect("failed to load bundled Lucide icon font");

    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    let display = gdk::Display::default().expect("a graphical display");
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
