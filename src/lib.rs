mod agent_hub;
mod alerts;
mod app;
mod bridge;
mod commands;
mod session_catalog;
mod sound_registry;
pub mod ui;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

pub const APP_ID: &str = "dev.omp.Gtk";

pub fn run() -> glib::ExitCode {
    initialize_gtk();

    let application = adw::Application::builder().application_id(APP_ID).build();
    application.connect_activate(app::build);
    application.run()
}

#[cfg(feature = "ui-stories")]
pub fn run_ui_gallery() -> glib::ExitCode {
    ui::gallery::run()
}

#[cfg(target_os = "macos")]
fn configure_macos_gsettings() {
    use std::path::{Path, PathBuf};

    if std::env::var_os("GSETTINGS_SCHEMA_DIR").is_some() {
        return;
    }

    let schema_dir = std::env::var_os("HOMEBREW_PREFIX")
        .map(PathBuf::from)
        .map(|prefix| prefix.join("share/glib-2.0/schemas"))
        .filter(|directory| directory.join("gschemas.compiled").is_file())
        .or_else(|| {
            ["/opt/homebrew", "/usr/local"]
                .into_iter()
                .map(Path::new)
                .map(|prefix| prefix.join("share/glib-2.0/schemas"))
                .find(|directory| directory.join("gschemas.compiled").is_file())
        });

    if let Some(schema_dir) = schema_dir {
        // GTK has not started any threads yet, so changing the process environment is safe.
        unsafe {
            std::env::set_var("GSETTINGS_SCHEMA_DIR", schema_dir);
        }
    }
}

pub(crate) fn initialize_gtk() {
    #[cfg(target_os = "macos")]
    configure_macos_gsettings();

    gtk::gio::resources_register_include!("omp-gtk.gresource")
        .expect("Failed to register application resources");
    #[cfg(target_os = "macos")]
    ui::icons::initialize_lucide_font().expect("failed to load bundled Lucide icon font");

    gtk::init().expect("GTK initialization failed");
    #[cfg(not(target_os = "macos"))]
    ui::icons::initialize_lucide_font().expect("failed to load bundled Lucide icon font");
    ui::icons::verify_lucide_font().expect("failed to resolve bundled Lucide icon font");

    let display = gtk::gdk::Display::default().expect("Failed to connect to a display");
    gtk::IconTheme::for_display(&display).add_resource_path("/dev/omp/Gtk/icons");
    gtk::Window::set_default_icon_name(APP_ID);

    if let Some(settings) = gtk::Settings::default() {
        settings.set_property("gtk-application-prefer-dark-theme", false);
    }

    ui::initialize();
}
