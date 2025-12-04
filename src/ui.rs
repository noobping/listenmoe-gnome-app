use crate::listen::Listen;
use crate::meta::{Meta, TrackInfo};
use crate::station::Station;

#[cfg(all(target_os = "linux", feature = "setup"))]
use crate::setup::{can_install_locally, install_locally, is_installed_locally, uninstall_locally};

use adw::glib;
use adw::gtk::{
    self,
    gdk::{gdk_pixbuf::Pixbuf, Display, Texture},
    gio::{Cancellable, MemoryInputStream, Menu, SimpleAction},
    ApplicationWindow, Button, GestureClick, HeaderBar, MenuButton, Orientation, Picture, Popover,
};
use adw::prelude::*;
use adw::{Application, WindowTitle};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const COVER_MAX_SIZE: i32 = 250;
const APP_ID: &str = env!("APP_ID");

fn make_action<F>(name: &str, f: F) -> SimpleAction
where
    F: Fn() + 'static,
{
    let action = SimpleAction::new(name, None);
    action.connect_activate(move |_, _| f());
    action
}

fn create_station_action(
    station: Station,
    play_button: &Button,
    window: &ApplicationWindow,
    radio: &Rc<Listen>,
    meta: &Rc<Meta>,
) -> SimpleAction {
    let radio = radio.clone();
    let meta = meta.clone();
    let win_clone = window.clone();
    let play = play_button.clone();

    make_action(station.name(), move || {
        radio.set_station(station);
        meta.set_station(station);
        if play.is_visible() {
            let _ = adw::prelude::WidgetExt::activate_action(
                &win_clone,
                "win.play",
                None::<&glib::Variant>,
            );
        }
    })
}

