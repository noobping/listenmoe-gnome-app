use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
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
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::station::Station;

const ALBUM_COVER_BASE: &str = "https://cdn.listen.moe/covers/";
const ARTIST_IMAGE_BASE: &str = "https://cdn.listen.moe/artists/";

type MetaError = Box<dyn std::error::Error + Send + Sync + 'static>;
type MetaResult<T> = Result<T, MetaError>;

/// Track info sent to the UI thread.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub artist: String,
    pub title: String,
    pub album_cover: Option<String>,
    pub artist_image: Option<String>,
}

#[derive(Debug)]
pub struct Meta {
    station: Cell<Station>,
    running: Cell<bool>,
    stop_flag: RefCell<Option<Arc<AtomicBool>>>,
    sender: Sender<TrackInfo>,
}

impl Meta {
    pub fn new(station: Station, sender: Sender<TrackInfo>) -> Rc<Self> {
        Rc::new(Self {
            station: Cell::new(station),
            running: Cell::new(false),
            stop_flag: RefCell::new(None),
            sender,
        })
    }

    pub fn set_station(&self, station: Station) {
        let was_running = self.running.get();
        if was_running {
            self.stop();
        }

        self.station.set(station);

        if was_running {
            self.start();
        }
    }

    pub fn start(&self) {
        if self.running.get() {
            return;
        }

        self.running.set(true);

        let station = self.station.get();
        let sender = self.sender.clone();

        let stop = Arc::new(AtomicBool::new(false));
        *self.stop_flag.borrow_mut() = Some(stop.clone());

        thread::spawn(move || {
            let rt =
                Runtime::new().expect("Failed to create Tokio runtime for Meta metadata loop");

            if let Err(err) = rt.block_on(run_meta_loop(station, sender, stop)) {
                eprintln!("Gateway error in metadata loop: {err}");
            }
        });
    }

    pub fn stop(&self) {
        self.running.set(false);

        if let Some(stop) = self.stop_flag.borrow_mut().take() {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

/// Protocol-level types for the LISTEN.moe gateway

#[derive(Debug, Deserialize)]
struct GatewayHello {
    heartbeat: u64,
}

#[derive(Debug, Deserialize)]
struct GatewaySongPayload {
    song: Song,
}

#[derive(Debug, Deserialize)]
struct Song {
    title: Option<String>,
    #[serde(default)]
    artists: Vec<Artist>,
    #[serde(default)]
    albums: Vec<Album>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    name: Option<String>,
    image: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Album {
    image: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayEnvelope {
    op: u8,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    d: Value,
}

const OP_HELLO: u8 = 0;
const OP_DISPATCH: u8 = 1;
const OP_HEARTBEAT_ACK: u8 = 10;
const EVENT_TRACK_UPDATE: &str = "TRACK_UPDATE";

/// Outer loop: handles reconnects.
async fn run_meta_loop(
    station: Station,
    sender: Sender<TrackInfo>,
    stop: Arc<AtomicBool>,
) -> MetaResult<()> {
    while !stop.load(Ordering::SeqCst) {
        match run_once(station.clone(), sender.clone(), stop.clone()).await {
            Ok(()) => {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                sleep(Duration::from_secs(5)).await;
            }
            Err(err) => {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                eprintln!("Gateway connection error: {err}, retrying in 5s…");
                sleep(Duration::from_secs(5)).await;
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
) -> MetaResult<()> {
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }

    let url = station.ws_url();
    let (ws_stream, _) = connect_async(url).await?;
    println!("Gateway connected to LISTEN.moe");

    let (mut write, mut read) = ws_stream.split();

    // === 1. Read hello (op=0) and get heartbeat interval ===
    let heartbeat_ms = read_hello_heartbeat(&mut read).await?;

    // === 2. Spawn heartbeat task (if we got an interval) ===
    if let Some(ms) = heartbeat_ms {
        let stop_for_hb = stop.clone();
        let mut write = write.reunite(read.into_inner()).expect("failed to reunite");
        // Re-split after spawning heartbeat:
        let (mut w, r) = write.split();
        write = w;
        read = r;

        tokio::spawn(async move {
            let interval = Duration::from_millis(ms);
            loop {
                if stop_for_hb.load(Ordering::SeqCst) {
                    break;
                }

                sleep(interval).await;

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

    // === 3. Process all subsequent messages ===
    while !stop.load(Ordering::SeqCst) {
        let Some(msg) = read.next().await else {
            break;
        };

        let msg = msg?;
        if !msg.is_text() {
            continue;
        }

        let txt = msg.into_text()?;
        let env: GatewayEnvelope = match serde_json::from_str(&txt) {
            Ok(env) => env,
            Err(err) => {
                eprintln!("Gateway JSON parse error: {err}");
                continue;
            }
        };

        match (env.op, env.t.as_deref()) {
            (OP_HEARTBEAT_ACK, _) => {
                // Could be used for metrics / debugging
                println!("Gateway heartbeat ACK");
            }
            (OP_DISPATCH, Some(EVENT_TRACK_UPDATE)) => {
                if let Some(info) = parse_track_info(&env.d) {
                    let _ = sender.send(info);
                }
            }
            _ => {
                // Ignore everything else for now
            }
        }
    }

    Ok(())
}

/// Read the initial hello and extract the heartbeat interval (if any).
async fn read_hello_heartbeat<S>(read: &mut S) -> MetaResult<Option<u64>>
where
    S: StreamExt<Item = tokio_tungstenite::tungstenite::Result<Message>> + Unpin,
{
    if let Some(msg) = read.next().await {
        let msg = msg?;
        if msg.is_text() {
            let txt = msg.into_text()?;
            let env: GatewayEnvelope = serde_json::from_str(&txt)?;

            if env.op == OP_HELLO {
                let hello: GatewayHello = serde_json::from_value(env.d)?;
                return Ok(Some(hello.heartbeat));
            }
        }
    }

    Ok(None)
}

/// Extract artist(s) + title from the typed JSON `d` value.
fn parse_track_info(d: &Value) -> Option<TrackInfo> {
    let payload: GatewaySongPayload = serde_json::from_value(d.clone()).ok()?;
    let Song {
        title,
        artists,
        albums,
    } = payload.song;

    let title = title.unwrap_or_else(|| "unknown title".to_owned());

    let artist = if artists.is_empty() {
        "Unknown artist".to_owned()
    } else {
        artists
            .iter()
            .filter_map(|a| a.name.as_deref())
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .join(", ")
    };

    let album_cover = albums
        .first()
        .and_then(|album| album.image.as_deref())
        .map(|name| format!("{ALBUM_COVER_BASE}{name}"));

    let artist_image = artists
        .first()
        .and_then(|a| a.image.as_deref())
        .map(|name| format!("{ARTIST_IMAGE_BASE}{name}"));

    Some(TrackInfo {
        artist,
        title,
        album_cover,
        artist_image,
    })
}
