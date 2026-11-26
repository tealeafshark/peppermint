// SPDX-License-Identifier: GPL-3.0-only

//! Spacious centered free-floating Album Art showcase and bottom big clean media controls.

use crate::app::Message;
use crate::library::metadata::{Track, format_duration};
use crate::library::playlist::RepeatMode;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Alignment, Color, Length, Shadow, Vector};
use cosmic::prelude::*;
use cosmic::widget;

const APP_ICON: &[u8] = include_bytes!("../../resources/icons/hicolor/scalable/apps/icon.svg");

/// Open, spacious center stage showing free-floating Album Art (with subtle drop shadow)
/// and clean track title/artist.
pub fn album_stage<'a>(
    current_track: Option<&'a Track>,
    artwork: Option<&'a widget::image::Handle>,
    status_error: Option<&'a str>,
) -> Element<'a, Message> {
    let art_element: Element<'a, Message> = if let Some(handle) = artwork {
        let art_size = 270.0;
        widget::container(
            widget::image(handle.clone())
                .width(Length::Fixed(art_size))
                .height(Length::Fixed(art_size)),
        )
        .class(cosmic::theme::Container::custom(|_| widget::container::Style {
            background: None,
            shadow: Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.42,
                },
                offset: Vector::new(0.0, 10.0),
                blur_radius: 26.0,
            },
            ..Default::default()
        }))
        .into()
    } else {
        widget::container(
            widget::icon::from_svg_bytes(APP_ICON)
                .symbolic(false)
                .icon()
                .size(144),
        )
        .padding([18, 0, 12, 0])
        .into()
    };

    let (title_text, subtitle_text, badge_text) = match current_track {
        Some(track) => {
            let sub = if track.album.is_empty() {
                track.display_artist().to_string()
            } else {
                format!("{} — {}", track.display_artist(), track.album)
            };
            (track.title.clone(), sub, track.tech_badge())
        }
        None => (
            "Peppermint".to_string(),
            "Drop music onto the playlist on the right to begin".to_string(),
            String::new(),
        ),
    };

    let mut stack_col = widget::column::with_capacity(4)
        .push(art_element)
        .push(widget::text::title3(title_text).align_x(Horizontal::Center))
        .push(
            widget::text::body(subtitle_text)
                .size(14.0)
                .align_x(Horizontal::Center),
        )
        .spacing(12)
        .align_x(Alignment::Center)
        .padding([24, 32]);

    if let Some(err) = status_error {
        stack_col = stack_col.push(widget::text::caption(format!("! {err}")).size(12.0));
    } else if !badge_text.is_empty() {
        stack_col = stack_col.push(widget::text::caption(badge_text).size(11.5));
    }

    widget::container(stack_col)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into()
}

