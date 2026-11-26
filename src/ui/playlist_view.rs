// SPDX-License-Identifier: GPL-3.0-only

//! Airy, uniform-height drag-and-drop reorderable playlist view with hover-scrolling titles
//! and native right-click context menus.

use crate::app::{DragState, Message, TrackContextAction};
use crate::library::metadata::{Track, format_duration};
use crate::library::playlist::PlaylistTab;
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::core::text::{Ellipsize, EllipsizeHeightLimit, Wrapping};
use cosmic::iced::{Alignment, Border, Color, Length, mouse};
use cosmic::prelude::*;
use cosmic::widget::{self, menu};
use std::collections::HashMap;

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    tab: &'a PlaylistTab,
    playing: Option<(u64, usize)>,
    is_playing: bool,
    selected_idx: Option<usize>,
    hovered_idx: Option<usize>,
    hover_scroll_secs: f32,
    playlist_width: f32,
    drag_state: Option<DragState>,
    external_dnd_hover: bool,
    filter: &'a str,
    context_key_binds: &'a HashMap<menu::KeyBind, TrackContextAction>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let filter_lower = filter.trim().to_lowercase();
    let is_filtered = !filter_lower.is_empty();

    let active_drag = drag_state.filter(|d| d.tab_id == tab.id && !is_filtered);

    let list_area: Element<'a, Message> = if tab.tracks.is_empty() {
        let empty_box = widget::column::with_capacity(4)
            .push(widget::icon::from_name("audio-x-generic-symbolic").size(40))
            .push(widget::text::title4(&tab.name))
            .push(
                widget::text::caption(
                    "Drop files or folders here from COSMIC Files, or add music below.",
                )
                .align_x(Horizontal::Center),
            )
            .push(
                widget::row::with_capacity(2)
                    .push(
                        widget::button::suggested("Add Files")
                            .on_press(Message::OpenAddFilesDialog),
                    )
                    .push(
                        widget::button::standard("Library")
                            .on_press(Message::ToggleLibraryPanel),
                    )
                    .spacing(8),
            )
            .spacing(10)
            .align_x(Alignment::Center);

        widget::container(empty_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(cosmic::iced::alignment::Vertical::Center)
            .padding(24)
            .into()
    } else {
        let mut rows = widget::column::with_capacity(tab.tracks.len() + 2)
            .spacing(3)
            .padding([6, 8, 12, 8]);

        if is_filtered {
            rows = rows.push(
                widget::container(
                    widget::text::caption("Clear filter to drag-reorder.").size(11.0),
                )
                .padding([2, 6]),
            );
        }

        // Approximate character budget for the title column based on current sidebar width.
        let visible_title_chars = (((playlist_width - 138.0).max(80.0)) / 7.2) as usize;
        let mut visible_count = 0usize;

        for (idx, track) in tab.tracks.iter().enumerate() {
            if is_filtered
                && !track.title.to_lowercase().contains(&filter_lower)
                && !track.artist.to_lowercase().contains(&filter_lower)
                && !track.album.to_lowercase().contains(&filter_lower)
            {
                continue;
            }
            visible_count += 1;

            let is_current = playing == Some((tab.id, idx));
            let is_selected = selected_idx == Some(idx);
            let is_hovered = hovered_idx == Some(idx) && active_drag.is_none();
            let is_dragged = active_drag.is_some_and(|d| d.from_idx == idx);

            let show_drop_above =
                active_drag.is_some_and(|d| d.target_idx == idx && d.target_idx < d.from_idx);
            let show_drop_below =
                active_drag.is_some_and(|d| d.target_idx == idx && d.target_idx > d.from_idx);

            if show_drop_above {
                rows = rows.push(drop_indicator());
            }

            rows = rows.push(track_card(
                tab.id,
                idx,
                track,
                is_current,
                is_playing,
                is_selected,
                is_hovered,
                hover_scroll_secs,
                visible_title_chars,
                is_dragged,
                active_drag.is_some(),
                !is_filtered,
                context_key_binds,
            ));

            if show_drop_below {
                rows = rows.push(drop_indicator());
            }
        }

        if visible_count == 0 {
            rows = rows.push(
                widget::container(widget::text::caption("No matching tracks."))
                    .padding(spacing.space_m),
            );
        }

        widget::scrollable(rows)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    // Subtle bottom footer: filter box + track count & total time
    let footer = widget::column::with_capacity(2)
        .push(
            widget::text_input("Filter playlist…", filter)
                .on_input(Message::SetPlaylistFilter)
                .width(Length::Fill),
        )
        .push(
            widget::row::with_capacity(2)
                .push(widget::text::caption(format!(
                    "{} {}",
                    tab.tracks.len(),
                    if tab.tracks.len() == 1 {
                        "track"
                    } else {
                        "tracks"
                    }
                )))
                .push(widget::space::horizontal())
                .push(widget::text::caption(format_duration(
                    tab.total_duration_secs(),
                )))
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .spacing(6)
        .padding([8, 10, 10, 10])
        .width(Length::Fill);

    let mut content = widget::column::with_capacity(4)
        .width(Length::Fill)
        .height(Length::Fill);

    if external_dnd_hover {
        let banner = widget::container(
            widget::row::with_capacity(2)
                .push(widget::icon::from_name("list-add-symbolic").size(15))
                .push(widget::text::caption(format!("Drop into “{}”", tab.name)))
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding([8, 10])
        .width(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            let accent: Color = theme.cosmic().accent_color().into();
            widget::container::Style {
                background: Some(Color { a: 0.22, ..accent }.into()),
                border: Border {
                    color: accent,
                    width: 1.5,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        }));
        content = content.push(widget::container(banner).padding([6, 8, 2, 8]));
    }

    content = content
        .push(list_area)
        .push(widget::divider::horizontal::light())
        .push(footer);

    content.into()
}

/// Computes a smooth back-and-forth (ping-pong) character window for long hovered track titles.
fn marquee_slice(text: &str, visible_chars: usize, elapsed_secs: f32) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= visible_chars || visible_chars < 6 {
        return text.to_string();
    }
    let overflow = chars.len() - visible_chars;
    let speed_cps = 5.5_f32; // characters per second
    let pause_secs = 0.75_f32;
    let travel_secs = (overflow as f32 / speed_cps).max(0.4);
    let cycle = (pause_secs + travel_secs) * 2.0;
    let t = elapsed_secs.rem_euclid(cycle);

    let offset = if t < pause_secs {
        0usize
    } else if t < pause_secs + travel_secs {
        let frac = (t - pause_secs) / travel_secs;
        ((frac * overflow as f32).round() as usize).min(overflow)
    } else if t < pause_secs * 2.0 + travel_secs {
        overflow
    } else {
        let frac = (t - (pause_secs * 2.0 + travel_secs)) / travel_secs;
        (((1.0 - frac) * overflow as f32).round() as usize).min(overflow)
    };

    chars[offset..].iter().collect()
}

#[allow(clippy::too_many_arguments)]
fn track_card<'a>(
    tab_id: u64,
    idx: usize,
    track: &'a Track,
    is_current: bool,
    is_playing: bool,
    is_selected: bool,
    is_hovered: bool,
    hover_scroll_secs: f32,
    visible_title_chars: usize,
    is_dragged: bool,
    any_dragging: bool,
    allow_drag: bool,
    context_key_binds: &'a HashMap<menu::KeyBind, TrackContextAction>,
) -> Element<'a, Message> {
    let lead: Element<'a, Message> = if is_current {
        let icon_name = if is_playing {
            "media-playback-start-symbolic"
        } else {
            "media-playback-pause-symbolic"
        };
        widget::icon::from_name(icon_name).size(14).into()
    } else {
        widget::text::caption(format!("{:02}", idx + 1))
            .size(11.5)
            .into()
    };

    let grip_col = widget::row::with_capacity(2)
        .push(widget::text::caption("≡").size(13.0))
        .push(lead)
        .spacing(5)
        .align_y(Alignment::Center)
        .width(Length::Fixed(34.0));

    let displayed_title = if is_hovered {
        marquee_slice(&track.title, visible_title_chars, hover_scroll_secs)
    } else {
        track.title.clone()
    };

    let title_text = widget::text::body(displayed_title)
        .size(13.0)
        .wrapping(Wrapping::None)
        .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1)))
        .width(Length::Fill);

    let artist_text = widget::text::caption(track.display_artist())
        .size(11.5)
        .wrapping(Wrapping::None)
        .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1)))
        .width(Length::Fill);

    let title_artist = widget::column::with_capacity(2)
        .push(title_text)
        .push(artist_text)
        .spacing(1)
        .width(Length::Fill);

    let dur = widget::text::caption(format_duration(track.duration_secs)).size(11.5);

    let remove_btn = widget::button::icon(widget::icon::from_name("window-close-symbolic"))
        .icon_size(11)
        .padding(3)
        .on_press(Message::RemoveTrack { tab_id, index: idx })
        .tooltip("Remove (Del)");

    let row_inner = widget::row::with_capacity(4)
        .push(grip_col)
        .push(title_artist)
        .push(dur)
        .push(remove_btn)
        .spacing(6)
        .padding([7, 8])
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let styled = widget::container(row_inner)
        .width(Length::Fill)
        .class(cosmic::theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_bg: Color = cosmic.on_bg_color().into();

            let (bg, border_color, border_width) = if is_dragged {
                (Color { a: 0.28, ..accent }, accent, 1.5)
            } else if is_current {
                (Color { a: 0.20, ..accent }, Color { a: 0.55, ..accent }, 1.0)
            } else if is_selected {
                (
                    Color { a: 0.12, ..on_bg },
                    Color { a: 0.25, ..on_bg },
                    1.0,
                )
            } else if is_hovered {
                (Color { a: 0.08, ..on_bg }, Color::TRANSPARENT, 0.0)
            } else {
                (Color { a: 0.04, ..on_bg }, Color::TRANSPARENT, 0.0)
            };

            widget::container::Style {
                background: Some(bg.into()),
                border: Border {
                    color: border_color,
                    width: border_width,
                    radius: 8.0.into(),
                },
                ..Default::default()
            }
        }));

    let mut area = widget::mouse_area(styled)
        .on_press(Message::PlaylistRowPress(idx))
        .on_double_click(Message::PlayTrack { tab_id, index: idx })
        .on_enter(Message::PlaylistRowHover(idx))
        .on_exit(Message::PlaylistRowExit(idx));

    if allow_drag {
        area = area
            .on_drag(Message::PlaylistRowStartDrag(idx))
            .interaction(if any_dragging {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Grab
            });
    }

    // Right-click native COSMIC context menu on every track row
    let ctx_items = menu::items(
        context_key_binds,
        vec![
            menu::Item::Button("Play Now", None, TrackContextAction::PlayNow(tab_id, idx)),
            menu::Item::Button(
                "Track Info & Tags…",
                None,
                TrackContextAction::EditInfo(tab_id, idx),
            ),
            menu::Item::Button(
                "View Lyrics",
                None,
                TrackContextAction::ViewLyrics(tab_id, idx),
            ),
            menu::Item::Divider,
            menu::Item::Button(
                "Remove from Playlist",
                None,
                TrackContextAction::Remove(tab_id, idx),
            ),
        ],
    );

    widget::context_menu(area, Some(ctx_items))
        .on_open(Message::PlaylistRowPress(idx))
        .into()
}

/// Crisp COSMIC-accent insertion line shown between rows while drag-reordering.
fn drop_indicator<'a>() -> Element<'a, Message> {
    widget::container(widget::space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(3.0))
        .class(cosmic::theme::Container::custom(|theme| {
            let accent: Color = theme.cosmic().accent_color().into();
            widget::container::Style {
                background: Some(accent.into()),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}
