mod bridge;
mod commands;
mod ui;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

const APP_ID: &str = "dev.omp.Native";

fn main() -> glib::ExitCode {
    gtk::gio::resources_register_include!("omp-native.gresource")
        .expect("Failed to register application resources");

    gtk::init().expect("GTK initialization failed");
    let display = gtk::gdk::Display::default().expect("Failed to connect to a display");
    gtk::IconTheme::for_display(&display).add_resource_path("/dev/omp/Native/icons");
    gtk::Window::set_default_icon_name(APP_ID);

    if let Some(settings) = gtk::Settings::default() {
        settings.set_property("gtk-application-prefer-dark-theme", false);
    }

    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_activate(ui::build);
    app.run()
}
