fn main() {
    glib_build_tools::compile_resources(
        &["src/assets"],
        "src/assets/resources.gresource.xml",
        "omp-gtk.gresource",
    );
}
