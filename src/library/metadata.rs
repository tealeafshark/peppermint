// SPDX-License-Identifier: GPL-3.0-only

//! Audio metadata reading and writing using `lofty` with graceful filename fallbacks.

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
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

/// Complete editable metadata + read-only file properties for the Tag Editor window.
#[derive(Debug, Clone)]
pub struct TrackTagData {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_number: String,
    pub year: String,
    pub genre: String,
    pub lyrics: String,
    pub artwork: Option<Vec<u8>>,
    pub artwork_modified: bool,
    pub format_label: String,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
    pub duration_secs: u64,
    pub file_size_bytes: u64,
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

/// Reads full editable metadata, lyrics, artwork, and technical file info for the Tag Editor.
pub fn read_full_track_tags(path: &Path) -> TrackTagData {
    let basic = read_track_metadata(path);
    let file_size_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let mut data = TrackTagData {
        path: path.to_path_buf(),
        title: basic.title,
        artist: basic.artist,
        album: basic.album,
        track_number: basic
            .track_number
            .map(|n| n.to_string())
            .unwrap_or_default(),
        year: String::new(),
        genre: String::new(),
        lyrics: String::new(),
        artwork: None,
        artwork_modified: false,
        format_label: basic.format_label,
        sample_rate: basic.sample_rate,
        bitrate: basic.bitrate,
        duration_secs: basic.duration_secs,
        file_size_bytes,
    };

    let Ok(tagged_file) = Probe::open(path).and_then(|p| p.read()) else {
        return data;
    };

    if let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    {
        if let Some(year) = tag.year() {
            data.year = year.to_string();
        }
        if let Some(genre) = tag.genre() {
            data.genre = genre.trim().to_string();
        }
        if let Some(lyrics) = tag.get_string(&ItemKey::Lyrics) {
            data.lyrics = lyrics.to_string();
        }
        if let Some(pic) = tag
            .pictures()
            .iter()
            .find(|p| p.pic_type() == PictureType::CoverFront)
            .or_else(|| tag.pictures().first())
        {
            data.artwork = Some(pic.data().to_vec());
        }
    }

    data
}

/// Writes edited tags, lyrics, and album artwork directly back into the audio file.
pub fn write_track_tags(data: &TrackTagData) -> Result<Track, String> {
    let mut tagged_file = Probe::open(&data.path)
        .and_then(|p| p.read())
        .map_err(|e| format!("Failed to read audio file: {e}"))?;

    let tag_type = tagged_file.primary_tag_type();
    if tagged_file.primary_tag_mut().is_none() {
        tagged_file.insert_tag(Tag::new(tag_type));
    }

    let tag = tagged_file
        .primary_tag_mut()
        .ok_or_else(|| "File format does not support writable tags".to_string())?;

    let title = data.title.trim();
    if title.is_empty() {
        tag.remove_title();
    } else {
        tag.set_title(title.to_string());
    }

    let artist = data.artist.trim();
    if artist.is_empty() {
        tag.remove_artist();
    } else {
        tag.set_artist(artist.to_string());
    }

    let album = data.album.trim();
    if album.is_empty() {
        tag.remove_album();
    } else {
        tag.set_album(album.to_string());
    }

    let track_str = data.track_number.trim();
    if track_str.is_empty() {
        tag.remove_track();
    } else if let Ok(num) = track_str.parse::<u32>() {
        tag.set_track(num);
    } else {
        return Err("Track # must be a valid whole number".into());
    }

    let year_str = data.year.trim();
    if year_str.is_empty() {
        tag.remove_year();
    } else if let Ok(yr) = year_str.parse::<u32>() {
        tag.set_year(yr);
    } else {
        return Err("Year must be a valid number (e.g. 2026)".into());
    }

    let genre = data.genre.trim();
    if genre.is_empty() {
        tag.remove_genre();
    } else {
        tag.set_genre(genre.to_string());
    }

    let lyrics = data.lyrics.trim();
    if lyrics.is_empty() {
        tag.remove_key(&ItemKey::Lyrics);
    } else {
        tag.insert_text(ItemKey::Lyrics, data.lyrics.clone());
    }

    if data.artwork_modified {
        tag.remove_picture_type(PictureType::CoverFront);
        if let Some(ref bytes) = data.artwork {
            let mut picture = Picture::from_reader(&mut Cursor::new(bytes))
                .unwrap_or_else(|_| {
                    let mime = detect_image_mime(bytes);
                    Picture::new_unchecked(PictureType::CoverFront, Some(mime), None, bytes.clone())
                });
            picture.set_pic_type(PictureType::CoverFront);
            tag.push_picture(picture);
        } else {
            // Clear remaining cover artwork slots if user explicitly removed art
            while !tag.pictures().is_empty() {
                let _ = tag.remove_picture(0);
            }
        }
    }

    tag.save_to_path(&data.path, WriteOptions::default())
        .map_err(|e| format!("Failed to save tags (check file write permissions): {e}"))?;

    Ok(read_track_metadata(&data.path))
}

fn detect_image_mime(bytes: &[u8]) -> MimeType {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        MimeType::Png
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        MimeType::Jpeg
    } else if bytes.starts_with(b"GIF8") {
        MimeType::Gif
    } else if bytes.starts_with(b"BM") {
        MimeType::Bmp
    } else {
        MimeType::Png
    }
}

/// Extracts raw embedded album artwork bytes (JPEG/PNG) if present in the audio file.
pub fn read_embedded_artwork(path: &Path) -> Option<Vec<u8>> {
    let tagged_file = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())?;
    tag.pictures()
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first())
        .map(|pic| pic.data().to_vec())
}

/// Extracts raw embedded lyrics string from the audio file if present.
pub fn read_embedded_lyrics(path: &Path) -> Option<String> {
    let tagged_file = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())?;
    tag.get_string(&ItemKey::Lyrics).map(ToString::to_string)
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

pub fn format_file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}
