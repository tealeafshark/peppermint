// SPDX-License-Identifier: GPL-3.0-only

//! Dedicated pure-Rust audio playback engine using `rodio` + `symphonia` with:
//! - True 0 ms gapless playback via pre-decoded next-track chaining
//! - Optional equal-power crossfade between tracks
//! - ReplayGain track/album loudness normalization with peak limiting
//! - Optional soft fade-out/in on pause and resume
//! - Real-time 10-band graphic EQ and live FFT audio tap for the visualizer

use crate::audio::eq::{EqSettings, GraphicEqProcessor, SharedEqParams};
use crate::audio::visualizer::SharedAudioTap;
use crate::library::metadata::{ReplayGainInfo, read_replay_gain};
use crate::settings::{AppSettings, ReplayGainMode};
use rodio::source::SeekError;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::f32::consts::FRAC_PI_2;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

const FADE_PLAYING: u8 = 0;
const FADE_OUT_TO_PAUSE: u8 = 1;

/// Specification of the upcoming track to pre-open and pre-decode for gapless/crossfade playback.
#[derive(Debug, Clone)]
pub struct NextTrackSpec {
    pub token: u64,
    pub path: PathBuf,
    pub duration_hint: u64,
}

/// Commands sent from the UI thread to the audio engine thread.
#[derive(Debug)]
pub enum AudioCommand {
    Play {
        path: PathBuf,
        duration_hint: u64,
        start_paused: bool,
    },
    PreloadNext(Option<NextTrackSpec>),
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SetVolume(f32),
}

/// Live telemetry snapshot read by the UI.
#[derive(Debug, Clone, Default)]
pub struct PlaybackSnapshot {
    pub is_playing: bool,
    pub is_paused: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Incremented when playback drains completely (end of queue or gapless disabled).
    pub finished_generation: u64,
    /// Incremented when the engine seamlessly auto-advanced into a preloaded track.
    pub gapless_advanced_generation: u64,
    /// The `NextTrackSpec::token` of the track we seamlessly advanced into.
    pub gapless_advanced_token: u64,
    pub last_error: Option<String>,
}

struct SharedTelemetry {
    is_playing: AtomicBool,
    is_paused: AtomicBool,
    position_ms: AtomicU64,
    duration_ms: AtomicU64,
    finished_generation: AtomicU64,
    gapless_advanced_generation: AtomicU64,
    gapless_advanced_token: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl Default for SharedTelemetry {
    fn default() -> Self {
        Self {
            is_playing: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            position_ms: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            finished_generation: AtomicU64::new(0),
            gapless_advanced_generation: AtomicU64::new(0),
            gapless_advanced_token: AtomicU64::new(0),
            last_error: Mutex::new(None),
        }
    }
}

/// Lock-free runtime playback settings read directly inside the audio callback.
struct SharedAudioSettings {
    gapless: AtomicBool,
    crossfade_enabled: AtomicBool,
    crossfade_ms: AtomicU64,
    replay_gain_mode: AtomicU8,
    soft_fade: AtomicBool,
}

impl SharedAudioSettings {
    fn new(settings: &AppSettings) -> Self {
        let s = Self {
            gapless: AtomicBool::new(true),
            crossfade_enabled: AtomicBool::new(false),
            crossfade_ms: AtomicU64::new(3000),
            replay_gain_mode: AtomicU8::new(0),
            soft_fade: AtomicBool::new(true),
        };
        s.apply(settings);
        s
    }

    fn apply(&self, settings: &AppSettings) {
        self.gapless.store(settings.gapless, Ordering::Relaxed);
        self.crossfade_enabled
            .store(settings.crossfade_enabled, Ordering::Relaxed);
        let cf_ms = (settings.crossfade_secs.clamp(1.0, 10.0) * 1000.0).round() as u64;
        self.crossfade_ms.store(cf_ms, Ordering::Relaxed);
        let rg = match settings.replay_gain {
            ReplayGainMode::Off => 0,
            ReplayGainMode::Track => 1,
            ReplayGainMode::Album => 2,
        };
        self.replay_gain_mode.store(rg, Ordering::Relaxed);
        self.soft_fade.store(settings.soft_fade, Ordering::Relaxed);
    }

