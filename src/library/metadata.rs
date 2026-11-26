// SPDX-License-Identifier: GPL-3.0-only

//! Audio metadata extraction using `lofty` with graceful filename fallbacks.

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TRACK_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_track_id() -> u64 {
    NEXT_TRACK_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    /// Unique runtime ID per playlist entry so duplicate files can be dragged/selected independently.
    #[serde(default = "next_track_id")]
    pub id: u64,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_number: Option<u32>,
    pub duration_secs: u64,
    pub format_label: String,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
}

impl Track {
    /// Creates a new copy with a fresh unique playlist-entry ID.
    pub fn clone_with_new_id(&self) -> Self {
        let mut copy = self.clone();
        copy.id = next_track_id();
        copy
    }

    pub fn display_artist(&self) -> &str {
        if self.artist.is_empty() {
            "Unknown Artist"
        } else {
            &self.artist
        }
    }

    pub fn display_album(&self) -> &str {
        if self.album.is_empty() {
            "Unknown Album"
        } else {
            &self.album
        }
    }

    pub fn tech_badge(&self) -> String {
        let mut parts = Vec::new();
        if !self.format_label.is_empty() {
            parts.push(self.format_label.clone());
        }
        if let Some(sr) = self.sample_rate {
            let khz = f64::from(sr) / 1000.0;
            if (khz - khz.round()).abs() < 0.05 {
                parts.push(format!("{:.0} kHz", khz));
            } else {
                parts.push(format!("{:.1} kHz", khz));
            }
        }
        if let Some(br) = self.bitrate
            && br > 0
        {
            parts.push(format!("{br} kbps"));
        }
        parts.join(" • ")
    }
}

/// Reads metadata tags and audio properties from a local file.
pub fn read_track_metadata(path: &Path) -> Track {
    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown Track")
        .to_string();

    let format_label = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_uppercase())
        .unwrap_or_default();

    let mut track = Track {
        id: next_track_id(),
        path: path.to_path_buf(),
        title: fallback_title,
        artist: String::new(),
        album: String::new(),
        track_number: None,
        duration_secs: 0,
        format_label,
        sample_rate: None,
        bitrate: None,
    };

    let Ok(tagged_file) = Probe::open(path).and_then(|p| p.read()) else {
        return track;
    };

    let props = tagged_file.properties();
    track.duration_secs = props.duration().as_secs();
    track.sample_rate = props.sample_rate();
    track.bitrate = props.audio_bitrate();

    if let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    {
        if let Some(title) = tag.title() {
            let t = title.trim();
            if !t.is_empty() {
                track.title = t.to_string();
            }
        }
        if let Some(artist) = tag.artist() {
            track.artist = artist.trim().to_string();
        }
        if let Some(album) = tag.album() {
            track.album = album.trim().to_string();
        }
        track.track_number = tag.track();
    }

    track
}

/// Extracts raw embedded album artwork bytes (JPEG/PNG) if present in the audio file.
pub fn read_embedded_artwork(path: &Path) -> Option<Vec<u8>> {
    let tagged_file = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())?;
    tag.pictures().first().map(|pic| pic.data().to_vec())
}

pub fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem = secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{rem:02}")
    } else {
        format!("{mins}:{rem:02}")
    }
}
