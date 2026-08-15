use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;

use super::stories::{self, Story};

const GALLERY_APP_ID: &str = "dev.omp.Native.UiGallery";

#[derive(Clone)]
enum Mode {
    Browse,
    Story(String),
    List,
    Help,
}

#[derive(Clone)]
struct Options {
    mode: Mode,
    snapshot: Option<PathBuf>,
}

pub(crate) fn run() -> glib::ExitCode {
    let options = match parse_options(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            return glib::ExitCode::FAILURE;
        }
    };

    match &options.mode {
        Mode::List => {
            for story in stories::all() {
                println!(
                    "{}\t{}\t{}x{}",
                    story.id, story.title, story.width, story.height
                );
            }
            return glib::ExitCode::SUCCESS;
        }
        Mode::Help => {
            print_usage();
            return glib::ExitCode::SUCCESS;
        }
        Mode::Browse | Mode::Story(_) => {}
    }

    let selected = match &options.mode {
        Mode::Story(id) => match stories::find(id) {
            Some(story) => Some(story),
            None => {
                eprintln!("Unknown story {id:?}. Use --list to inspect available stories.");
                return glib::ExitCode::FAILURE;
            }
        },
        Mode::Browse => None,
        Mode::List | Mode::Help => unreachable!(),
    };
    if options.snapshot.is_some() && selected.is_none() {
        eprintln!("--snapshot requires --story <id>");
        return glib::ExitCode::FAILURE;
    }

    crate::initialize_gtk();
    glib::set_application_name("omp-native-ui-gallery");
    let application = adw::Application::builder()
        .application_id(GALLERY_APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let snapshot = options.snapshot;
    application.connect_activate(move |app| match selected {
        Some(story) => present_direct_story(app, story, snapshot.clone()),
        None => present_browser(app),
    });
    application.run_with_args(&["ui-gallery"])
}

fn parse_options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut args = args.peekable();
    let mut mode = Mode::Browse;
    let mut snapshot = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--list" => mode = Mode::List,
            "--help" | "-h" => mode = Mode::Help,
            "--story" => {
                let id = args
                    .next()
                    .ok_or_else(|| "--story requires a story id".to_owned())?;
                mode = Mode::Story(id);
            }
            "--snapshot" => {
                snapshot =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "--snapshot requires a PNG path".to_owned()
                    })?));
            }
            unknown => return Err(format!("Unknown argument {unknown:?}")),
        }
    }
    Ok(Options { mode, snapshot })
}

fn print_usage() {
    println!(
        "Usage: ui-gallery [--list | --story <id> [--snapshot <path>]]\n\n  --list           List story ids and viewports\n  --story ID       Open one story directly\n  --snapshot PATH  Render the selected story to PNG and exit"
    );
}

fn present_direct_story(app: &adw::Application, story: Story, snapshot_path: Option<PathBuf>) {
    let subject = (story.build)();
    let content = story_canvas(story, &subject);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(format!("omp UI Gallery — {}", story.id))
        .default_width(story.width.max(360))
        .default_height(story.height.max(180))
        .content(&content)
        .build();
    window.add_css_class("app-window");
    let id = story.id;
    let app = app.clone();
    window.connect_map(move |window| {
        println!("UI_STORY_READY {id}");
        let Some(path) = snapshot_path.clone() else {
            return;
        };
        let window = window.clone();
        let subject = subject.clone();
        let app = app.clone();
        glib::timeout_add_local_once(Duration::from_millis(100), move || {
            snapshot_widget(&window, &subject, &path)
                .unwrap_or_else(|error| panic!("Failed to snapshot {id}: {error}"));
            println!("UI_STORY_SNAPSHOT {}", path.display());
            app.quit();
        });
    });
    window.present();
}

