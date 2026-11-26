// SPDX-License-Identifier: GPL-3.0-only

//! Compact, easy-to-access multi-tab playlist header for the right-hand Playlist column.

use crate::app::{Message, SortOrder};
use crate::library::playlist::PlaylistTab;
use cosmic::iced::widget::scrollable::{Direction, Scrollbar};
use cosmic::iced::{Alignment, Border, Color, Length};
use cosmic::prelude::*;
use cosmic::widget;

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    tabs: &'a [PlaylistTab],
    active_tab_id: u64,
    playing_tab_id: Option<u64>,
    is_playing: bool,
    renaming_tab: Option<(u64, &'a str)>,
    show_library: bool,
) -> Element<'a, Message> {
    // Top strip: Horizontal scrollable playlist tabs + "+" button
    let mut tab_row = widget::row::with_capacity(tabs.len() + 1)
        .spacing(4)
        .align_y(Alignment::Center);

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = tab.id == active_tab_id && !show_library;
        let owns_playback = playing_tab_id == Some(tab.id);

        if let Some((renaming_id, draft)) = renaming_tab
            && renaming_id == tab.id
        {
            let input = widget::text_input("Name", draft)
                .on_input(Message::RenameTabInput)
                .on_submit(|_| Message::CommitRenameTab)
                .width(Length::Fixed(116.0));

            let done_btn = widget::button::icon(widget::icon::from_name("object-select-symbolic"))
                .icon_size(13)
                .padding(3)
                .on_press(Message::CommitRenameTab)
                .tooltip("Save Name");

            let cancel_btn = widget::button::icon(widget::icon::from_name("window-close-symbolic"))
                .icon_size(13)
                .padding(3)
                .on_press(Message::CancelRenameTab)
                .tooltip("Cancel");

            let edit_chip = widget::row::with_capacity(3)
                .push(input)
                .push(done_btn)
                .push(cancel_btn)
                .spacing(2)
                .align_y(Alignment::Center);

            tab_row = tab_row.push(
                widget::container(edit_chip)
                    .padding([2, 4])
                    .class(cosmic::theme::Container::Card),
            );
            continue;
        }

        let mut chip_inner = widget::row::with_capacity(4)
            .spacing(4)
            .align_y(Alignment::Center);

        if owns_playback {
            let icon_name = if is_playing {
                "audio-volume-high-symbolic"
            } else {
                "media-playback-pause-symbolic"
            };
            chip_inner = chip_inner.push(widget::icon::from_name(icon_name).size(12));
        }

        let name_btn = widget::button::custom(widget::text::body(&tab.name).size(12.5))
            .class(cosmic::theme::Button::Text)
            .padding([3, 4])
            .on_press(Message::SelectTab(tab.id));

        chip_inner = chip_inner.push(name_btn);

        if is_active {
            chip_inner = chip_inner.push(
                widget::button::icon(widget::icon::from_name("edit-symbolic"))
                    .icon_size(11)
                    .padding(2)
                    .on_press(Message::BeginRenameTab(tab.id))
                    .tooltip("Rename Playlist"),
            );
        }

        if tabs.len() > 1 {
            chip_inner = chip_inner.push(
                widget::button::icon(widget::icon::from_name("window-close-symbolic"))
                    .icon_size(11)
                    .padding(2)
                    .on_press(Message::CloseTab(tab.id))
                    .tooltip("Close Tab"),
            );
        }

        let tab_container = widget::container(chip_inner)
            .padding([2, 6])
            .class(cosmic::theme::Container::custom(move |theme| {
                let cosmic = theme.cosmic();
                let accent: Color = cosmic.accent_color().into();
                let on_bg: Color = cosmic.on_bg_color().into();
                let bg = if is_active {
                    Color { a: 0.22, ..accent }
                } else {
                    Color { a: 0.06, ..on_bg }
                };
                let border_color = if is_active {
                    accent
                } else {
                    Color { a: 0.12, ..on_bg }
                };
                widget::container::Style {
                    background: Some(bg.into()),
                    border: Border {
                        color: border_color,
                        width: if is_active { 1.2 } else { 1.0 },
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                }
            }));

        let wrapped: Element<'a, Message> = if idx < 9 {
            widget::tooltip(
                tab_container,
                widget::text::caption(format!("{} tracks • Ctrl+{}", tab.tracks.len(), idx + 1)),
                widget::tooltip::Position::Bottom,
            )
            .into()
        } else {
            tab_container.into()
        };

        tab_row = tab_row.push(wrapped);
    }

    let tabs_scroll = widget::scrollable(tab_row)
        .direction(Direction::Horizontal(
            Scrollbar::new().width(3).scroller_width(3),
        ))
        .width(Length::Fill);

    let add_tab_btn = widget::button::icon(widget::icon::from_name("list-add-symbolic"))
        .icon_size(14)
        .padding(5)
        .on_press(Message::CreateTab)
        .tooltip("New Playlist Tab (Ctrl+T)");

    let top_strip = widget::row::with_capacity(2)
        .push(tabs_scroll)
        .push(add_tab_btn)
        .spacing(4)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    // Compact action strip right underneath the tabs
    let lib_btn = widget::button::icon(widget::icon::from_name("folder-music-symbolic"))
        .icon_size(15)
        .padding(5)
        .class(if show_library {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(Message::ToggleLibraryPanel)
        .tooltip(if show_library {
            "Back to Playlist (Ctrl+L)"
        } else {
            "Browse Local Library (Ctrl+L)"
        });

    let actions = widget::row::with_capacity(8)
        .push(
            widget::button::icon(widget::icon::from_name("audio-x-generic-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::OpenAddFilesDialog)
                .tooltip("Add Audio Files"),
        )
        .push(
            widget::button::icon(widget::icon::from_name("folder-new-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::OpenAddFolderDialog)
                .tooltip("Add Folder to Playlist"),
        )
        .push(
            widget::button::icon(widget::icon::from_name("document-open-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::OpenImportM3uDialog)
                .tooltip("Import M3U8"),
        )
        .push(
            widget::button::icon(widget::icon::from_name("document-save-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::OpenExportM3uDialog)
                .tooltip("Export M3U8"),
        )
        .push(widget::space::horizontal())
        .push(
            widget::button::icon(widget::icon::from_name("view-sort-ascending-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::SortActivePlaylist(SortOrder::ArtistAlbumTrack))
                .tooltip("Sort Playlist"),
        )
        .push(
            widget::button::icon(widget::icon::from_name("media-playlist-shuffle-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::ShuffleActivePlaylistOrder)
                .tooltip("Randomize Playlist Order"),
        )
        .push(
            widget::button::icon(widget::icon::from_name("edit-clear-symbolic"))
                .icon_size(15)
                .padding(5)
                .on_press(Message::ClearActivePlaylist)
                .tooltip("Clear Playlist"),
        )
        .push(lib_btn)
        .spacing(2)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    widget::column::with_capacity(3)
        .push(top_strip)
        .push(actions)
        .push(widget::divider::horizontal::light())
        .spacing(6)
        .padding([10, 10, 0, 10])
        .width(Length::Fill)
        .into()
}
