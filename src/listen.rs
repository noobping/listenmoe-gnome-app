use reqwest::blocking::{Client, Response};
use rodio::{buffer::SamplesBuffer, OutputStreamBuilder, Sink};
use std::cell::RefCell;
use std::error::Error;
use std::io::{self, Read, Seek, SeekFrom};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::station::Station;

type DynError = Box<dyn Error + Send + Sync>;
type Result<T> = std::result::Result<T, DynError>;

#[derive(Debug)]
struct HttpSource {
    inner: Response,
}

impl Read for HttpSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner
            .read(buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "seeking not supported on HTTP stream",
        ))
    }
}

impl MediaSource for HttpSource {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[derive(Debug)]
struct Inner {
    station: Station,
    running: bool,
    stop_flag: Option<Arc<AtomicBool>>,
}

#[derive(Debug)]
pub struct Listen {
    inner: RefCell<Inner>,
}

impl Listen {
    pub fn new(station: Station) -> Rc<Self> {
        Rc::new(Self {
            inner: RefCell::new(Inner {
                station,
                running: false,
                stop_flag: None,
            }),
        })
    }

    pub fn set_station(&self, station: Station) {
        let mut inner = self.inner.borrow_mut();

        let was_running = inner.running;
        if was_running {
            Self::stop_inner(&mut inner);
        }

        inner.station = station;

        if was_running {
            Self::start_inner(&mut inner);
        }
    }

    pub fn start(&self) {
        let mut inner = self.inner.borrow_mut();
        if inner.running {
            return;
        }
        Self::start_inner(&mut inner);
    }

    pub fn stop(&self) {
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }

    fn start_inner(inner: &mut Inner) {
        if inner.running {
            return;
        }

        inner.running = true;

        let stop = Arc::new(AtomicBool::new(false));
        inner.stop_flag = Some(Arc::clone(&stop));

        let station = inner.station;

        thread::spawn(move || {
            if let Err(err) = run_listenmoe_stream(station, stop) {
                eprintln!("stream error: {err}");
            }
        });
    }

    fn stop_inner(inner: &mut Inner) {
        if !inner.running {
            return;
        }

        inner.running = false;

        if let Some(stop) = inner.stop_flag.take() {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

impl Drop for Listen {
    fn drop(&mut self) {
        // Best-effort cleanup if user "forgets" to stop.
        let mut inner = self.inner.borrow_mut();
        Self::stop_inner(&mut inner);
    }
}

fn run_listenmoe_stream(station: Station, stop: Arc<AtomicBool>) -> Result<()> {
    let url = station.stream_url();
    println!("Connecting to {url}…");

    let client = Client::new();
    let response = client
        .get(url)
        .header("User-Agent", "listenmoe-rodio-symphonia/0.1")
        .send()?;

    println!("HTTP status: {}", response.status());
    if !response.status().is_success() {
        return Err(format!("HTTP status {}", response.status()).into());
    }

    let http_source = HttpSource { inner: response };
    let mss = MediaSourceStream::new(Box::new(http_source), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("ogg");

    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    let probed =
        symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "no supported audio tracks".to_string())?;

    let mut track_id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

    let stream = OutputStreamBuilder::open_default_stream()?;
    let sink = Sink::connect_new(&stream.mixer());

    println!("Started decoding + playback.");

    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;

    while !stop.load(Ordering::Relaxed) {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::ResetRequired) => {
                eprintln!("Stream reset, reconfiguring decoder…");
                let new_track = format
                    .tracks()
                    .iter()
                    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
                    .ok_or_else(|| "no supported audio tracks after reset".to_string())?;

                track_id = new_track.id;
                decoder = symphonia::default::get_codecs()
                    .make(&new_track.codec_params, &decoder_opts)?;

                sample_buf = None;
                channels = 0;
                sample_rate = 0;
                continue;
            }
            Err(err) => {
                return Err(format!("Error reading packet: {err:?}").into());
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::ResetRequired) => {
                eprintln!("Decoder reset required, rebuilding decoder…");
                let new_track = format
                    .tracks()
                    .iter()
                    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
                    .ok_or_else(|| "no supported audio tracks after decoder reset".to_string())?;

                track_id = new_track.id;
                decoder = symphonia::default::get_codecs()
                    .make(&new_track.codec_params, &decoder_opts)?;
                sample_buf = None;
                channels = 0;
                sample_rate = 0;
                continue;
            }
            Err(err) => {
                return Err(format!("Fatal decode error: {err:?}").into());
            }
        };

        if sample_buf.is_none() {
            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;

            channels = spec.channels.count() as u16;
            sample_rate = spec.rate;

            sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
        }

        let buf = sample_buf.as_mut().expect("sample_buf just initialized");
        buf.copy_interleaved_ref(decoded);

        let samples = buf.samples().to_owned();
        let source = SamplesBuffer::new(channels, sample_rate, samples);
        sink.append(source);
    }

    sink.stop();
    Ok(())
}