/// Build the user interface.  This function is called once when the application
/// is activated.  It constructs the window, header bar, actions and spawns
/// background tasks for streaming audio and metadata.
pub fn build_ui(app: &Application) {
    let station = Station::Jpop;
    let radio = Listen::new(station);
    let (tx, rx) = mpsc::channel::<TrackInfo>();
    let meta = Meta::new(station, tx);
    let (cover_tx, cover_rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    let win_title = WindowTitle::new("LISTEN.moe", "JPOP/KPOP Radio");

    let play_button = Button::from_icon_name("media-playback-start-symbolic");
    play_button.set_action_name(Some("win.play"));
    let stop_button = Button::from_icon_name("media-playback-pause-symbolic");
    stop_button.set_action_name(Some("win.stop"));
    stop_button.set_visible(false);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Listen.moe Radio")
        .icon_name(APP_ID)
        .default_width(300)
        .default_height(40)
        .resizable(false)
        .build();

    let platform_config = PlatformConfig {
        dbus_name: APP_ID,
        display_name: "LISTEN.moe",
        hwnd: None,
    };
    let mut controls = MediaControls::new(platform_config).expect("Failed to init media controls");
    let controls = Rc::new(RefCell::new(controls));
    let (ctrl_tx, ctrl_rx) = mpsc::channel::<MediaControlEvent>();
    {
        let tx = ctrl_tx.clone();
        controls
            .borrow_mut()
            .attach(move |event| { let _ = tx.send(event); })
            .expect("Failed to attach media control events");
    }
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = stop_button.clone();
        let controls = controls.clone();
        make_action("play", move || {
            win.set_title("LISTEN.moe");
            win.set_subtitle("Connecting...");
            meta.start();
            radio.start();
            play.set_visible(false);
            stop.set_visible(true);
            let _ = controls.borrow_mut().set_playback(MediaPlayback::Playing { progress: None });
        })
    });
    window.add_action(&{
        let radio = radio.clone();
        let meta = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = stop_button.clone();
        let controls = controls.clone();
        make_action("stop", move || {
            meta.stop();
            radio.stop();
            stop.set_visible(false);
            play.set_visible(true);
            win.set_title("LISTEN.moe");
            win.set_subtitle("JPOP/KPOP Radio");
            let _ = controls.borrow_mut().set_playback(MediaPlayback::Paused { progress: None });
        })
    });
    window.add_action(&{
        let win = window.clone();
        make_action("quit", move || win.close())
    });
    window.add_action(&{
        let win_clone = window.clone();
        make_action("about", move || {
            let authors: Vec<_> = env!("CARGO_PKG_AUTHORS").split(':').collect();
            let about = adw::AboutDialog::builder()
                .application_name(env!("CARGO_PKG_NAME"))
                .application_icon(APP_ID)
                .version(env!("CARGO_PKG_VERSION"))
                .developers(&authors[..])
                .website(option_env!("CARGO_PKG_HOMEPAGE").unwrap_or(""))
                .issue_url(option_env!("ISSUE_TRACKER").unwrap_or(""))
                .license_type(gtk::License::MitX11)
                .comments(option_env!("CARGO_PKG_DESCRIPTION").unwrap_or(""))
                .build();
            about.present(Some(&win_clone));
        })
    });
    window.add_action(&{
        let play = play_button.clone();
        let stop = stop_button.clone();
        let win_clone = window.clone();
        make_action("toggle", move || {
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
        })
    });
    window.add_action(&{
        let win = win_title.clone();
        make_action("copy", move || {
            let artist = win.title();
            let title = win.subtitle();
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
        })
    });

    // Build UI
    let menu = Menu::new();
    menu.append(Some("Copy title & artist"), Some("win.copy"));
    let more_button = MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Main Menu")
        .menu_model(&menu)
        .build();
    let buttons = gtk::Box::new(Orientation::Horizontal, 0);
    buttons.append(&more_button);
    buttons.append(&play_button);
    buttons.append(&stop_button);
    let header = HeaderBar::new();
    header.pack_start(&buttons);
    header.set_title_widget(Some(&win_title));
    header.set_show_title_buttons(false);

    let art_picture = Picture::builder()
        .can_shrink(true)
        .focusable(false)
        .sensitive(false)
        .build();
    let art_popover = Popover::builder()
        .has_arrow(true)
        .position(gtk::PositionType::Bottom)
        .autohide(true)
        .child(&art_picture)
        .build();
    art_popover.set_parent(&header);
    let title_click = GestureClick::new();
    {
        let picture = art_picture.clone();
        let art = art_popover.clone();
        title_click.connect_released(move |_, _, _, _| {
            if art.is_visible() {
                art.popdown();
            } else if picture.paintable().is_some() {
                art.popup();
            }
        });
    }
    win_title.add_controller(title_click);
    let close_any_click = GestureClick::new();
    {
        let art = art_popover.clone();
        close_any_click.connect_released(move |_, _, _, _| {
            art.popdown();
        });
    }
    art_popover.add_controller(close_any_click);

    // Tiny dummy content so GTK can shrink the window
    let dummy = gtk::Box::new(Orientation::Vertical, 0);
    dummy.set_height_request(0);
    dummy.set_vexpand(false);

    let close_btn = Button::from_icon_name("window-close-symbolic");
    close_btn.set_action_name(Some("win.quit"));
    header.pack_end(&close_btn);

    window.set_titlebar(Some(&header));
    window.set_child(Some(&dummy));

    for station in [Station::Jpop, Station::Kpop] {
        let action = create_station_action(station, &play_button, &window, &radio, &meta);
        window.add_action(&action);
        menu.append(
            Some(&format!("Play {}", station.display_name())),
            Some(&format!("win.{}", station.name())),
        );
    }
    menu.append(Some("About"), Some("win.about"));

    #[cfg(all(target_os = "linux", feature = "setup"))]
    let setup_index = menu.n_items();
    #[cfg(all(target_os = "linux", feature = "setup"))]
    menu.append(Some(if is_installed_locally() { "Uninstall" } else { "Install" } ), Some("win.setup"));

    menu.append(Some("Quit"), Some("win.quit"));

    #[cfg(all(target_os = "linux", feature = "setup"))]
    window.add_action(&{
        let menu_clone = menu.clone();
        make_action("setup", move || {
            if !can_install_locally() {
                return;
            }
            let was_installed = is_installed_locally();
            let _ = match was_installed {
                true => uninstall_locally(),
                false => install_locally(),
            };
            let new_label = if was_installed { "Install" } else { "Uninstall" };
            menu_clone.remove(setup_index);
            menu_clone.insert(setup_index, Some(new_label), Some("win.setup"));
        })
    });

    #[cfg(all(target_os = "linux", feature = "setup"))]
    app.set_accels_for_action("win.setup", &["F2"]);
    app.set_accels_for_action("win.about", &["F1"]);
    app.set_accels_for_action("win.copy", &["<primary>c"]);
    app.set_accels_for_action("win.quit", &["<primary>q", "Escape"]);
    app.set_accels_for_action("win.play", &["XF86AudioPlay"]);
    app.set_accels_for_action("win.stop", &["XF86AudioStop", "XF86AudioPause"]);
    app.set_accels_for_action("win.jpop", &["<primary>j", "XF86AudioPrev", "<primary>z"]);
    app.set_accels_for_action("win.kpop", &["<primary>k", "XF86AudioNext", "<primary><shift>z", "<primary>y"]);
    app.set_accels_for_action("win.toggle", &["<primary>p", "space", "Return", "<primary>s"]);

    // Poll the channels on the GTK main thread and update the UI.
    {
        let win = win_title.clone();
        let art_popover = art_popover.clone();
        let art_picture = art_picture.clone();
        let cover_rx = cover_rx;
        let cover_tx = cover_tx.clone();
        let window = window.clone();
        let ctrl_rx = ctrl_rx;
        glib::timeout_add_local(Duration::from_millis(100), move || {
            for event in ctrl_rx.try_iter() {
                let _ = match event {
                    MediaControlEvent::Play => adw::prelude::WidgetExt::activate_action(&window, "win.play", None::<&glib::Variant>),
                    MediaControlEvent::Pause | MediaControlEvent::Stop => adw::prelude::WidgetExt::activate_action(&win, "win.stop", None::<&glib::Variant>),
                    MediaControlEvent::Toggle => adw::prelude::WidgetExt::activate_action(&window, "win.toggle", None::<&glib::Variant>),
                    MediaControlEvent::Next => adw::prelude::WidgetExt::activate_action(&window, "win.kpop", None::<&glib::Variant>),
                    MediaControlEvent::Previous => adw::prelude::WidgetExt::activate_action(&window, "win.jpop", None::<&glib::Variant>),
                    _ => Ok(())
                };
            }

            for info in rx.try_iter() {
                win.set_title(&info.artist);
                win.set_subtitle(&info.title);

                if let Some(url) = info.album_cover.as_ref().or(info.artist_image.as_ref()) {
                    let tx = cover_tx.clone();
                    let url = url.to_string();
                    thread::spawn(move || {
                        let result = fetch_cover_bytes_blocking(&url).map_err(|e| e.to_string());
                        let _ = tx.send(result);
                    });
                } else {
                    art_popover.popdown();
                }
            }

            for result in cover_rx.try_iter() {
                match result {
                    Ok(bytes_vec) => {
                        let bytes = glib::Bytes::from_owned(bytes_vec);
                        let stream = MemoryInputStream::from_bytes(&bytes);
                        match Pixbuf::from_stream_at_scale(&stream, COVER_MAX_SIZE, COVER_MAX_SIZE, true, None::<&Cancellable>) {
                            Ok(pixbuf) => {
                                let texture = Texture::for_pixbuf(&pixbuf);
                                art_picture.set_paintable(Some(&texture));
                                if window.is_active() {
                                    art_popover.popup();
                                }
                            }
                            Err(err) => {
                                eprintln!("Failed to decode cover pixbuf: {err}");
                                art_popover.popdown();
                            }
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to load cover bytes: {err}");
                        art_popover.popdown();
                    }
                }
            }

            glib::ControlFlow::Continue
        });
    }

    window.present();
}

/// Download an image synchronously. This helper runs in a worker thread and
/// therefore does not block the GTK main loop. It returns the raw bytes
/// representing the image or an error if the request fails.
pub fn fetch_cover_bytes_blocking(url: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let resp = reqwest::blocking::get(url)?;
    if !resp.status().is_success() {
        return Err(format!("Non-success status: {}", resp.status()).into());
    }
    let body = resp.bytes()?;
    Ok(body.to_vec())
}
