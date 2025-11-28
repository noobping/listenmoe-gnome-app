#[cfg(feature = "setup")]
mod setup;

mod listen;
mod meta;
mod station;

use crate::listen::ListenMoeRadio;
use crate::meta::Meta;
use crate::meta::TrackInfo;
use crate::station::Station;

#[cfg(feature = "setup")]
use crate::setup::*;
#[cfg(feature = "setup")]
use adw::gio::SimpleAction;

use adw::glib;
use adw::prelude::*;
use adw::{Application, WindowTitle};
use gtk::{gio, ApplicationWindow, Box, Button, HeaderBar, MenuButton, Orientation};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

const APP_ID: &str = "dev.noobping.listenmoe-radio";

fn main() {
    gio::resources_register_include!("compiled.gresource").expect("Failed to register resources");
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    let station = Station::Jpop;
    let radio = Rc::new(RefCell::new(ListenMoeRadio::new(station)));

    // Channel from Meta worker to main thread
    let (tx, rx) = mpsc::channel::<TrackInfo>();
    let meta = Meta::new(station, tx);
    let win_title = WindowTitle::new("LISTEN.moe", "JPOP/KPOP Radio");

    let play_button = Button::from_icon_name("media-playback-start-symbolic");
    let stop_button = Button::from_icon_name("media-playback-pause-symbolic");
    stop_button.set_visible(false);
    let play_action = gio::SimpleAction::new("play", None);
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
            radio.borrow_mut().start();
            play.set_visible(false);
            stop.set_visible(true);
        });
    }
    let stop_action = gio::SimpleAction::new("stop", None);
    {
        let radio = radio.clone();
        let data = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = stop_button.clone();
        stop_action.connect_activate(move |_, _| {
            data.stop();
            radio.borrow_mut().stop();
            stop.set_visible(false);
            play.set_visible(true);
            win.set_title("LISTEN.moe");
            win.set_subtitle("JPOP/KPOP Radio");
        });
    }
    play_button.set_action_name(Some("win.play"));
    stop_button.set_action_name(Some("win.stop"));

    // Headerbar with buttons
    let buttons = Box::new(Orientation::Horizontal, 0);
    buttons.append(&play_button);
    buttons.append(&stop_button);

    let header = HeaderBar::new();
    header.pack_start(&buttons);
    header.set_title_widget(Some(&win_title));

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

    let toggle_action = gio::SimpleAction::new("play_or_stop", None);
    {
        let play = play_button.clone();
        let stop = stop_button.clone();
        let win_clone = window.clone();
        toggle_action.connect_activate(move |_, _| {
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
    }
    window.add_action(&toggle_action);

    #[cfg(feature = "setup")]
    app.set_accels_for_action("win.setup", &["F1"]);
    app.set_accels_for_action("win.play", &["XF86AudioPlay"]);
    app.set_accels_for_action("win.stop", &["XF86AudioStop", "XF86AudioPause"]);
    app.set_accels_for_action("win.jpop", &["XF86AudioPrev"]);
    app.set_accels_for_action("win.kpop", &["XF86AudioNext"]);
    app.set_accels_for_action("win.play_or_stop", &["space", "Return"]);

    // Poll the channel on the GTK main thread and update WindowTitle
    {
        let win = win_title.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            loop {
                match rx.try_recv() {
                    Ok(info) => {
                        // Artist as title, song as subtitle
                        win.set_title(&info.artist);
                        win.set_subtitle(&info.title);
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
