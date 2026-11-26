// SPDX-License-Identifier: GPL-3.0-only

//! Dedicated pure-Rust audio playback engine using `rodio` + `symphonia` with real-time 10-band EQ
//! and live FFT audio tap for the background visualizer.

use crate::audio::eq::{EqSettings, GraphicEqProcessor, SharedEqParams};
use crate::audio::visualizer::SharedAudioTap;
use rodio::source::SeekError;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

/// Commands sent from the UI thread to the audio engine thread.
#[derive(Debug)]
pub enum AudioCommand {
    Play { path: PathBuf, duration_hint: u64 },
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
    pub finished_generation: u64,
    pub last_error: Option<String>,
}

struct SharedTelemetry {
    is_playing: AtomicBool,
    is_paused: AtomicBool,
    position_ms: AtomicU64,
    duration_ms: AtomicU64,
    finished_generation: AtomicU64,
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
            last_error: Mutex::new(None),
        }
    }
}

/// Handle owned by the UI to control audio playback, real-time EQ, and read visualizer audio.
pub struct AudioEngine {
    tx: mpsc::Sender<AudioCommand>,
    telemetry: Arc<SharedTelemetry>,
    eq: Arc<SharedEqParams>,
    viz_tap: Arc<SharedAudioTap>,
}

impl AudioEngine {
    pub fn spawn(initial_eq: &EqSettings, initial_volume: f32) -> Self {
        let (tx, rx) = mpsc::channel::<AudioCommand>();
        let telemetry = Arc::new(SharedTelemetry::default());
        let eq = Arc::new(SharedEqParams::new(initial_eq));
        let viz_tap = Arc::new(SharedAudioTap::new());

        let telemetry_bg = Arc::clone(&telemetry);
        let eq_bg = Arc::clone(&eq);
        let viz_bg = Arc::clone(&viz_tap);

        thread::Builder::new()
            .name("peppermint-audio".into())
            .spawn(move || run_audio_loop(rx, telemetry_bg, eq_bg, viz_bg, initial_volume))
            .expect("failed to spawn audio thread");

        Self {
            tx,
            telemetry,
            eq,
            viz_tap,
        }
    }

    pub fn play(&self, path: PathBuf, duration_hint_secs: u64) {
        let _ = self.tx.send(AudioCommand::Play {
            path,
            duration_hint: duration_hint_secs,
        });
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
            last_error,
        }
    }
}

fn set_error(telemetry: &SharedTelemetry, err: Option<String>) {
    if let Ok(mut guard) = telemetry.last_error.lock() {
        *guard = err;
    }
}

