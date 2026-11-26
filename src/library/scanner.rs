// SPDX-License-Identifier: GPL-3.0-only

//! Recursive local music folder scanner.

use crate::library::metadata::{Track, read_track_metadata};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "wav", "m4a", "aac", "aiff", "aif",
];

pub fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Expands a list of dropped or selected paths (files and/or directories) into tracks.
pub fn collect_tracks_from_paths(paths: &[PathBuf]) -> Vec<Track> {
    let mut audio_files = Vec::new();
    for path in paths {
        if path.is_file() {
            if is_supported_audio(path) {
                audio_files.push(path.clone());
            }
        } else if path.is_dir() {
            for entry in WalkDir::new(path)
                .follow_links(true)
                .into_iter()
                .filter_map(Result::ok)
            {
                let p = entry.path();
                if p.is_file() && is_supported_audio(p) {
                    audio_files.push(p.to_path_buf());
                }
            }
        }
    }
    audio_files.sort();
    audio_files
        .iter()
        .map(|p| read_track_metadata(p))
        .collect()
}

/// Scans all configured library directories and returns sorted tracks.
pub fn scan_library_folders(folders: &[PathBuf]) -> Vec<Track> {
    let mut tracks = collect_tracks_from_paths(folders);
    tracks.sort_by(|a, b| {
        a.artist
            .to_lowercase()
            .cmp(&b.artist.to_lowercase())
            .then_with(|| a.album.to_lowercase().cmp(&b.album.to_lowercase()))
            .then_with(|| a.track_number.unwrap_or(u32::MAX).cmp(&b.track_number.unwrap_or(u32::MAX)))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    tracks
}