    fn replay_gain_mode(&self) -> ReplayGainMode {
        match self.replay_gain_mode.load(Ordering::Relaxed) {
            1 => ReplayGainMode::Track,
            2 => ReplayGainMode::Album,
            _ => ReplayGainMode::Off,
        }
    }
}

struct SharedFadeControl {
    command: AtomicU8,
    fade_out_done: AtomicBool,
}

impl Default for SharedFadeControl {
    fn default() -> Self {
        Self {
            command: AtomicU8::new(FADE_PLAYING),
            fade_out_done: AtomicBool::new(false),
        }
    }
}

type BoxedSource = Box<dyn Source<Item = f32> + Send>;

struct PreloadedTrack {
    token: u64,
    path: PathBuf,
    source: BoxedSource,
    duration_ms: u64,
    replay_gain: ReplayGainInfo,
}

/// Handle owned by the UI to control audio playback, gapless preloading, EQ, and visualizer tap.
pub struct AudioEngine {
    tx: mpsc::Sender<AudioCommand>,
    telemetry: Arc<SharedTelemetry>,
    eq: Arc<SharedEqParams>,
    audio_settings: Arc<SharedAudioSettings>,
    viz_tap: Arc<SharedAudioTap>,
}

impl AudioEngine {
    pub fn spawn(
        initial_eq: &EqSettings,
        initial_settings: &AppSettings,
        initial_volume: f32,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<AudioCommand>();
        let telemetry = Arc::new(SharedTelemetry::default());
        let eq = Arc::new(SharedEqParams::new(initial_eq));
        let audio_settings = Arc::new(SharedAudioSettings::new(initial_settings));
        let viz_tap = Arc::new(SharedAudioTap::new());

        let telemetry_bg = Arc::clone(&telemetry);
        let eq_bg = Arc::clone(&eq);
        let settings_bg = Arc::clone(&audio_settings);
        let viz_bg = Arc::clone(&viz_tap);

        thread::Builder::new()
            .name("peppermint-audio".into())
            .spawn(move || {
                run_audio_loop(rx, telemetry_bg, eq_bg, settings_bg, viz_bg, initial_volume)
            })
            .expect("failed to spawn audio thread");

        Self {
            tx,
            telemetry,
            eq,
            audio_settings,
            viz_tap,
        }
    }

    pub fn play(&self, path: PathBuf, duration_hint_secs: u64) {
        let _ = self.tx.send(AudioCommand::Play {
            path,
            duration_hint: duration_hint_secs,
            start_paused: false,
        });
    }

    pub fn load_paused(&self, path: PathBuf, duration_hint_secs: u64) {
        let _ = self.tx.send(AudioCommand::Play {
            path,
            duration_hint: duration_hint_secs,
            start_paused: true,
        });
    }

    pub fn preload_next(&self, next: Option<NextTrackSpec>) {
        let _ = self.tx.send(AudioCommand::PreloadNext(next));
    }

