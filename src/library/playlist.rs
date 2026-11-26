// SPDX-License-Identifier: GPL-3.0-only

//! Multi-tab playlist data model, M3U/M3U8 import/export, and state persistence.

use crate::audio::eq::EqSettings;
use crate::audio::visualizer::VizMode;
use crate::library::metadata::{Track, read_track_metadata};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat: Off",
            Self::All => "Repeat: All",
            Self::One => "Repeat: One",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistTab {
    pub id: u64,
    pub name: String,
    pub tracks: Vec<Track>,
}

impl PlaylistTab {
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            tracks: Vec::new(),
        }
    }

    pub fn total_duration_secs(&self) -> u64 {
        self.tracks.iter().map(|t| t.duration_secs).sum()
    }

    /// Moves a track from `from_idx` to `to_idx` (final index in the resulting vec).
    pub fn move_track(&mut self, from_idx: usize, to_idx: usize) -> bool {
        if from_idx >= self.tracks.len() || to_idx >= self.tracks.len() || from_idx == to_idx {
            return false;
        }
        let item = self.tracks.remove(from_idx);
        self.tracks.insert(to_idx, item);
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPersistence {
    pub tabs: Vec<PlaylistTab>,
    pub active_tab_id: u64,
    pub next_tab_id: u64,
    pub library_folders: Vec<PathBuf>,
    pub volume: f32,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub eq: EqSettings,
    pub show_library: bool,
    pub show_eq: bool,
    #[serde(default)]
    pub viz_mode: VizMode,
}

impl Default for AppPersistence {
    fn default() -> Self {
        let mut library_folders = Vec::new();
        if let Some(music_dir) = dirs::audio_dir()
            && music_dir.exists()
        {
            library_folders.push(music_dir);
        }
        Self {
            tabs: vec![
                PlaylistTab::new(1, "Playlist 1"),
                PlaylistTab::new(2, "Favorites"),
            ],
            active_tab_id: 1,
            next_tab_id: 3,
            library_folders,
            volume: 0.85,
            muted: false,
            shuffle: false,
            repeat: RepeatMode::Off,
            eq: EqSettings::default(),
            show_library: false,
            show_eq: false,
            viz_mode: VizMode::AuroraOrbit,
        }
    }
}

pub fn state_file_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("peppermint").join("state.json"))
}

pub fn load_state() -> AppPersistence {
    let Some(path) = state_file_path() else {
        return AppPersistence::default();
    };
    let Ok(bytes) = fs::read(&path) else {
        return AppPersistence::default();
    };
    let mut state: AppPersistence = serde_json::from_slice(&bytes).unwrap_or_default();
    if state.tabs.is_empty() {
        state.tabs.push(PlaylistTab::new(1, "Playlist 1"));
        state.active_tab_id = 1;
        state.next_tab_id = state.next_tab_id.max(2);
    }
    if !state.tabs.iter().any(|t| t.id == state.active_tab_id) {
        state.active_tab_id = state.tabs[0].id;
    }
    state
}

pub fn save_state(state: &AppPersistence) {
    let Some(path) = state_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, json);
    }
}

/// Imports tracks from an `.m3u` or `.m3u8` file.
pub fn import_m3u(path: &Path) -> Result<Vec<Track>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read playlist: {e}"))?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tracks = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let resolved = if let Some(stripped) = trimmed.strip_prefix("file://") {
            PathBuf::from(percent_decode(stripped))
        } else {
            let p = PathBuf::from(trimmed);
            if p.is_absolute() {
                p
            } else {
                base_dir.join(p)
            }
        };
        if resolved.is_file() {
            tracks.push(read_track_metadata(&resolved));
        }
    }

    Ok(tracks)
}

/// Exports a playlist tab to an `.m3u8` file.
pub fn export_m3u8(path: &Path, tracks: &[Track]) -> Result<(), String> {
    let mut out = String::from("#EXTM3U\n");
    for track in tracks {
        let artist_prefix = if track.artist.is_empty() {
            String::new()
        } else {
            format!("{} - ", track.artist)
        };
        out.push_str(&format!(
            "#EXTINF:{},{}{}\n{}\n",
            track.duration_secs,
            artist_prefix,
            track.title,
            track.path.display()
        ));
    }
    fs::write(path, out).map_err(|e| format!("Failed to write playlist: {e}"))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
                && let Ok(val) = u8::from_str_radix(hex, 16)
            {
                out.push(val);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
