use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::station::Station;

const ALBUM_COVER_BASE: &str = "https://cdn.listen.moe/covers/";
const ARTIST_IMAGE_BASE: &str = "https://cdn.listen.moe/artists/";

/// Track info sent to the UI thread.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub artist: String,
    pub title: String,
    pub album_cover: Option<String>,
    pub artist_image: Option<String>,
}

pub struct Meta {
    station: Cell<Station>,
    running: Cell<bool>,
    stop_flag: RefCell<Option<Arc<AtomicBool>>>,
    sender: Sender<TrackInfo>,
}

impl Meta {
    /// Create a new Meta, using the given channel to send track updates.
    pub fn new(station: Station, sender: Sender<TrackInfo>) -> Rc<Self> {
        Rc::new(Self {
            station: Cell::new(station),
            running: Cell::new(false),
            stop_flag: RefCell::new(None),
            sender,
        })
    }

    pub fn set_station(self: &Rc<Self>, station: Station) {
        let was_running = self.running.get();
        if was_running {
            self.stop();
        }
        self.station.set(station);
        if was_running {
            self.start();
        }
    }

    pub fn start(self: &Rc<Self>) {
        if self.running.get() {
            return;
        }
        self.running.set(true);

        let station = self.station.get();
        let sender = self.sender.clone();

        let stop = Arc::new(AtomicBool::new(false));
        *self.stop_flag.borrow_mut() = Some(stop.clone());

        thread::spawn(move || {
            let rt = Runtime::new().expect("Failed to create Tokio runtime for Meta");

            if let Err(err) = rt.block_on(run_meta_loop(station, sender, stop)) {
                eprintln!("Gateway error in metadata loop: {err}");
            }
        });
    }

    pub fn stop(&self) {
        self.running.set(false); // Mark as not running

        // Signal the background meta loop to stop
        if let Some(stop) = self.stop_flag.borrow_mut().take() {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

/// Outer loop: reconnect if needed.
async fn run_meta_loop(
    station: Station,
    sender: Sender<TrackInfo>,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    while !stop.load(Ordering::SeqCst) {
        match run_once(station.clone(), sender.clone(), stop.clone()).await {
            Ok(()) => {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(err) => {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                eprintln!("Gateway connection error: {err}, retrying in 5s…");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    Ok(())
}

/// Single websocket session.
async fn run_once(
    station: Station,
    sender: Sender<TrackInfo>,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }

    let url = station.ws_url();
    let (ws_stream, _) = connect_async(url).await?;
    println!("Gateway connected to LISTEN.moe");

    let (mut write, mut read) = ws_stream.split();

    // 1. Read hello (op 0) and prepare heartbeat
    let mut heartbeat_ms: Option<u64> = None;

    if let Some(msg) = read.next().await {
        let msg = msg?;
        if msg.is_text() {
            let txt = msg.into_text()?;
            if let Ok(json) = serde_json::from_str::<Value>(&txt) {
                let op = json["op"].as_i64().unwrap_or(-1);
                if op == 0 {
                    heartbeat_ms = json["d"]["heartbeat"].as_u64();
                }
            }
        }
    }

    // Spawn heartbeat sender if there is an interval
    if let Some(ms) = heartbeat_ms {
        let stop_for_hb = stop.clone();
        tokio::spawn(async move {
            let interval = Duration::from_millis(ms);
            loop {
                if stop_for_hb.load(Ordering::SeqCst) {
                    break;
                }

                tokio::time::sleep(interval).await;

                if stop_for_hb.load(Ordering::SeqCst) {
                    break;
                }

                if let Err(err) = write.send(Message::Text(r#"{"op":9}"#.into())).await {
                    eprintln!("Gateway heartbeat send error: {err}");
                    break;
                }
            }
        });
    }

    // 2. Process messages, look for TRACK_UPDATE
    while !stop.load(Ordering::SeqCst) {
        let Some(msg) = read.next().await else {
            break;
        };

        let msg = msg?;
        if !msg.is_text() {
            continue;
        }

        let txt = msg.into_text()?;
        let json: Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("Gateway JSON parse error: {err}");
                continue;
            }
        };

        let op = json["op"].as_i64().unwrap_or(-1);
        let t = json["t"].as_str().unwrap_or("");

        if op == 10 {
            println!("Gateway heartbeat ACK");
            continue;
        }

        if op == 1 && t == "TRACK_UPDATE" {
            if let Some(info) = parse_track_info(&json) {
                let _ = sender.send(info);
            }
        }
    }

    Ok(())
}

/// Extract artist(s) + title from JSON.
fn parse_track_info(json: &Value) -> Option<TrackInfo> {
    let song = json.get("d")?.get("song")?;

    let title = song
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown title")
        .to_string();

    let artists: Vec<String> = song
        .get("artists")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                .map(|s| s.to_owned())
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(Vec::new);

    let artist = if artists.is_empty() {
        "Unknown artist".to_string()
    } else {
        artists.join(", ")
    };

    let album_cover = song
        .get("albums")
        .and_then(|a| a.as_array())
        .and_then(|arr| arr.first())
        .and_then(|album| album.get("image"))
        .and_then(|img| img.as_str())
        .map(|name| format!("{ALBUM_COVER_BASE}{name}"));

    let artist_image = song
        .get("artists")
        .and_then(|a| a.as_array())
        .and_then(|arr| arr.first())
        .and_then(|artist| artist.get("image"))
        .and_then(|img| img.as_str())
        .map(|name| format!("{ARTIST_IMAGE_BASE}{name}"));

    Some(TrackInfo {
        artist,
        title,
        album_cover,
        artist_image,
    })
}
