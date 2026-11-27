// SPDX-License-Identifier: GPL-3.0-only

//! Stylish Lyrics Viewer — available both as a separate frosted pop-out window
//! and as an optional in-stage overlay over the translucent visualizer.

use crate::app::Message;
use crate::library::lyrics::ParsedLyrics;
use crate::library::metadata::Track;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Alignment, Border, Color, Length, Shadow, Vector, window};
use cosmic::prelude::*;
use cosmic::widget;

const APP_ICON: &[u8] = include_bytes!("../../resources/icons/hicolor/scalable/apps/icon.svg");

/// Separate frosted pop-out Lyrics window.
#[allow(clippy::too_many_arguments)]
pub fn window_view<'a>(
    win_id: window::Id,
    track: Option<&'a Track>,
    artwork: Option<&'a widget::image::Handle>,
    lyrics: Option<&'a ParsedLyrics>,
    position_ms: u64,
    is_playing: bool,
    font_scale: f32,
) -> Element<'a, Message> {
    let header_title = track
        .map(|t| format!("Lyrics — {}", t.title))
        .unwrap_or_else(|| "Lyrics — Peppermint".to_string());

    let header = widget::header_bar()
        .title(header_title)
        .on_drag(Message::DragSubWindow(win_id))
        .on_close(Message::CloseLyricsWindow);

    // Compact stylish now-playing banner inside the Lyrics window
    let mini_art: Element<'a, Message> = if let Some(handle) = artwork {
        widget::container(
            widget::image(handle.clone())
                .width(Length::Fixed(48.0))
                .height(Length::Fixed(48.0)),
        )
        .class(cosmic::theme::Container::custom(|_| widget::container::Style {
            background: None,
            shadow: Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.35,
                },
                offset: Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..Default::default()
        }))
        .into()
    } else {
        widget::icon::from_svg_bytes(APP_ICON)
            .symbolic(false)
            .icon()
            .size(44)
            .into()
    };

    let (title_s, artist_s) = match track {
        Some(t) => (t.title.clone(), t.display_artist().to_string()),
        None => ("Nothing Playing".to_string(), "Play a track to view lyrics".to_string()),
    };

    let source_badge = lyrics
        .map(|l| l.source_label.as_str())
        .unwrap_or("No Lyrics");

    let play_icon = if is_playing {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    };

    let top_banner = widget::row::with_capacity(5)
        .push(mini_art)
        .push(
            widget::column::with_capacity(2)
                .push(widget::text::title4(title_s))
                .push(widget::text::caption(format!("{artist_s}  •  {source_badge}")))
                .spacing(2)
                .width(Length::Fill),
        )
        .push(
            widget::button::icon(widget::icon::from_name(play_icon))
                .icon_size(18)
                .padding(8)
                .class(cosmic::theme::Button::Suggested)
                .on_press(Message::PlayPause)
                .tooltip(if is_playing { "Pause" } else { "Play" }),
        )
        .push(
            widget::button::icon(widget::icon::from_name("edit-symbolic"))
                .icon_size(16)
                .padding(8)
                .on_press_maybe(track.map(|t| Message::OpenTagEditorForPath(t.path.clone())))
                .tooltip("Edit Track Info & Lyrics"),
        )
        .spacing(12)
        .padding([12, 20])
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let body = lyrics_scroll_body(track, lyrics, position_ms, true, font_scale);

    let full = widget::column::with_capacity(4)
        .push(header)
        .push(top_banner)
        .push(widget::divider::horizontal::light())
        .push(body)
        .width(Length::Fill)
        .height(Length::Fill);

    widget::container(full)
        .width(Length::Fill)
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            let cosmic = theme.cosmic();
            let bg: Color = cosmic.bg_color().into();
            let divider: Color = cosmic.bg_divider().into();
            widget::container::Style {
                background: Some(Color { a: 0.78, ..bg }.into()),
                border: Border {
                    color: divider,
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }
        }))
        .into()
}

