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

pub const APP_ID: &str = "dev.omp.Native";

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

pub(crate) fn initialize_gtk() {
    gtk::gio::resources_register_include!("omp-native.gresource")
        .expect("Failed to register application resources");
    gtk::init().expect("GTK initialization failed");

    let display = gtk::gdk::Display::default().expect("Failed to connect to a display");
    gtk::IconTheme::for_display(&display).add_resource_path("/dev/omp/Native/icons");
    gtk::Window::set_default_icon_name(APP_ID);

    if let Some(settings) = gtk::Settings::default() {
        settings.set_property("gtk-application-prefer-dark-theme", false);
    }

    ui::initialize();
}
