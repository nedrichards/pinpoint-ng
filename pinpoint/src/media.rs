use gst::prelude::*;
use gtk::gio::prelude::*;
use gtk::{gdk, gio, glib};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MEDIA_PREPARE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaStatus {
    pub state: String,
    pub pending_state: String,
    pub desired_playing: bool,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub negotiated_caps: Option<String>,
    pub eos_count: u64,
    pub async_done_count: u64,
    pub error: Option<String>,
}

pub struct Media {
    player: gst::Element,
    sink: gst::Element,
    paintable: gdk::Paintable,
    path: PathBuf,
    bus: gst::Bus,
    error: Arc<Mutex<Option<String>>>,
    eos_count: Arc<AtomicU64>,
    async_done_count: Arc<AtomicU64>,
    desired_playing: Arc<AtomicBool>,
    has_played: AtomicBool,
    looping: Arc<AtomicBool>,
    prepare_started: Instant,
    prepare_timed_out: AtomicBool,
}

impl Media {
    pub fn new_prepared(path: &Path, audio_enabled: bool, looping: bool) -> Result<Self, String> {
        let sink = gst::ElementFactory::make("gtk4paintablesink")
            .build()
            .map_err(|error| error.to_string())?;
        let player = gst::ElementFactory::make("playbin3")
            .build()
            .map_err(|error| error.to_string())?;
        player.set_property("uri", gio::File::for_path(path).uri());
        player.set_property("video-sink", &sink);
        if !audio_enabled {
            let audio_sink = gst::ElementFactory::make("fakesink")
                .property("sync", false)
                .build()
                .map_err(|error| error.to_string())?;
            player.set_property("audio-sink", &audio_sink);
        }
        let paintable = sink.property::<gdk::Paintable>("paintable");
        let bus = player
            .bus()
            .ok_or_else(|| "GStreamer player has no message bus".to_owned())?;
        let error = Arc::new(Mutex::new(None));
        let eos_count = Arc::new(AtomicU64::new(0));
        let async_done_count = Arc::new(AtomicU64::new(0));
        let desired_playing = Arc::new(AtomicBool::new(false));
        let looping = Arc::new(AtomicBool::new(looping));
        bus.add_signal_watch();
        bus.connect_message(
            None,
            glib::clone!(
                #[strong]
                error,
                #[strong]
                eos_count,
                #[strong]
                async_done_count,
                #[strong]
                desired_playing,
                #[strong]
                looping,
                #[weak]
                player,
                move |_, message| match message.view() {
                    gst::MessageView::Error(message) => {
                        *error.lock().expect("media error lock") =
                            Some(message.error().to_string());
                    }
                    gst::MessageView::Eos(_) => {
                        eos_count.fetch_add(1, Ordering::Relaxed);
                        if looping.load(Ordering::Relaxed) {
                            let _ = player.seek_simple(
                                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                                gst::ClockTime::ZERO,
                            );
                            if desired_playing.load(Ordering::Relaxed) {
                                let _ = player.set_state(gst::State::Playing);
                            }
                        }
                    }
                    gst::MessageView::AsyncDone(_) => {
                        async_done_count.fetch_add(1, Ordering::Relaxed);
                        if desired_playing.load(Ordering::Relaxed) {
                            let _ = player.set_state(gst::State::Playing);
                        }
                    }
                    _ => {}
                }
            ),
        );
        player
            .set_state(gst::State::Paused)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            player,
            sink,
            paintable,
            path: path.to_path_buf(),
            bus,
            error,
            eos_count,
            async_done_count,
            desired_playing,
            has_played: AtomicBool::new(false),
            looping,
            prepare_started: Instant::now(),
            prepare_timed_out: AtomicBool::new(false),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn paintable(&self) -> &gdk::Paintable {
        &self.paintable
    }

    pub fn is_prepared(&self) -> bool {
        self.expire_prepare();
        self.error().is_some()
            || matches!(
                self.player.current_state(),
                gst::State::Paused | gst::State::Playing
            )
    }

    pub fn error(&self) -> Option<String> {
        self.expire_prepare();
        self.error.lock().expect("media error lock").clone()
    }

    fn expire_prepare(&self) {
        if self.prepare_started.elapsed() < MEDIA_PREPARE_TIMEOUT
            || matches!(
                self.player.current_state(),
                gst::State::Paused | gst::State::Playing
            )
            || self.error.lock().expect("media error lock").is_some()
            || self.prepare_timed_out.swap(true, Ordering::Relaxed)
        {
            return;
        }
        *self.error.lock().expect("media error lock") =
            Some("media preparation timed out after 20 seconds".into());
        self.desired_playing.store(false, Ordering::Relaxed);
        let _ = self.player.set_state(gst::State::Null);
    }

    pub fn set_playing(&self, playing: bool) -> Result<(), String> {
        let was_playing = self.desired_playing.swap(playing, Ordering::Relaxed);
        let replaying = playing && !was_playing && self.has_played.swap(true, Ordering::Relaxed);
        if replaying {
            self.seek(std::time::Duration::ZERO)?;
        }
        self.player
            .set_state(if playing {
                gst::State::Playing
            } else {
                gst::State::Paused
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn set_looping(&self, looping: bool) {
        self.looping.store(looping, Ordering::Relaxed);
    }

    pub fn seek(&self, position: std::time::Duration) -> Result<(), String> {
        self.player
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                gst::ClockTime::from_nseconds(position.as_nanos().min(u64::MAX as u128) as u64),
            )
            .map_err(|error| error.to_string())
    }

    pub fn status(&self) -> MediaStatus {
        let position = self.player.query_position::<gst::ClockTime>();
        let duration = self.player.query_duration::<gst::ClockTime>();
        let negotiated_caps = self
            .sink
            .static_pad("sink")
            .and_then(|pad| pad.current_caps())
            .map(|caps| caps.to_string());
        MediaStatus {
            state: format!("{:?}", self.player.current_state()).to_lowercase(),
            pending_state: format!("{:?}", self.player.pending_state()).to_lowercase(),
            desired_playing: self.desired_playing.load(Ordering::Relaxed),
            position_ms: position.map(|value| value.mseconds()),
            duration_ms: duration.map(|value| value.mseconds()),
            negotiated_caps,
            eos_count: self.eos_count.load(Ordering::Relaxed),
            async_done_count: self.async_done_count.load(Ordering::Relaxed),
            error: self.error(),
        }
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        self.desired_playing.store(false, Ordering::Relaxed);
        let _ = self.player.set_state(gst::State::Null);
        self.bus.remove_signal_watch();
    }
}