fn present_browser(app: &adw::Application) {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("story-browser");

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("story-list");
    for story in stories::all() {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(story.title));
        label.set_xalign(0.0);
        label.set_margin_top(9);
        label.set_margin_bottom(9);
        label.set_margin_start(12);
        label.set_margin_end(12);
        row.set_tooltip_text(Some(story.id));
        row.set_child(Some(&label));
        list.append(&row);
    }
    let sidebar = gtk::ScrolledWindow::builder()
        .width_request(260)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    sidebar.add_css_class("story-sidebar");

    let canvas = gtk::Box::new(gtk::Orientation::Vertical, 0);
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);
    let stories = stories::all();
    list.connect_row_activated({
        let canvas = canvas.clone();
        move |_, row| {
            if let Some(story) = stories.get(row.index() as usize).copied() {
                set_story(&canvas, story);
            }
        }
    });
    if let Some(story) = stories.first().copied() {
        set_story(&canvas, story);
        if let Some(row) = list.row_at_index(0) {
            list.select_row(Some(&row));
        }
    }

    root.append(&sidebar);
    root.append(&canvas);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("omp UI Gallery")
        .default_width(1180)
        .default_height(820)
        .content(&root)
        .build();
    window.add_css_class("app-window");
    window.connect_map(|_| println!("UI_STORY_READY browser"));
    window.present();
}

fn set_story(canvas: &gtk::Box, story: Story) {
    while let Some(child) = canvas.first_child() {
        canvas.remove(&child);
    }
    let subject = (story.build)();
    canvas.append(&story_canvas(story, &subject));
}

fn story_canvas(story: Story, subject: &gtk::Widget) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("story-canvas");
    let heading = gtk::Label::new(Some(&format!("Story canvas: {}", story.id)));
    heading.set_xalign(0.0);
    heading.add_css_class("story-canvas-heading");
    let frame = gtk::Frame::new(None);
    frame.set_halign(gtk::Align::Center);
    frame.set_valign(gtk::Align::Center);
    frame.set_size_request(story.width, story.height);
    frame.set_child(Some(subject));
    frame.add_css_class("story-frame");
    root.append(&heading);
    root.append(&frame);
    root
}

fn snapshot_widget(
    window: &adw::ApplicationWindow,
    widget: &gtk::Widget,
    path: &Path,
) -> Result<(), String> {
    let parent = widget
        .parent()
        .ok_or_else(|| "story widget has no mapped parent".to_owned())?;
    let content_snapshot = gtk::Snapshot::new();
    parent.snapshot_child(widget, &content_snapshot);
    let content = content_snapshot
        .to_node()
        .ok_or_else(|| "story widget produced no render node".to_owned())?;
    let bounds = content.bounds();
    let snapshot = gtk::Snapshot::new();
    snapshot.append_color(
        &gtk::gdk::RGBA::new(11.0 / 255.0, 13.0 / 255.0, 16.0 / 255.0, 1.0),
        &bounds,
    );
    snapshot.append_node(&content);
    let node = snapshot
        .to_node()
        .ok_or_else(|| "story widget produced no composed render node".to_owned())?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let surface = window
        .surface()
        .ok_or_else(|| "gallery window has no render surface".to_owned())?;
    let renderer = gtk::gsk::Renderer::for_surface(&surface)
        .ok_or_else(|| "GSK could not create a renderer for the gallery surface".to_owned())?;
    let bounds = node.bounds();
    let texture = renderer.render_texture(&node, Some(&bounds));
    let result = texture
        .save_to_png(path)
        .map_err(|error| format!("could not write {}: {error}", path.display()));
    renderer.unrealize();
    result
}

#[cfg(test)]
mod tests {
    use super::{Mode, parse_options};

    #[test]
    fn parses_direct_list_and_snapshot_modes() {
        let options = parse_options(
            [
                "--story".to_owned(),
                "composer/ready".to_owned(),
                "--snapshot".to_owned(),
                "/tmp/composer.png".to_owned(),
            ]
            .into_iter(),
        )
        .expect("direct snapshot options");
        assert!(matches!(options.mode, Mode::Story(id) if id == "composer/ready"));
        assert_eq!(
            options.snapshot.as_deref(),
            Some(std::path::Path::new("/tmp/composer.png"))
        );
        assert!(matches!(
            parse_options(["--list".to_owned()].into_iter()).map(|options| options.mode),
            Ok(Mode::List)
        ));
        assert!(parse_options(["--story".to_owned()].into_iter()).is_err());
    }
}
