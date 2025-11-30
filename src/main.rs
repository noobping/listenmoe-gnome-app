#[cfg(target_os = "linux")]
#[cfg(feature = "setup")]
mod setup;

mod config;
mod listen;
mod meta;
mod station;

use crate::config::APP_ID;
use crate::listen::Listen;
use crate::meta::Meta;
use crate::meta::TrackInfo;
use crate::station::Station;

#[cfg(feature = "setup")]
use crate::setup::*;

use adw::glib;
use adw::prelude::*;
use adw::{Application, WindowTitle};
use gtk::{
    gdk::{gdk_pixbuf, Display, Texture}, GestureClick,
    gio::{resources_register_include, Cancellable, File, Menu, SimpleAction},
    ApplicationWindow, Box, Button, HeaderBar, MenuButton, Orientation, Picture, Popover,
};
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

fn main() {
    resources_register_include!("compiled.gresource").expect("Failed to register resources");
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    let station = Station::Jpop;
    let radio = Listen::new(station);

    // Channel from Meta worker to main thread
    let (tx, rx) = mpsc::channel::<TrackInfo>();
    let meta = Meta::new(station, tx);
    let win_title = WindowTitle::new("LISTEN.moe", "JPOP/KPOP Radio");

    let play_button = Button::from_icon_name("media-playback-start-symbolic");
    let stop_button = Button::from_icon_name("media-playback-pause-symbolic");
    stop_button.set_visible(false);
    let play_action = SimpleAction::new("play", None);
    {
        let radio = radio.clone();
        let data = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = stop_button.clone();
        play_action.connect_activate(move |_, _| {
            win.set_title("LISTEN.moe");
            win.set_subtitle("Connecting...");
            data.start();
            radio.start();
            play.set_visible(false);
            stop.set_visible(true);
        });
    }
    let stop_action = SimpleAction::new("stop", None);
    {
        let radio = radio.clone();
        let data = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = stop_button.clone();
        stop_action.connect_activate(move |_, _| {
            data.stop();
            radio.stop();
            stop.set_visible(false);
            play.set_visible(true);
            win.set_title("LISTEN.moe");
            win.set_subtitle("JPOP/KPOP Radio");
        });
    }
    play_button.set_action_name(Some("win.play"));
    stop_button.set_action_name(Some("win.stop"));

    // menu
    let menu = Menu::new();
    // menu.append(Some("About"), Some("app.about"));
    menu.append(Some("Copy title & artist"), Some("win.copy"));
    menu.append(Some("Play J-pop"), Some("win.jpop"));
    menu.append(Some("Play K-pop"), Some("win.kpop"));
    menu.append(Some("Quite"), Some("win.quite"));
    let more_button = MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Main Menu")
        .menu_model(&menu)
        .build();

    // Headerbar with buttons
    let buttons = Box::new(Orientation::Horizontal, 0);
    buttons.append(&more_button);
    buttons.append(&play_button);
    buttons.append(&stop_button);

    let header = HeaderBar::new();
    header.pack_start(&buttons);
    header.set_title_widget(Some(&win_title));
    header.set_show_title_buttons(false);

    let art_picture = Picture::builder().can_shrink(true).build();
    let art_popover = Popover::builder()
        .has_arrow(true)
        .position(gtk::PositionType::Bottom)
        .autohide(true)
        .child(&art_picture)
        .build();
    art_popover.set_parent(&header);

    let click = GestureClick::new();
    {
        let art = art_popover.clone();
        click.connect_released(move |_, _, _, _| {
            art.popdown();
        });
    }
    art_picture.add_controller(click);

    // Tiny dummy content so GTK can shrink the window
    let dummy = Box::new(Orientation::Vertical, 0);
    dummy.set_height_request(0);
    dummy.set_vexpand(false);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Listen.moe Radio")
        .icon_name("listenmoe")
        .default_width(300)
        .default_height(40)
        .resizable(false)
        .build();

    let close_btn = Button::from_icon_name("window-close-symbolic");
    close_btn.set_action_name(Some("win.quite"));
    header.pack_end(&close_btn);
    let close_action = SimpleAction::new("quite", None);
    {
        let win = window.clone();
        close_action.connect_activate(move |_, _| {
            win.close();
        });
    }

    window.set_titlebar(Some(&header));
    window.set_child(Some(&dummy));

    #[cfg(feature = "setup")]
    let action = SimpleAction::new("setup", None);
    #[cfg(feature = "setup")]
    action.connect_activate(move |_, _| {
        if !can_install_locally() {
            return;
        }
        let _ = match is_installed_locally() {
            true => uninstall_locally(),
            false => install_locally(),
        };
    });

    #[cfg(feature = "setup")]
    window.add_action(&action);
    window.add_action(&play_action);
    window.add_action(&stop_action);
    window.add_action(&close_action);

    {
        let play = play_button.clone();
        let stop = stop_button.clone();
        let win_clone = window.clone();
        let action = SimpleAction::new("toggle", None);
        action.connect_activate(move |_, _| {
            if play.is_visible() {
                let _ = adw::prelude::WidgetExt::activate_action(
                    &win_clone,
                    "win.play",
                    None::<&glib::Variant>,
                );
            } else if stop.is_visible() {
                let _ = adw::prelude::WidgetExt::activate_action(
                    &win_clone,
                    "win.stop",
                    None::<&glib::Variant>,
                );
            }
        });
        window.add_action(&action);
    }

    {
        let play = play_button.clone();
        let win_clone = window.clone();
        let radio = radio.clone();
        let data = meta.clone();
        let action = SimpleAction::new("jpop", None);
        action.connect_activate(move |_, _| {
            radio.set_station(Station::Jpop);
            data.set_station(Station::Jpop);
            if play.is_visible() {
                let _ = adw::prelude::WidgetExt::activate_action(
                    &win_clone,
                    "win.play",
                    None::<&glib::Variant>,
                );
            }
        });
        window.add_action(&action);
    }

    {
        let play = play_button.clone();
        let win_clone = window.clone();
        let radio = radio.clone();
        let data = meta.clone();
        let action = SimpleAction::new("kpop", None);
        action.connect_activate(move |_, _| {
            radio.set_station(Station::Kpop);
            data.set_station(Station::Kpop);
            if play.is_visible() {
                let _ = adw::prelude::WidgetExt::activate_action(
                    &win_clone,
                    "win.play",
                    None::<&glib::Variant>,
                );
            }
        });
        window.add_action(&action);
    }

    {
        let win = win_title.clone();
        let action = SimpleAction::new("copy", None);
        action.connect_activate(move |_, _| {
            // Get artist and title from the WindowTitle
            let artist = win.title(); // artist
            let title = win.subtitle(); // song title

            // If nothing is playing yet, do nothing
            if artist.is_empty() && title.is_empty() {
                return;
            }

            let text = if artist.is_empty() {
                title.to_string()
            } else if title.is_empty() {
                artist.to_string()
            } else {
                format!("{artist}, {title}")
            };

            if let Some(display) = Display::default() {
                let clipboard = display.clipboard();
                clipboard.set_text(&text);
            }
        });
        window.add_action(&action);
    }

    #[cfg(feature = "setup")]
    app.set_accels_for_action("win.setup", &["F1"]);
    app.set_accels_for_action("win.copy", &["<primary>c"]);
    app.set_accels_for_action("win.quite", &["<primary>q", "Escape"]);
    app.set_accels_for_action("win.play", &["XF86AudioPlay"]);
    app.set_accels_for_action("win.stop", &["XF86AudioStop", "XF86AudioPause"]);
    app.set_accels_for_action("win.jpop", &["<primary>j", "XF86AudioPrev", "<primary>z"]);
    app.set_accels_for_action("win.kpop", &["<primary>k", "XF86AudioNext", "<primary><shift>z", "<primary>y"]);
    app.set_accels_for_action("win.toggle", &["<primary>p", "space", "Return", "<primary>s"]);

    // Poll the channel on the GTK main thread and update WindowTitle
    {
        let win = win_title.clone();
        let art_popover = art_popover.clone();
        let art_picture = art_picture.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            loop {
                match rx.try_recv() {
                    Ok(info) => {
                        // Artist as title, song as subtitle
                        win.set_title(&info.artist);
                        win.set_subtitle(&info.title);
                        // Prefer album cover; fall back to artist image
                        let image_url = info.album_cover.clone().or(info.artist_image.clone());
                        if let Some(url) = image_url {
                            let file = File::for_uri(&url);
                            if let Ok(stream) = file.read(None::<&Cancellable>) {
                                // Scale down while decoding
                                if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_stream_at_scale(
                                    &stream,
                                    250,  // max width
                                    250,  // max height
                                    true, // preserve aspect ratio
                                    None::<&Cancellable>,
                                ) {
                                    let texture = Texture::for_pixbuf(&pixbuf);
                                    art_picture.set_paintable(Some(&texture));
                                    art_popover.popup();
                                } else {
                                    art_popover.popdown(); // Failed to decode
                                }
                            } else {
                                art_popover.popdown(); // Failed to open remote file
                            }
                        } else {
                            art_popover.popdown(); // No image at all
                        }
                    }
                    Err(TryRecvError::Empty) => {
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        return glib::ControlFlow::Break;
                    }
                }
            }

            glib::ControlFlow::Continue
        });
    }

    window.present();
}