/// Optional in-stage Lyrics overlay rendered cleanly over the translucent visualizer
/// inside the main window (off by default).
pub fn stage_view<'a>(
    track: Option<&'a Track>,
    artwork: Option<&'a widget::image::Handle>,
    lyrics: Option<&'a ParsedLyrics>,
    position_ms: u64,
    font_scale: f32,
) -> Element<'a, Message> {
    let mini_art: Element<'a, Message> = if let Some(handle) = artwork {
        widget::image(handle.clone())
            .width(Length::Fixed(42.0))
            .height(Length::Fixed(42.0))
            .into()
    } else {
        widget::icon::from_svg_bytes(APP_ICON)
            .symbolic(false)
            .icon()
            .size(40)
            .into()
    };

    let (title_s, artist_s) = match track {
        Some(t) => (t.title.clone(), t.display_artist().to_string()),
        None => ("Peppermint".to_string(), "No track playing".to_string()),
    };

    let compact_header = widget::row::with_capacity(5)
        .push(mini_art)
        .push(
            widget::column::with_capacity(2)
                .push(widget::text::title4(title_s).font(cosmic::font::bold()))
                .push(widget::text::caption(artist_s))
                .spacing(1),
        )
        .push(widget::space::horizontal())
        .push(
            widget::button::icon(widget::icon::from_name("edit-symbolic"))
                .icon_size(15)
                .padding(7)
                .on_press_maybe(track.map(|t| Message::OpenTagEditorForPath(t.path.clone())))
                .tooltip("Edit Lyrics in Track Info"),
        )
        .push(
            widget::button::standard("Pop Out")
                .on_press(Message::OpenLyricsWindow)
                .tooltip("Open Lyrics in Separate Window (Ctrl+Y)"),
        )
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let header_box = widget::container(compact_header)
        .padding([16, 108, 6, 108])
        .width(Length::Fill);

    let body = lyrics_scroll_body(track, lyrics, position_ms, false, font_scale);

    widget::column::with_capacity(2)
        .push(header_box)
        .push(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn lyrics_scroll_body<'a>(
    track: Option<&'a Track>,
    lyrics: Option<&'a ParsedLyrics>,
    position_ms: u64,
    large_window: bool,
    font_scale: f32,
) -> Element<'a, Message> {
    let Some(parsed) = lyrics else {
        let mut empty = widget::column::with_capacity(3)
            .push(widget::icon::from_name("audio-x-generic-symbolic").size(42))
            .push(widget::text::title4("No Lyrics Found"))
            .push(
                widget::text::body(
                    "Add embedded lyrics in Track Info, or place a matching .lrc file next to your song.",
                )
                .size(13.5)
                .align_x(Horizontal::Center),
            )
            .spacing(10)
            .align_x(Alignment::Center)
            .max_width(360.0);

        if let Some(t) = track {
            empty = empty.push(
                widget::button::suggested("Add Lyrics in Tag Editor")
                    .on_press(Message::OpenTagEditorForPath(t.path.clone())),
            );
        }

        return widget::container(empty)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .padding(28)
            .into();
    };

    let active_idx = parsed.active_line_index(position_ms);
    let bold_font = cosmic::font::bold();
    let horizontal_pad = if large_window { 36 } else { 96 };
    let mut col = widget::column::with_capacity(parsed.lines.len())
        .spacing(if parsed.is_synced { 14 } else { 12 })
        .padding([24, horizontal_pad, 48, horizontal_pad])
        .align_x(Alignment::Center)
        .width(Length::Fill);

    for (idx, line) in parsed.lines.iter().enumerate() {
        let display = if line.text.trim().is_empty() {
            if parsed.is_synced { "♪" } else { " " }
        } else {
            line.text.as_str()
        };

        if parsed.is_synced {
            let is_active = active_idx == Some(idx);
            let distance = active_idx.map(|a| a.abs_diff(idx)).unwrap_or(1);
            let base_font_size = if is_active {
                if large_window { 30.0 } else { 26.0 }
            } else if distance == 1 {
                if large_window { 23.0 } else { 20.5 }
            } else if distance == 2 {
                if large_window { 20.5 } else { 18.5 }
            } else {
                if large_window { 19.0 } else { 17.0 }
            };
            let font_size = base_font_size * font_scale;

            let style_fn: fn(&cosmic::Theme) -> cosmic::iced::widget::text::Style = if is_active {
                active_lyric_style
            } else if distance == 1 {
                near_lyric_style
            } else if distance == 2 {
                mid_lyric_style
            } else {
                far_lyric_style
            };

            let text_el = widget::text(display)
                .font(bold_font)
                .size(font_size)
                .align_x(Horizontal::Center)
                .class(cosmic::theme::Text::Custom(style_fn));

            let pill = widget::container(text_el)
                .padding(if is_active { [10, 22] } else { [4, 16] })
                .max_width(660.0)
                .class(cosmic::theme::Container::custom(move |theme| {
                    if is_active {
                        let accent: Color = theme.cosmic().accent_color().into();
                        widget::container::Style {
                            background: Some(Color { a: 0.14, ..accent }.into()),
                            border: Border {
                                color: Color { a: 0.42, ..accent },
                                width: 1.0,
                                radius: 14.0.into(),
                            },
                            ..Default::default()
                        }
                    } else {
                        widget::container::Style::default()
                    }
                }));

            if let Some(ts_ms) = line.time_ms {
                col = col.push(
                    widget::button::custom(pill)
                        .class(cosmic::theme::Button::Text)
                        .padding(0)
                        .on_press(Message::SeekToMs(ts_ms)),
                );
            } else {
                col = col.push(pill);
            }
        } else {
            let line_el = widget::text(display)
                .font(bold_font)
                .size((if large_window { 23.0 } else { 20.5 }) * font_scale)
                .align_x(Horizontal::Center)
                .class(cosmic::theme::Text::Custom(plain_lyric_style));
            col = col.push(widget::container(line_el).max_width(660.0));
        }
    }

    widget::scrollable(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn active_lyric_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().on_bg_color().into()),
        ..Default::default()
    }
}

fn plain_lyric_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    let base: Color = theme.cosmic().on_bg_color().into();
    cosmic::iced::widget::text::Style {
        color: Some(Color { a: 0.92, ..base }),
        ..Default::default()
    }
}

fn near_lyric_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    let base: Color = theme.cosmic().on_bg_color().into();
    cosmic::iced::widget::text::Style {
        color: Some(Color { a: 0.60, ..base }),
        ..Default::default()
    }
}

fn mid_lyric_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    let base: Color = theme.cosmic().on_bg_color().into();
    cosmic::iced::widget::text::Style {
        color: Some(Color { a: 0.40, ..base }),
        ..Default::default()
    }
}

fn far_lyric_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    let base: Color = theme.cosmic().on_bg_color().into();
    cosmic::iced::widget::text::Style {
        color: Some(Color { a: 0.25, ..base }),
        ..Default::default()
    }
}