/// Big, clean, spacious bottom media controls bar (seek bar + large circular Play/Pause + volume).
#[allow(clippy::too_many_arguments)]
pub fn controls_bar<'a>(
    is_playing: bool,
    position_secs: f64,
    duration_secs: f64,
    volume: f32,
    muted: bool,
    shuffle: bool,
    repeat: RepeatMode,
) -> Element<'a, Message> {
    let max_dur = duration_secs.max(1.0);
    let clamped_pos = position_secs.clamp(0.0, max_dur);

    // Clean full-width seek row
    let seek_slider = widget::slider(0.0..=max_dur, clamped_pos, Message::SeekChanged)
        .on_release(Message::SeekReleased)
        .step(0.5)
        .width(Length::Fill);

    let seek_row = widget::row::with_capacity(3)
        .push(
            widget::text::caption(format_duration(clamped_pos as u64))
                .size(12.0)
                .width(Length::Fixed(46.0)),
        )
        .push(seek_slider)
        .push(
            widget::text::caption(format_duration(duration_secs as u64))
                .size(12.0)
                .width(Length::Fixed(46.0))
                .align_x(Horizontal::Right),
        )
        .spacing(12)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    // Left controls: Shuffle + Repeat
    let shuffle_btn = widget::button::icon(widget::icon::from_name("media-playlist-shuffle-symbolic"))
        .icon_size(18)
        .padding(8)
        .class(if shuffle {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(Message::ToggleShuffle)
        .tooltip(if shuffle { "Shuffle: On" } else { "Shuffle: Off" });

    let repeat_btn: Element<'a, Message> = if repeat == RepeatMode::One {
        widget::tooltip(
            widget::button::custom(
                widget::row::with_capacity(2)
                    .push(widget::icon::from_name("media-playlist-repeat-symbolic").size(18))
                    .push(widget::text::caption_heading("1").size(11.0))
                    .spacing(2)
                    .align_y(Alignment::Center),
            )
            .padding([6, 8])
            .class(cosmic::theme::Button::Suggested)
            .on_press(Message::CycleRepeat),
            widget::text::caption(repeat.label()),
            widget::tooltip::Position::Top,
        )
        .into()
    } else {
        widget::button::icon(widget::icon::from_name("media-playlist-repeat-symbolic"))
            .icon_size(18)
            .padding(8)
            .class(if repeat != RepeatMode::Off {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Icon
            })
            .on_press(Message::CycleRepeat)
            .tooltip(repeat.label())
            .into()
    };

    let left_group = widget::row::with_capacity(2)
        .push(shuffle_btn)
        .push(repeat_btn)
        .spacing(6)
        .align_y(Alignment::Center)
        .width(Length::FillPortion(1));

    // Center hero transport: Prev, BIG circular Play/Pause, Next
    let play_icon = if is_playing {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    };

    let big_play_btn = widget::button::custom(
        widget::container(widget::icon::from_name(play_icon).size(26))
            .width(Length::Fixed(54.0))
            .height(Length::Fixed(54.0))
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center),
    )
    .padding(0)
    .class(cosmic::theme::Button::Custom {
        active: Box::new(|_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_accent: Color = cosmic.on_accent_color().into();
            widget::button::Style {
                background: Some(accent.into()),
                border_radius: 27.0.into(),
                icon_color: Some(on_accent),
                text_color: Some(on_accent),
                ..Default::default()
            }
        }),
        disabled: Box::new(|_| widget::button::Style::default()),
        hovered: Box::new(|_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_accent: Color = cosmic.on_accent_color().into();
            let brighter = Color {
                r: (accent.r + 0.08).min(1.0),
                g: (accent.g + 0.08).min(1.0),
                b: (accent.b + 0.08).min(1.0),
                a: 1.0,
            };
            widget::button::Style {
                background: Some(brighter.into()),
                border_radius: 27.0.into(),
                icon_color: Some(on_accent),
                text_color: Some(on_accent),
                ..Default::default()
            }
        }),
        pressed: Box::new(|_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_accent: Color = cosmic.on_accent_color().into();
            widget::button::Style {
                background: Some(Color { a: 0.85, ..accent }.into()),
                border_radius: 27.0.into(),
                icon_color: Some(on_accent),
                text_color: Some(on_accent),
                ..Default::default()
            }
        }),
    })
    .on_press(Message::PlayPause);

    let center_transport = widget::row::with_capacity(4)
        .push(
            widget::button::icon(widget::icon::from_name("media-skip-backward-symbolic"))
                .icon_size(22)
                .padding(10)
                .on_press(Message::PrevTrack)
                .tooltip("Previous Track"),
        )
        .push(widget::tooltip(
            big_play_btn,
            widget::text::caption(if is_playing {
                "Pause (Space)"
            } else {
                "Play (Space)"
            }),
            widget::tooltip::Position::Top,
        ))
        .push(
            widget::button::icon(widget::icon::from_name("media-skip-forward-symbolic"))
                .icon_size(22)
                .padding(10)
                .on_press(Message::NextTrack)
                .tooltip("Next Track"),
        )
        .push(
            widget::button::icon(widget::icon::from_name("media-playback-stop-symbolic"))
                .icon_size(18)
                .padding(8)
                .on_press(Message::Stop)
                .tooltip("Stop"),
        )
        .spacing(12)
        .align_y(Alignment::Center);

    // Right controls: Volume
    let vol_icon = if muted || volume <= 0.001 {
        "audio-volume-muted-symbolic"
    } else if volume < 0.35 {
        "audio-volume-low-symbolic"
    } else if volume < 0.7 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    };

    let right_group = widget::row::with_capacity(3)
        .push(widget::space::horizontal())
        .push(
            widget::button::icon(widget::icon::from_name(vol_icon))
                .icon_size(18)
                .padding(6)
                .on_press(Message::ToggleMute)
                .tooltip(if muted { "Unmute" } else { "Mute" }),
        )
        .push(
            widget::slider(0.0..=1.0, if muted { 0.0 } else { volume }, Message::SetVolume)
                .step(0.01_f32)
                .width(Length::Fixed(96.0)),
        )
        .spacing(6)
        .align_y(Alignment::Center)
        .width(Length::FillPortion(1));

    let buttons_row = widget::row::with_capacity(3)
        .push(left_group)
        .push(center_transport)
        .push(right_group)
        .spacing(12)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let controls_col = widget::column::with_capacity(2)
        .push(seek_row)
        .push(buttons_row)
        .spacing(12)
        .width(Length::Fill);

    widget::container(controls_col)
        .padding([14, 24, 18, 24])
        .width(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            let cosmic = theme.cosmic();
            let bg: Color = cosmic.primary_container_color().into();
            widget::container::Style {
                background: Some(Color { a: 0.76, ..bg }.into()),
                ..Default::default()
            }
        }))
        .into()
}
