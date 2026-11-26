// SPDX-License-Identifier: GPL-3.0-only

//! Local music library browser shown inside the right panel when toggled open.

use crate::app::{LibraryViewMode, Message};
use crate::library::metadata::{Track, format_duration};
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn view<'a>(
    library_tracks: &'a [Track],
    library_folders: &'a [PathBuf],
    search_query: &'a str,
    view_mode: LibraryViewMode,
    is_scanning: bool,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    // Library header with folder controls and a quick Back-to-Playlist button
    let header = widget::row::with_capacity(5)
        .push(widget::text::title4("Local Library"))
        .push(widget::space::horizontal())
        .push(
            widget::button::icon(widget::icon::from_name("folder-new-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::OpenAddLibraryFolderDialog)
                .tooltip("Add Music Folder"),
        )
        .push(
            widget::button::icon(widget::icon::from_name("view-refresh-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::RescanLibrary)
                .tooltip("Rescan Library Folders"),
        )
        .push(
            widget::button::standard("Done")
                .on_press(Message::ToggleLibraryPanel)
                .tooltip("Return to Playlist (Ctrl+L)"),
        )
        .spacing(4)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let search = widget::text_input("Search local library…", search_query)
        .on_input(Message::SetLibrarySearch)
        .width(Length::Fill);

    let mode_btn = |label: &'static str, mode: LibraryViewMode| {
        widget::button::text(label)
            .class(if view_mode == mode {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Standard
            })
            .padding([4, 8])
            .on_press(Message::SetLibraryViewMode(mode))
    };

    let modes = widget::row::with_capacity(4)
        .push(mode_btn("Tracks", LibraryViewMode::Tracks))
        .push(mode_btn("Albums", LibraryViewMode::Albums))
        .push(mode_btn("Artists", LibraryViewMode::Artists))
        .push(mode_btn("Folders", LibraryViewMode::Folders))
        .spacing(4)
        .width(Length::Fill);

    let q = search_query.trim().to_lowercase();
    let filtered: Vec<&Track> = library_tracks
        .iter()
        .filter(|t| {
            q.is_empty()
                || t.title.to_lowercase().contains(&q)
                || t.artist.to_lowercase().contains(&q)
                || t.album.to_lowercase().contains(&q)
        })
        .collect();

    let list_body: Element<'a, Message> = if is_scanning {
        widget::container(widget::text::body("Scanning music folders…"))
            .padding(spacing.space_m)
            .into()
    } else if library_folders.is_empty() {
        let empty = widget::column::with_capacity(2)
            .push(widget::text::body("No music folders configured."))
            .push(
                widget::button::suggested("Add Music Folder")
                    .on_press(Message::OpenAddLibraryFolderDialog),
            )
            .spacing(spacing.space_s)
            .align_x(Alignment::Center);
        widget::container(empty)
            .padding(spacing.space_m)
            .width(Length::Fill)
            .into()
    } else {
        match view_mode {
            LibraryViewMode::Tracks => render_tracks_list(&filtered),
            LibraryViewMode::Albums => render_albums_list(&filtered),
            LibraryViewMode::Artists => render_artists_list(&filtered),
            LibraryViewMode::Folders => render_folders_list(library_folders),
        }
    };

    let footer_stats = widget::text::caption(format!(
        "{} tracks in {} {}",
        library_tracks.len(),
        library_folders.len(),
        if library_folders.len() == 1 {
            "folder"
        } else {
            "folders"
        }
    ));

    widget::column::with_capacity(6)
        .push(header)
        .push(search)
        .push(modes)
        .push(widget::divider::horizontal::light())
        .push(widget::scrollable(list_body).height(Length::Fill))
        .push(footer_stats)
        .spacing(spacing.space_xs)
        .padding([10, 10, 10, 10])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn render_tracks_list<'a>(tracks: &[&'a Track]) -> Element<'a, Message> {
    if tracks.is_empty() {
        return widget::container(widget::text::caption("No matching tracks."))
            .padding(12)
            .into();
    }

    let limit = 400usize;
    let mut col = widget::column::with_capacity(tracks.len().min(limit) + 1).spacing(3);

    for track in tracks.iter().take(limit) {
        let track_clone_add = (*track).clone();
        let track_clone_play = (*track).clone();

        let text_col = widget::column::with_capacity(2)
            .push(widget::text::body(&track.title).size(13.0))
            .push(
                widget::text::caption(format!(
                    "{} • {}",
                    track.display_artist(),
                    format_duration(track.duration_secs)
                ))
                .size(11.0),
            )
            .width(Length::Fill);

        let row = widget::row::with_capacity(2)
            .push(
                widget::mouse_area(text_col)
                    .on_double_click(Message::AddTrackAndPlay(track_clone_play)),
            )
            .push(
                widget::button::icon(widget::icon::from_name("list-add-symbolic"))
                    .icon_size(14)
                    .padding(4)
                    .on_press(Message::AddTracksToActivePlaylist(vec![track_clone_add]))
                    .tooltip("Append to Active Playlist"),
            )
            .spacing(4)
            .align_y(Alignment::Center)
            .padding([5, 8]);

        col = col.push(
            widget::container(row)
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        );
    }

    if tracks.len() > limit {
        col = col.push(
            widget::text::caption(format!(
                "Showing first {limit} of {} tracks — use search to filter.",
                tracks.len()
            ))
            .size(11.0),
        );
    }

    col.into()
}

