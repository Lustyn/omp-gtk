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
fn configure_macos_environment() {
    use std::path::{Path, PathBuf};

    let bundled_share = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent()?.parent().map(Path::to_path_buf))
        .map(|contents| contents.join("Resources/share"))
        .filter(|directory| directory.is_dir());

    if let Some(share) = &bundled_share {
        let mut data_dirs = vec![share.clone()];
        if let Some(existing) = std::env::var_os("XDG_DATA_DIRS") {
            data_dirs.extend(std::env::split_paths(&existing));
        }
        let data_dirs =
            std::env::join_paths(data_dirs).expect("macOS bundle data paths must be valid");
        // GTK has not started any threads yet, so changing the process environment is safe.
        unsafe {
            std::env::set_var("XDG_DATA_DIRS", data_dirs);
        }
    }

    let bundled_loaders = bundled_share
        .as_ref()
        .and_then(|share| share.parent())
        .map(|resources| resources.join("lib/gdk-pixbuf-2.0/2.10.0/loaders"))
        .filter(|directory| directory.is_dir());
    if let Some(loaders) = bundled_loaders {
        let cache_template = std::fs::read_to_string(loaders.join("loaders.cache.in"))
            .expect("failed to read bundled GdkPixbuf loader cache");
        let escaped_loader_path = loaders
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let cache = cache_template.replace("@GDK_PIXBUF_MODULEDIR@", &escaped_loader_path);
        let cache_dir =
            std::env::temp_dir().join(format!("omp-gtk-gdk-pixbuf-{}", env!("CARGO_PKG_VERSION")));
        std::fs::create_dir_all(&cache_dir)
            .expect("failed to create GdkPixbuf loader cache directory");
        let cache_file = cache_dir.join("loaders.cache");
        let cache_is_current = std::fs::read(&cache_file)
            .map(|existing| existing == cache.as_bytes())
            .unwrap_or(false);
        if !cache_is_current {
            std::fs::write(&cache_file, cache).expect("failed to stage GdkPixbuf loader cache");
        }

        // Prevent GdkPixbuf from loading Homebrew modules beside the bundled runtime.
        unsafe {
            std::env::set_var("GDK_PIXBUF_MODULEDIR", &loaders);
            std::env::set_var("GDK_PIXBUF_MODULE_FILE", &cache_file);
        }
        gdk_pixbuf::Pixbuf::init_modules(&cache_dir.to_string_lossy())
            .expect("failed to initialize bundled GdkPixbuf loaders");
    }

    if std::env::var_os("GSETTINGS_SCHEMA_DIR").is_some() {
        return;
    }

    let schema_dir = bundled_share
        .as_ref()
        .map(|share| share.join("glib-2.0/schemas"))
        .filter(|directory| directory.join("gschemas.compiled").is_file())
        .or_else(|| {
            std::env::var_os("HOMEBREW_PREFIX")
                .map(PathBuf::from)
                .map(|prefix| prefix.join("share/glib-2.0/schemas"))
                .filter(|directory| directory.join("gschemas.compiled").is_file())
        })
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
    configure_macos_environment();

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