fn run_audio_loop(
    rx: mpsc::Receiver<AudioCommand>,
    telemetry: Arc<SharedTelemetry>,
    eq: Arc<SharedEqParams>,
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

    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(cmd) => match cmd {
                AudioCommand::Play {
                    path,
                    duration_hint,
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

                    if let Some(old_sink) = sink.take() {
                        old_sink.stop();
                    }
                    viz_tap.clear();

                    let file = match File::open(&path) {
                        Ok(f) => f,
                        Err(err) => {
                            set_error(&telemetry, Some(format!("Cannot open file: {err}")));
                            active_track = false;
                            telemetry.is_playing.store(false, Ordering::Relaxed);
                            telemetry.is_paused.store(false, Ordering::Relaxed);
                            continue;
                        }
                    };

                    let decoder = match Decoder::new(BufReader::new(file)) {
                        Ok(d) => d,
                        Err(err) => {
                            set_error(&telemetry, Some(format!("Cannot decode audio: {err}")));
                            active_track = false;
                            telemetry.is_playing.store(false, Ordering::Relaxed);
                            telemetry.is_paused.store(false, Ordering::Relaxed);
                            continue;
                        }
                    };

                    let decoded_duration_ms = decoder
                        .total_duration()
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(duration_hint * 1000);

                    let eq_source = EqSource::new(
                        decoder.convert_samples::<f32>(),
                        Arc::clone(&eq),
                        Arc::clone(&viz_tap),
                    );

                    if let Some((_, ref handle)) = output {
                        match Sink::try_new(handle) {
                            Ok(new_sink) => {
                                new_sink.set_volume(volume);
                                new_sink.append(eq_source);
                                new_sink.play();
                                sink = Some(new_sink);
                                active_track = true;
                                set_error(&telemetry, None);
                                telemetry.position_ms.store(0, Ordering::Relaxed);
                                telemetry
                                    .duration_ms
                                    .store(decoded_duration_ms, Ordering::Relaxed);
                                telemetry.is_playing.store(true, Ordering::Relaxed);
                                telemetry.is_paused.store(false, Ordering::Relaxed);
                            }
                            Err(err) => {
                                set_error(&telemetry, Some(format!("Audio sink error: {err}")));
                            }
                        }
                    }
                }
                AudioCommand::Pause => {
                    if let Some(ref s) = sink {
                        s.pause();
                        telemetry.is_playing.store(false, Ordering::Relaxed);
                        telemetry.is_paused.store(true, Ordering::Relaxed);
                    }
                }
                AudioCommand::Resume => {
                    if let Some(ref s) = sink {
                        s.play();
                        telemetry.is_playing.store(true, Ordering::Relaxed);
                        telemetry.is_paused.store(false, Ordering::Relaxed);
                    }
                }
                AudioCommand::Stop => {
                    if let Some(old_sink) = sink.take() {
                        old_sink.stop();
                    }
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

        if let Some(ref s) = sink
            && active_track
        {
            if s.empty() {
                active_track = false;
                viz_tap.clear();
                telemetry.is_playing.store(false, Ordering::Relaxed);
                telemetry.is_paused.store(false, Ordering::Relaxed);
                telemetry
                    .finished_generation
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                let pos_ms = s.get_pos().as_millis() as u64;
                telemetry.position_ms.store(pos_ms, Ordering::Relaxed);
            }
        }
    }
}

/// Real-time 10-band EQ wrapper around any `rodio::Source<Item = f32>` that also taps mono audio
/// into `SharedAudioTap` for the live background visualizer.
struct EqSource<S> {
    inner: S,
    eq_params: Arc<SharedEqParams>,
    viz_tap: Arc<SharedAudioTap>,
    processor: GraphicEqProcessor,
    sample_index: usize,
    channels: usize,
    sample_rate: u32,
    frame_sum: f32,
    mono_batch: [f32; 128],
    mono_len: usize,
}

impl<S> EqSource<S>
where
    S: Source<Item = f32>,
{
    fn new(inner: S, eq_params: Arc<SharedEqParams>, viz_tap: Arc<SharedAudioTap>) -> Self {
        let sr = inner.sample_rate();
        let ch = inner.channels();
        let mut processor = GraphicEqProcessor::new(sr, ch);
        processor.sync_from(&eq_params);
        Self {
            inner,
            eq_params,
            viz_tap,
            processor,
            sample_index: 0,
            channels: (ch as usize).max(1),
            sample_rate: sr,
            frame_sum: 0.0,
            mono_batch: [0.0; 128],
            mono_len: 0,
        }
    }
}

impl<S> Iterator for EqSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        if self.sample_index & 63 == 0 {
            self.processor.sync_from(&self.eq_params);
        }
        let ch_idx = self.sample_index % self.channels;
        self.sample_index = self.sample_index.wrapping_add(1);
        let processed = self.processor.process_sample(ch_idx, sample);

        // Mix channels down to mono frames and batch-push into the visualizer tap.
        self.frame_sum += processed;
        if ch_idx + 1 == self.channels {
            let mono = self.frame_sum / self.channels as f32;
            self.frame_sum = 0.0;
            self.mono_batch[self.mono_len] = mono;
            self.mono_len += 1;
            if self.mono_len == self.mono_batch.len() {
                self.viz_tap.push_batch(&self.mono_batch, self.sample_rate);
                self.mono_len = 0;
            }
        }

        Some(processed)
    }
}

impl<S> Source for EqSource<S>
where
    S: Source<Item = f32>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let res = self.inner.try_seek(pos);
        if res.is_ok() {
            self.processor =
                GraphicEqProcessor::new(self.inner.sample_rate(), self.inner.channels());
            self.processor.sync_from(&self.eq_params);
            self.sample_index = 0;
            self.frame_sum = 0.0;
            self.mono_len = 0;
        }
        res
    }
}