fn render_albums_list<'a>(tracks: &[&'a Track]) -> Element<'a, Message> {
    let mut by_album: BTreeMap<(String, String), Vec<Track>> = BTreeMap::new();
    for track in tracks {
        let key = (
            track.display_album().to_string(),
            track.display_artist().to_string(),
        );
        by_album.entry(key).or_default().push((*track).clone());
    }

    if by_album.is_empty() {
        return widget::container(widget::text::caption("No albums found."))
            .padding(12)
            .into();
    }

    let mut col = widget::column::with_capacity(by_album.len()).spacing(4);
    for ((album, artist), album_tracks) in by_album {
        let count = album_tracks.len();
        let info = widget::column::with_capacity(2)
            .push(widget::text::body(album).size(13.0))
            .push(widget::text::caption(format!("{artist} • {count} tracks")).size(11.0))
            .width(Length::Fill);

        let row = widget::row::with_capacity(2)
            .push(info)
            .push(
                widget::button::icon(widget::icon::from_name("list-add-symbolic"))
                    .icon_size(14)
                    .padding(4)
                    .on_press(Message::AddTracksToActivePlaylist(album_tracks))
                    .tooltip("Add Entire Album to Active Playlist"),
            )
            .spacing(4)
            .align_y(Alignment::Center)
            .padding([6, 8]);

        col = col.push(
            widget::container(row)
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        );
    }

    col.into()
}

fn render_artists_list<'a>(tracks: &[&'a Track]) -> Element<'a, Message> {
    let mut by_artist: BTreeMap<String, Vec<Track>> = BTreeMap::new();
    for track in tracks {
        by_artist
            .entry(track.display_artist().to_string())
            .or_default()
            .push((*track).clone());
    }

    if by_artist.is_empty() {
        return widget::container(widget::text::caption("No artists found."))
            .padding(12)
            .into();
    }

    let mut col = widget::column::with_capacity(by_artist.len()).spacing(4);
    for (artist, artist_tracks) in by_artist {
        let count = artist_tracks.len();
        let info = widget::column::with_capacity(2)
            .push(widget::text::body(artist).size(13.0))
            .push(widget::text::caption(format!("{count} tracks")).size(11.0))
            .width(Length::Fill);

        let row = widget::row::with_capacity(2)
            .push(info)
            .push(
                widget::button::icon(widget::icon::from_name("list-add-symbolic"))
                    .icon_size(14)
                    .padding(4)
                    .on_press(Message::AddTracksToActivePlaylist(artist_tracks))
                    .tooltip("Add All Tracks by Artist to Active Playlist"),
            )
            .spacing(4)
            .align_y(Alignment::Center)
            .padding([6, 8]);

        col = col.push(
            widget::container(row)
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        );
    }

    col.into()
}

fn render_folders_list<'a>(folders: &'a [PathBuf]) -> Element<'a, Message> {
    let mut col = widget::column::with_capacity(folders.len() + 1).spacing(4);
    for (idx, folder) in folders.iter().enumerate() {
        let name = folder
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Music Folder");
        let full = folder.display().to_string();

        let info = widget::column::with_capacity(2)
            .push(widget::text::body(name).size(13.0))
            .push(widget::text::caption(full).size(11.0))
            .width(Length::Fill);

        let row = widget::row::with_capacity(3)
            .push(info)
            .push(
                widget::button::icon(widget::icon::from_name("list-add-symbolic"))
                    .icon_size(14)
                    .padding(4)
                    .on_press(Message::AddPathsToActivePlaylist(vec![folder.clone()]))
                    .tooltip("Append Folder to Active Playlist"),
            )
            .push(
                widget::button::icon(widget::icon::from_name("user-trash-symbolic"))
                    .icon_size(14)
                    .padding(4)
                    .on_press(Message::RemoveLibraryFolder(idx))
                    .tooltip("Remove Folder from Library"),
            )
            .spacing(4)
            .align_y(Alignment::Center)
            .padding([6, 8]);

        col = col.push(
            widget::container(row)
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        );
    }

    col.into()
}