    pub fn pause(&self) {
        let _ = self.tx.send(AudioCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.tx.send(AudioCommand::Resume);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(AudioCommand::Stop);
    }

    pub fn seek(&self, pos: Duration) {
        let _ = self.tx.send(AudioCommand::Seek(pos));
    }

    pub fn set_volume(&self, vol: f32) {
        let _ = self.tx.send(AudioCommand::SetVolume(vol.clamp(0.0, 1.0)));
    }

    pub fn update_eq(&self, settings: &EqSettings) {
        self.eq.apply_settings(settings);
    }

    pub fn update_settings(&self, settings: &AppSettings) {
        self.audio_settings.apply(settings);
    }

    pub fn viz_tap(&self) -> &SharedAudioTap {
        &self.viz_tap
    }

    pub fn snapshot(&self) -> PlaybackSnapshot {
        let last_error = self
            .telemetry
            .last_error
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        PlaybackSnapshot {
            is_playing: self.telemetry.is_playing.load(Ordering::Relaxed),
            is_paused: self.telemetry.is_paused.load(Ordering::Relaxed),
            position_ms: self.telemetry.position_ms.load(Ordering::Relaxed),
            duration_ms: self.telemetry.duration_ms.load(Ordering::Relaxed),
            finished_generation: self.telemetry.finished_generation.load(Ordering::Relaxed),
            gapless_advanced_generation: self
                .telemetry
                .gapless_advanced_generation
                .load(Ordering::Relaxed),
            gapless_advanced_token: self
                .telemetry
                .gapless_advanced_token
                .load(Ordering::Relaxed),
            last_error,
        }
    }
}

fn set_error(telemetry: &SharedTelemetry, err: Option<String>) {
    if let Ok(mut guard) = telemetry.last_error.lock() {
        *guard = err;
    }
}

fn open_decoded_track(
    path: PathBuf,
    duration_hint: u64,
    token: u64,
) -> Result<PreloadedTrack, String> {
    let file = File::open(&path).map_err(|err| format!("Cannot open file: {err}"))?;
    let decoder = Decoder::new(BufReader::new(file))
        .map_err(|err| format!("Cannot decode audio: {err}"))?;
    let duration_ms = decoder
        .total_duration()
        .map(|d| d.as_millis() as u64)
        .unwrap_or(duration_hint.saturating_mul(1000));
    let replay_gain = read_replay_gain(&path);
    Ok(PreloadedTrack {
        token,
        path,
        source: Box::new(decoder.convert_samples::<f32>()),
        duration_ms,
        replay_gain,
    })
}

fn run_audio_loop(
    rx: mpsc::Receiver<AudioCommand>,
    telemetry: Arc<SharedTelemetry>,
    eq: Arc<SharedEqParams>,
    audio_settings: Arc<SharedAudioSettings>,
    viz_tap: Arc<SharedAudioTap>,
    initial_volume: f32,
) {
    let mut output = match OutputStream::try_default() {
        Ok(pair) => Some(pair),
        Err(err) => {
            set_error(&telemetry, Some(format!("Audio output unavailable: {err}")));
            None
        }
    };

    let mut sink: Option<Sink> = None;
    let mut volume = initial_volume.clamp(0.0, 1.0);
    let mut active_track = false;
    let mut pending_pause = false;
    let mut fade_ctrl = Arc::new(SharedFadeControl::default());
    let next_slot: Arc<Mutex<Option<PreloadedTrack>>> = Arc::new(Mutex::new(None));

    loop {
        match rx.recv_timeout(Duration::from_millis(15)) {
            Ok(cmd) => match cmd {
                AudioCommand::Play {
                    path,
                    duration_hint,
                    start_paused,
                } => {
                    if output.is_none() {
                        match OutputStream::try_default() {
                            Ok(pair) => {
                                output = Some(pair);
                                set_error(&telemetry, None);
                            }
                            Err(err) => {
                                set_error(
                                    &telemetry,
                                    Some(format!("Audio output unavailable: {err}")),
                                );
                                continue;
                            }
                        }
                    }

                    // Reuse preloaded decoder if the user manually clicked the already-queued next track!
                    let reused = next_slot.lock().ok().and_then(|mut guard| {
                        if guard.as_ref().is_some_and(|p| p.path == path) {
                            guard.take()
                        } else {
                            *guard = None;
                            None
                        }
                    });

                    let loaded = match reused {
                        Some(track) => Ok(track),
                        None => open_decoded_track(path, duration_hint, 0),
                    };

                    if let Some(old_sink) = sink.take() {
                        old_sink.stop();
                    }
                    pending_pause = false;
                    viz_tap.clear();

                    let track = match loaded {
                        Ok(t) => t,
                        Err(msg) => {
                            set_error(&telemetry, Some(msg));
                            active_track = false;
                            telemetry.is_playing.store(false, Ordering::Relaxed);
                            telemetry.is_paused.store(false, Ordering::Relaxed);
                            continue;
                        }
                    };

                    fade_ctrl = Arc::new(SharedFadeControl::default());
                    let duration_ms = track.duration_ms;

                    let seamless = SeamlessEqSource::new(
                        track,
                        Arc::clone(&next_slot),
                        Arc::clone(&eq),
                        Arc::clone(&audio_settings),
                        Arc::clone(&telemetry),
                        Arc::clone(&fade_ctrl),
                        Arc::clone(&viz_tap),
                    );

                    if let Some((_, ref handle)) = output {
                        match Sink::try_new(handle) {
                            Ok(new_sink) => {
                                new_sink.set_volume(volume);
                                new_sink.append(seamless);
                                if start_paused {
                                    new_sink.pause();
                                } else {
                                    new_sink.play();
                                }
                                sink = Some(new_sink);
                                active_track = true;
                                set_error(&telemetry, None);
                                telemetry.position_ms.store(0, Ordering::Relaxed);
                                telemetry.duration_ms.store(duration_ms, Ordering::Relaxed);
                                telemetry.is_playing.store(!start_paused, Ordering::Relaxed);
                                telemetry.is_paused.store(start_paused, Ordering::Relaxed);
                            }
                            Err(err) => {
                                set_error(&telemetry, Some(format!("Audio sink error: {err}")));
                            }
                        }
                    }
                }
                AudioCommand::PreloadNext(spec) => match spec {
                    None => {
                        if let Ok(mut guard) = next_slot.lock() {
                            *guard = None;
                        }
                    }
                    Some(spec) => {
                        let already_loaded = next_slot.lock().ok().is_some_and(|guard| {
                            guard
                                .as_ref()
                                .is_some_and(|p| p.token == spec.token && p.path == spec.path)
                        });
                        if !already_loaded {
                            match open_decoded_track(spec.path, spec.duration_hint, spec.token) {
                                Ok(preloaded) => {
                                    if let Ok(mut guard) = next_slot.lock() {
                                        *guard = Some(preloaded);
                                    }
                                }
                                Err(_) => {
                                    if let Ok(mut guard) = next_slot.lock() {
                                        *guard = None;
                                    }
                                }
                            }
                        }
                    }
                },
                AudioCommand::Pause => {
                    if let Some(ref s) = sink
                        && active_track
                    {
                        if audio_settings.soft_fade.load(Ordering::Relaxed)
                            && telemetry.is_playing.load(Ordering::Relaxed)
                        {
                            fade_ctrl.fade_out_done.store(false, Ordering::Relaxed);
                            fade_ctrl
                                .command
                                .store(FADE_OUT_TO_PAUSE, Ordering::Relaxed);
                            pending_pause = true;
                        } else {
                            s.pause();
                            pending_pause = false;
                        }
                        telemetry.is_playing.store(false, Ordering::Relaxed);
                        telemetry.is_paused.store(true, Ordering::Relaxed);
                    }
                }
                AudioCommand::Resume => {
                    if let Some(ref s) = sink
                        && active_track
                    {
                        pending_pause = false;
                        fade_ctrl.fade_out_done.store(false, Ordering::Relaxed);
                        fade_ctrl.command.store(FADE_PLAYING, Ordering::Relaxed);
                        s.play();
                        telemetry.is_playing.store(true, Ordering::Relaxed);
                        telemetry.is_paused.store(false, Ordering::Relaxed);
                    }
                }
                AudioCommand::Stop => {
                    if let Some(old_sink) = sink.take() {
                        old_sink.stop();
                    }
                    if let Ok(mut guard) = next_slot.lock() {
                        *guard = None;
                    }
                    pending_pause = false;
                    viz_tap.clear();
                    active_track = false;
                    telemetry.is_playing.store(false, Ordering::Relaxed);
                    telemetry.is_paused.store(false, Ordering::Relaxed);
                    telemetry.position_ms.store(0, Ordering::Relaxed);
                }
                AudioCommand::Seek(pos) => {
                    if let Some(ref s) = sink {
                        if let Err(err) = s.try_seek(pos) {
                            set_error(&telemetry, Some(format!("Seek not supported: {err}")));
                        } else {
                            telemetry
                                .position_ms
                                .store(pos.as_millis() as u64, Ordering::Relaxed);
                            set_error(&telemetry, None);
                        }
                    }
                }
                AudioCommand::SetVolume(v) => {
                    volume = v.clamp(0.0, 1.0);
                    if let Some(ref s) = sink {
                        s.set_volume(volume);
                    }
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if pending_pause
            && fade_ctrl.fade_out_done.load(Ordering::Relaxed)
            && let Some(ref s) = sink
        {
            s.pause();
            pending_pause = false;
        }

        if let Some(ref s) = sink
            && active_track
            && s.empty()
        {
            active_track = false;
            pending_pause = false;
            viz_tap.clear();
            telemetry.is_playing.store(false, Ordering::Relaxed);
            telemetry.is_paused.store(false, Ordering::Relaxed);
            telemetry
                .finished_generation
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Adapts any `BoxedSource` to a target `(out_sr, out_ch)` format while preserving `try_seek`.
/// When the source already matches `(out_sr, out_ch)`, audio passes through with zero resampling.
struct UnifiedTrack {
    source: BoxedSource,
    src_ch: usize,
    out_ch: usize,
    passthrough: bool,
    // Resampler state (used only when src_sr != out_sr || src_ch != out_ch)
    prev_frame: Vec<f32>,
    curr_frame: Vec<f32>,
    phase: f64,
    step: f64,
    initialized: bool,
    exhausted: bool,
}

impl UnifiedTrack {
    fn new(source: BoxedSource, out_sr: u32, out_ch: u16) -> Self {
        let src_sr = source.sample_rate().max(1);
        let src_ch = (source.channels() as usize).max(1);
        let out_sr = out_sr.max(1);
        let out_ch = (out_ch as usize).max(1);
        let passthrough = src_sr == out_sr && src_ch == out_ch;
        Self {
            source,
            src_ch,
            out_ch,
            passthrough,
            prev_frame: vec![0.0; out_ch],
            curr_frame: vec![0.0; out_ch],
            phase: 0.0,
            step: src_sr as f64 / out_sr as f64,
            initialized: false,
            exhausted: false,
        }
    }

    fn read_src_frame_mapped(&mut self, dst: &mut [f32]) -> bool {
        let mut raw = [0.0_f32; 8];
        let ch = self.src_ch.min(raw.len());
        let first = match self.source.next() {
            Some(s) => s,
            None => return false,
        };
        raw[0] = first;
        for slot in raw.iter_mut().take(ch).skip(1) {
            *slot = self.source.next().unwrap_or(0.0);
        }
        for _ in ch..self.src_ch {
            let _ = self.source.next();
        }

        if self.out_ch == ch {
            dst[..self.out_ch].copy_from_slice(&raw[..self.out_ch]);
        } else if ch == 1 {
            for slot in dst.iter_mut().take(self.out_ch) {
                *slot = raw[0];
            }
        } else if self.out_ch == 1 {
            let sum: f32 = raw[..ch].iter().sum();
            dst[0] = sum / ch as f32;
        } else {
            for (i, slot) in dst.iter_mut().take(self.out_ch).enumerate() {
                *slot = raw[i % ch];
            }
        }
        true
    }

    /// Reads one interleaved frame of `self.out_ch` samples into `out`. Returns `false` at EOF.
    fn next_frame(&mut self, out: &mut [f32]) -> bool {
        if self.passthrough {
            let first = match self.source.next() {
                Some(s) => s,
                None => return false,
            };
            out[0] = first;
            for slot in out.iter_mut().take(self.out_ch).skip(1) {
                *slot = self.source.next().unwrap_or(0.0);
            }
            return true;
        }

        if self.exhausted {
            return false;
        }

        if !self.initialized {
            let mut first = vec![0.0; self.out_ch];
            if !self.read_src_frame_mapped(&mut first) {
                self.exhausted = true;
                return false;
            }
            self.prev_frame.copy_from_slice(&first);
            let mut second = vec![0.0; self.out_ch];
            if self.read_src_frame_mapped(&mut second) {
                self.curr_frame.copy_from_slice(&second);
            } else {
                self.curr_frame.copy_from_slice(&first);
            }
            self.phase = 0.0;
            self.initialized = true;
        }

        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.prev_frame.copy_from_slice(&self.curr_frame);
            let mut next = vec![0.0; self.out_ch];
            if !self.read_src_frame_mapped(&mut next) {
                self.exhausted = true;
                return false;
            }
            self.curr_frame.copy_from_slice(&next);
        }

        let frac = self.phase as f32;
        let inv = 1.0 - frac;
        for i in 0..self.out_ch {
            out[i] = self.prev_frame[i] * inv + self.curr_frame[i] * frac;
        }
        self.phase += self.step;
        true
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let res = self.source.try_seek(pos);
        if res.is_ok() {
            self.initialized = false;
            self.exhausted = false;
            self.phase = 0.0;
        }
        res
    }
}

struct ActiveTrack {
    track: UnifiedTrack,
    duration_ms: u64,
    replay_gain: ReplayGainInfo,
    frames_played: u64,
}

struct CrossfadeIncoming {
    token: u64,
    track: UnifiedTrack,
    duration_ms: u64,
    replay_gain: ReplayGainInfo,
    frames_played: u64,
    total_fade_frames: u64,
}

/// Seamless gapless + crossfade + ReplayGain + soft-fade + 10-band EQ + FFT tap `Source`.
struct SeamlessEqSource {
    out_sr: u32,
    out_ch: u16,
    current: ActiveTrack,
    incoming: Option<CrossfadeIncoming>,
    next_slot: Arc<Mutex<Option<PreloadedTrack>>>,
    eq_params: Arc<SharedEqParams>,
    audio_settings: Arc<SharedAudioSettings>,
    telemetry: Arc<SharedTelemetry>,
    fade_ctrl: Arc<SharedFadeControl>,
    viz_tap: Arc<SharedAudioTap>,
    processor: GraphicEqProcessor,

    // Frame buffer so `Iterator::next` delivers interleaved samples cleanly
    frame_buf: Vec<f32>,
    frame_ch_idx: usize,
    frame_ready: bool,
    cur_scratch: Vec<f32>,
    inc_scratch: Vec<f32>,

    // Soft pause/resume gain envelope
    soft_gain: f32,
    frames_since_pos_sync: u32,
    mono_batch: [f32; 128],
    mono_len: usize,
    finished: bool,
}

impl SeamlessEqSource {
    fn new(
        initial: PreloadedTrack,
        next_slot: Arc<Mutex<Option<PreloadedTrack>>>,
        eq_params: Arc<SharedEqParams>,
        audio_settings: Arc<SharedAudioSettings>,
        telemetry: Arc<SharedTelemetry>,
        fade_ctrl: Arc<SharedFadeControl>,
        viz_tap: Arc<SharedAudioTap>,
    ) -> Self {
        let out_sr = initial.source.sample_rate().max(8_000);
        let out_ch = initial.source.channels().clamp(1, 2);
        let ch_usize = out_ch as usize;

        let mut processor = GraphicEqProcessor::new(out_sr, out_ch);
        processor.sync_from(&eq_params);

        let soft_start = if audio_settings.soft_fade.load(Ordering::Relaxed) {
            0.0
        } else {
            1.0
        };

        Self {
            out_sr,
            out_ch,
            current: ActiveTrack {
                track: UnifiedTrack::new(initial.source, out_sr, out_ch),
                duration_ms: initial.duration_ms,
                replay_gain: initial.replay_gain,
                frames_played: 0,
            },
            incoming: None,
            next_slot,
            eq_params,
            audio_settings,
            telemetry,
            fade_ctrl,
            viz_tap,
            processor,
            frame_buf: vec![0.0; ch_usize],
            frame_ch_idx: 0,
            frame_ready: false,
            cur_scratch: vec![0.0; ch_usize],
            inc_scratch: vec![0.0; ch_usize],
            soft_gain: soft_start,
            frames_since_pos_sync: 0,
            mono_batch: [0.0; 128],
            mono_len: 0,
            finished: false,
        }
    }

    fn take_preloaded(&self) -> Option<PreloadedTrack> {
        self.next_slot.try_lock().ok().and_then(|mut g| g.take())
    }

    fn promote_incoming(&mut self, inc: CrossfadeIncoming) {
        let pos_ms = (inc.frames_played.saturating_mul(1000)) / self.out_sr as u64;
        self.current = ActiveTrack {
            track: inc.track,
            duration_ms: inc.duration_ms,
            replay_gain: inc.replay_gain,
            frames_played: inc.frames_played,
        };
        self.telemetry
            .duration_ms
            .store(inc.duration_ms, Ordering::Relaxed);
        self.telemetry.position_ms.store(pos_ms, Ordering::Relaxed);
        self.telemetry
            .gapless_advanced_token
            .store(inc.token, Ordering::Relaxed);
        self.telemetry
            .gapless_advanced_generation
            .fetch_add(1, Ordering::Relaxed);
        self.frames_since_pos_sync = 0;
    }

    fn try_gapless_advance(&mut self) -> bool {
        let gapless = self.audio_settings.gapless.load(Ordering::Relaxed);
        let crossfade = self.audio_settings.crossfade_enabled.load(Ordering::Relaxed);
        if !gapless && !crossfade {
            return false;
        }
        let Some(next) = self.take_preloaded() else {
            return false;
        };
        let duration_ms = next.duration_ms;
        let token = next.token;
        self.current = ActiveTrack {
            track: UnifiedTrack::new(next.source, self.out_sr, self.out_ch),
            duration_ms,
            replay_gain: next.replay_gain,
            frames_played: 0,
        };
        self.telemetry
            .duration_ms
            .store(duration_ms, Ordering::Relaxed);
        self.telemetry.position_ms.store(0, Ordering::Relaxed);
        self.telemetry
            .gapless_advanced_token
            .store(token, Ordering::Relaxed);
        self.telemetry
            .gapless_advanced_generation
            .fetch_add(1, Ordering::Relaxed);
        self.frames_since_pos_sync = 0;
        true
    }

    fn maybe_start_crossfade(&mut self) {
        if self.incoming.is_some()
            || !self.audio_settings.crossfade_enabled.load(Ordering::Relaxed)
        {
            return;
        }
        let cf_ms = self.audio_settings.crossfade_ms.load(Ordering::Relaxed);
        if cf_ms < 200 || self.current.duration_ms <= cf_ms + 500 {
            return;
        }
        let elapsed_ms = (self.current.frames_played.saturating_mul(1000)) / self.out_sr as u64;
        let remaining_ms = self.current.duration_ms.saturating_sub(elapsed_ms);
        if remaining_ms > cf_ms {
            return;
        }
        let Some(next) = self.take_preloaded() else {
            return;
        };
        let total_fade_frames = ((remaining_ms.max(150) * self.out_sr as u64) / 1000).max(1);
        self.incoming = Some(CrossfadeIncoming {
            token: next.token,
            track: UnifiedTrack::new(next.source, self.out_sr, self.out_ch),
            duration_ms: next.duration_ms,
            replay_gain: next.replay_gain,
            frames_played: 0,
            total_fade_frames,
        });
    }

    /// Computes the next stereo/mono output frame into `self.frame_buf`.
    fn fill_next_frame(&mut self) -> bool {
        if self.finished {
            return false;
        }

        let ch = self.out_ch as usize;
        let rg_mode = self.audio_settings.replay_gain_mode();

        // Sync EQ params periodically (~every 64 frames)
        if self.current.frames_played & 63 == 0 {
            self.processor.sync_from(&self.eq_params);
        }

        // Check if we should begin crossfading into the preloaded track
        self.maybe_start_crossfade();

        let cur_rg = self.current.replay_gain.linear_gain(rg_mode);

        // Produce raw mixed frame into `self.cur_scratch`
        if let Some(mut inc) = self.incoming.take() {
            let cur_ok = self.current.track.next_frame(&mut self.cur_scratch);
            let inc_ok = inc.track.next_frame(&mut self.inc_scratch);
            let inc_rg = inc.replay_gain.linear_gain(rg_mode);

            if !cur_ok {
                // Current ended while crossfading — promote incoming seamlessly!
                if inc_ok {
                    inc.frames_played += 1;
                    for i in 0..ch {
                        self.cur_scratch[i] = self.inc_scratch[i] * inc_rg;
                    }
                    self.promote_incoming(inc);
                } else {
                    self.finished = true;
                    return false;
                }
            } else if !inc_ok {
                // Incoming was unexpectedly shorter than fade; continue current
                self.current.frames_played += 1;
                for i in 0..ch {
                    self.cur_scratch[i] *= cur_rg;
                }
            } else {
                let t = (inc.frames_played as f32 / inc.total_fade_frames as f32).clamp(0.0, 1.0);
                let out_g = (t * FRAC_PI_2).cos();
                let in_g = (t * FRAC_PI_2).sin();
                for i in 0..ch {
                    self.cur_scratch[i] =
                        self.cur_scratch[i] * cur_rg * out_g + self.inc_scratch[i] * inc_rg * in_g;
                }
                self.current.frames_played += 1;
                inc.frames_played += 1;

                if inc.frames_played >= inc.total_fade_frames {
                    self.promote_incoming(inc);
                } else {
                    self.incoming = Some(inc);
                }
            }
        } else {
            let mut got = self.current.track.next_frame(&mut self.cur_scratch);
            if !got {
                if self.try_gapless_advance() {
                    got = self.current.track.next_frame(&mut self.cur_scratch);
                }
                if !got {
                    self.finished = true;
                    return false;
                }
            }
            let gain = self.current.replay_gain.linear_gain(rg_mode);
            for i in 0..ch {
                self.cur_scratch[i] *= gain;
            }
            self.current.frames_played += 1;
        }

        // Update soft pause/resume fade gain envelope
        let soft_enabled = self.audio_settings.soft_fade.load(Ordering::Relaxed);
        let cmd = self.fade_ctrl.command.load(Ordering::Relaxed);
        if soft_enabled {
            let fade_frames = ((self.out_sr as f32) * 0.055).max(1.0);
            let step = 1.0 / fade_frames;
            if cmd == FADE_OUT_TO_PAUSE {
                self.soft_gain = (self.soft_gain - step).max(0.0);
                if self.soft_gain <= 0.0001 {
                    self.soft_gain = 0.0;
                    self.fade_ctrl.fade_out_done.store(true, Ordering::Relaxed);
                }
            } else if self.soft_gain < 1.0 {
                self.soft_gain = (self.soft_gain + step).min(1.0);
            }
        } else {
            self.soft_gain = 1.0;
            if cmd == FADE_OUT_TO_PAUSE {
                self.fade_ctrl.fade_out_done.store(true, Ordering::Relaxed);
            }
        }

        // Process through 10-band EQ + push mono mix to visualizer tap
        let mut mono_sum = 0.0_f32;
        for i in 0..ch {
            let processed = self.processor.process_sample(i, self.cur_scratch[i]) * self.soft_gain;
            self.frame_buf[i] = processed;
            mono_sum += processed;
        }

        self.mono_batch[self.mono_len] = mono_sum / ch as f32;
        self.mono_len += 1;
        if self.mono_len == self.mono_batch.len() {
            self.viz_tap.push_batch(&self.mono_batch, self.out_sr);
            self.mono_len = 0;
        }

        // Periodically publish exact track position_ms (~every 10 ms)
        self.frames_since_pos_sync += 1;
        if self.frames_since_pos_sync >= 512 {
            self.frames_since_pos_sync = 0;
            let pos_ms = (self.current.frames_played.saturating_mul(1000)) / self.out_sr as u64;
            self.telemetry.position_ms.store(pos_ms, Ordering::Relaxed);
        }

        self.frame_ch_idx = 0;
        self.frame_ready = true;
        true
    }
}

impl Iterator for SeamlessEqSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if !self.frame_ready && !self.fill_next_frame() {
            return None;
        }
        let sample = self.frame_buf[self.frame_ch_idx];
        self.frame_ch_idx += 1;
        if self.frame_ch_idx >= self.out_ch as usize {
            self.frame_ready = false;
        }
        Some(sample)
    }
}

impl Source for SeamlessEqSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.out_ch
    }

    fn sample_rate(&self) -> u32 {
        self.out_sr
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let res = self.current.track.try_seek(pos);
        if res.is_ok() {
            self.incoming = None;
            self.current.frames_played = (pos.as_secs_f64() * self.out_sr as f64) as u64;
            self.frames_since_pos_sync = 0;
            self.frame_ready = false;
            self.frame_ch_idx = 0;
            self.processor = GraphicEqProcessor::new(self.out_sr, self.out_ch);
            self.processor.sync_from(&self.eq_params);
            self.mono_len = 0;
            self.telemetry
                .position_ms
                .store(pos.as_millis() as u64, Ordering::Relaxed);
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConstSource {
        value: f32,
        remaining: usize,
        sr: u32,
        ch: u16,
    }

    impl Iterator for ConstSource {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            if self.remaining == 0 {
                return None;
            }
            self.remaining -= 1;
            Some(self.value)
        }
    }

    impl Source for ConstSource {
        fn current_frame_len(&self) -> Option<usize> {
            Some(self.remaining)
        }
        fn channels(&self) -> u16 {
            self.ch
        }
        fn sample_rate(&self) -> u32 {
            self.sr
        }
        fn total_duration(&self) -> Option<Duration> {
            let frames = self.remaining / self.ch as usize;
            Some(Duration::from_secs_f64(frames as f64 / self.sr as f64))
        }
    }

    fn make_track(value: f32, frames: usize, sr: u32, ch: u16, token: u64) -> PreloadedTrack {
        let duration_ms = ((frames as u64) * 1000) / sr as u64;
        PreloadedTrack {
            token,
            path: PathBuf::from(format!("track_{token}.flac")),
            source: Box::new(ConstSource {
                value,
                remaining: frames * ch as usize,
                sr,
                ch,
            }),
            duration_ms,
            replay_gain: ReplayGainInfo::default(),
        }
    }

    #[test]
    fn gapless_chains_two_tracks_with_zero_silence() {
        let eq = Arc::new(SharedEqParams::new(&EqSettings {
            enabled: false,
            ..Default::default()
        }));
        let mut app_settings = AppSettings::default();
        app_settings.gapless = true;
        app_settings.soft_fade = false;
        let audio_settings = Arc::new(SharedAudioSettings::new(&app_settings));
        let telemetry = Arc::new(SharedTelemetry::default());
        let fade_ctrl = Arc::new(SharedFadeControl::default());
        let viz_tap = Arc::new(SharedAudioTap::new());

        let first = make_track(0.25, 100, 44_100, 2, 1);
        let second = make_track(0.75, 100, 44_100, 2, 42);
        let next_slot = Arc::new(Mutex::new(Some(second)));

        let src = SeamlessEqSource::new(
            first,
            next_slot,
            eq,
            audio_settings,
            Arc::clone(&telemetry),
            fade_ctrl,
            viz_tap,
        );

        let samples: Vec<f32> = src.collect();
        // 100 stereo frames + 100 stereo frames = 400 samples, zero silence in between!
        assert_eq!(samples.len(), 400);
        assert!((samples[199] - 0.25).abs() < 1e-5);
        assert!((samples[200] - 0.75).abs() < 1e-5);
        assert_eq!(
            telemetry.gapless_advanced_generation.load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            telemetry.gapless_advanced_token.load(Ordering::Relaxed),
            42
        );
    }

    #[test]
    fn gapless_resamples_mismatched_sample_rate_and_channels() {
        let eq = Arc::new(SharedEqParams::new(&EqSettings {
            enabled: false,
            ..Default::default()
        }));
        let mut app_settings = AppSettings::default();
        app_settings.gapless = true;
        app_settings.soft_fade = false;
        let audio_settings = Arc::new(SharedAudioSettings::new(&app_settings));
        let telemetry = Arc::new(SharedTelemetry::default());
        let fade_ctrl = Arc::new(SharedFadeControl::default());
        let viz_tap = Arc::new(SharedAudioTap::new());

        // Track 1: 44.1 kHz stereo; Track 2: 48 kHz mono
        let first = make_track(0.2, 50, 44_100, 2, 1);
        let second = make_track(0.8, 480, 48_000, 1, 99);
        let next_slot = Arc::new(Mutex::new(Some(second)));

        let src = SeamlessEqSource::new(
            first,
            next_slot,
            eq,
            audio_settings,
            Arc::clone(&telemetry),
            fade_ctrl,
            viz_tap,
        );

        let samples: Vec<f32> = src.collect();
        // Track 1 = 100 samples, Track 2 (10 ms @ 48kHz -> ~441 frames * 2 ch = ~882 samples)
        assert!(samples.len() > 900);
        assert!((samples[99] - 0.2).abs() < 1e-5);
        assert!((samples[100] - 0.8).abs() < 1e-4);
        assert_eq!(
            telemetry.gapless_advanced_token.load(Ordering::Relaxed),
            99
        );
    }

    #[test]
    fn gapless_disabled_stops_at_track_end() {
        let eq = Arc::new(SharedEqParams::new(&EqSettings {
            enabled: false,
            ..Default::default()
        }));
        let mut app_settings = AppSettings::default();
        app_settings.gapless = false;
        app_settings.crossfade_enabled = false;
        app_settings.soft_fade = false;
        let audio_settings = Arc::new(SharedAudioSettings::new(&app_settings));
        let telemetry = Arc::new(SharedTelemetry::default());
        let fade_ctrl = Arc::new(SharedFadeControl::default());
        let viz_tap = Arc::new(SharedAudioTap::new());

        let first = make_track(0.25, 100, 44_100, 2, 1);
        let second = make_track(0.75, 100, 44_100, 2, 42);
        let next_slot = Arc::new(Mutex::new(Some(second)));

        let src = SeamlessEqSource::new(
            first,
            next_slot,
            eq,
            audio_settings,
            Arc::clone(&telemetry),
            fade_ctrl,
            viz_tap,
        );

        let samples: Vec<f32> = src.collect();
        assert_eq!(samples.len(), 200);
        assert_eq!(
            telemetry.gapless_advanced_generation.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn crossfade_blends_and_promotes_incoming_track() {
        let eq = Arc::new(SharedEqParams::new(&EqSettings {
            enabled: false,
            ..Default::default()
        }));
        let mut app_settings = AppSettings::default();
        app_settings.gapless = true;
        app_settings.crossfade_enabled = true;
        app_settings.crossfade_secs = 1.0;
        app_settings.soft_fade = false;
        let audio_settings = Arc::new(SharedAudioSettings::new(&app_settings));
        let telemetry = Arc::new(SharedTelemetry::default());
        let fade_ctrl = Arc::new(SharedFadeControl::default());
        let viz_tap = Arc::new(SharedAudioTap::new());

        // Use 8000 Hz stereo so 2 seconds = 16000 frames (fast test)
        let first = make_track(0.2, 16_000, 8_000, 2, 1);
        let second = make_track(0.8, 16_000, 8_000, 2, 77);
        let next_slot = Arc::new(Mutex::new(Some(second)));

        let src = SeamlessEqSource::new(
            first,
            next_slot,
            eq,
            audio_settings,
            Arc::clone(&telemetry),
            fade_ctrl,
            viz_tap,
        );

        let samples: Vec<f32> = src.collect();
        // 2s + 2s with 1s overlap = ~3s = ~24,000 stereo frames = ~48,000 samples
        assert!(samples.len() > 46_000 && samples.len() < 50_000);
        assert!((samples[10] - 0.2).abs() < 1e-4);
        assert!((samples[samples.len() - 10] - 0.8).abs() < 1e-4);
        assert_eq!(
            telemetry.gapless_advanced_generation.load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            telemetry.gapless_advanced_token.load(Ordering::Relaxed),
            77
        );
    }
}
