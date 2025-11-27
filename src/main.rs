mod listen;
mod meta;

use crate::listen::ListenMoeRadio;
use crate::meta::Meta;
use crate::meta::TrackInfo;

use adw::glib;
use adw::prelude::*;
use adw::{Application, WindowTitle};
use gtk::{gio, ApplicationWindow, Box, Button, HeaderBar, Orientation};
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
    let radio = Rc::new(RefCell::new(ListenMoeRadio::new()));

    // Channel from Meta worker to main thread
    let (tx, rx) = mpsc::channel::<TrackInfo>();
    let meta = Meta::new(tx);
    let win_title = WindowTitle::new("LISTEN.moe", "JPOP/KPOP Radio");

    let play_button = Button::with_label("play");
    let stop_button = Button::with_label("stop");
    stop_button.set_visible(false);

    {
        let radio = radio.clone();
        let data = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = stop_button.clone();
        play_button.connect_clicked(move |_| {
            win.set_title("LISTEN.moe");
            win.set_subtitle("Connecting...");
            data.start();
            radio.borrow_mut().start();
            play.set_visible(false);
            stop.set_visible(true);
        });
    }

    {
        let radio = radio.clone();
        let data = meta.clone();
        let win = win_title.clone();
        let play = play_button.clone();
        let stop = stop_button.clone();
        stop_button.connect_clicked(move |_| {
            data.stop();
            radio.borrow_mut().stop();
            stop.set_visible(false);
            play.set_visible(true);
            win.set_title("LISTEN.moe");
            win.set_subtitle("JPOP/KPOP Radio");
        });
    }

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
        .default_width(300)
        .default_height(40)
        .resizable(false)
        .build();

    window.set_titlebar(Some(&header));
    window.set_child(Some(&dummy));

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
