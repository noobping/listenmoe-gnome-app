#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(all(target_os = "linux", feature = "setup"))]
mod setup;

mod http_source;
mod listen;
mod locale;
mod meta;
mod station;
mod ui;

const APP_ID: &str = env!("APP_ID");
#[cfg(any(debug_assertions, feature = "setup", feature = "icon"))]
const RESOURCE_ID: &str = env!("RESOURCE_ID");
use adw::gtk::{gdk::Display, IconTheme};
use adw::prelude::*;
use adw::Application;
#[cfg(any(debug_assertions, feature = "setup", feature = "icon"))]
use adw::gtk::{gdk::Display, IconTheme};

fn main() {
    locale::init_i18n();

    // Register resources compiled into the binary. If this fails, the app cannot find its assets.
    #[cfg(any(debug_assertions, feature = "setup", feature = "icon"))]
    adw::gtk::gio::resources_register_include!("compiled.gresource")
        .expect("Failed to register resources");

    // Initialize libadwaita/GTK. This must be called before any UI code.
    adw::init().expect("Failed to initialize libadwaita");

    // Load the icon theme from the embedded resources so that icons resolve correctly even outside a installed environment.
    #[cfg(any(debug_assertions, feature = "setup", feature = "icon"))]
    if let Some(display) = Display::default() {
        let theme = IconTheme::for_display(&display);
        theme.add_resource_path(RESOURCE_ID);
    }

    // Create the GTK application. The application ID must be unique and corresponds to the desktop file name.
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(ui::build_ui); // Build the UI when the application is activated.
    app.run(); // Run the application. This function does not return until the last window is closed.
}
