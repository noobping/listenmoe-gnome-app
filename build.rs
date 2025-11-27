fn main() {
    glib_build_tools::compile_resources(
        &["icons"],
        "icons/resources.xml",
        "compiled.gresource",
    );
}
