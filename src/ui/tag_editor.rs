// SPDX-License-Identifier: GPL-3.0-only

//! Separate pop-out Track Information & Tag Editor window (title, artist, album, cover art, lyrics).

use crate::app::Message;
use crate::library::metadata::{TrackTagData, format_duration, format_file_size};
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Alignment, Border, Color, Length, Shadow, Vector, window};
use cosmic::prelude::*;
use cosmic::widget::{self, text_editor};

const APP_ICON: &[u8] = include_bytes!("../../resources/icons/hicolor/scalable/apps/icon.svg");

pub struct TagEditorState {
    pub window_id: window::Id,
    pub data: TrackTagData,
    pub artwork_handle: Option<widget::image::Handle>,
    pub lyrics_content: text_editor::Content,
    pub status_message: Option<Result<String, String>>,
    pub is_saving: bool,
}

impl TagEditorState {
    pub fn new(window_id: window::Id, data: TrackTagData) -> Self {
        let artwork_handle = data
            .artwork
            .as_ref()
            .map(|bytes| widget::image::Handle::from_bytes(bytes.clone()));
        let lyrics_content = text_editor::Content::with_text(&data.lyrics);
        Self {
            window_id,
            data,
            artwork_handle,
            lyrics_content,
            status_message: None,
            is_saving: false,
        }
    }
}

pub fn view<'a>(editor: &'a TagEditorState) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let d = &editor.data;

    // Header bar with drag & close
    let header_title = if d.title.trim().is_empty() {
        "Track Info & Tags".to_string()
    } else {
        format!("Track Info — {}", d.title)
    };

    let header = widget::header_bar()
        .title(header_title)
        .on_drag(Message::DragSubWindow(editor.window_id))
        .on_close(Message::CloseTagEditor);

    // Left column: Album Art preview + buttons + technical file details
    let art_preview: Element<'a, Message> = if let Some(ref handle) = editor.artwork_handle {
        widget::container(
            widget::image(handle.clone())
                .width(Length::Fixed(190.0))
                .height(Length::Fixed(190.0)),
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
                offset: Vector::new(0.0, 8.0),
                blur_radius: 20.0,
            },
            ..Default::default()
        }))
        .into()
    } else {
        widget::container(
            widget::icon::from_svg_bytes(APP_ICON)
                .symbolic(false)
                .icon()
                .size(110),
        )
        .width(Length::Fixed(190.0))
        .height(Length::Fixed(190.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into()
    };

    let art_buttons = widget::row::with_capacity(2)
        .push(
            widget::button::standard("Change Art…")
                .on_press(Message::TagEditorPickArtwork),
        )
        .push(
            widget::button::destructive("Remove")
                .on_press_maybe(editor.artwork_handle.as_ref().map(|_| Message::TagEditorRemoveArtwork)),
        )
        .spacing(6)
        .align_y(Alignment::Center);

    let sr_str = d
        .sample_rate
        .map(|sr| format!("{:.1} kHz", f64::from(sr) / 1000.0))
        .unwrap_or_else(|| "—".into());
    let br_str = d
        .bitrate
        .filter(|&b| b > 0)
        .map(|b| format!("{b} kbps"))
        .unwrap_or_else(|| "—".into());
    let file_name = d
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");

    let file_info_card = widget::container(
        widget::column::with_capacity(7)
            .push(widget::text::caption_heading("File Information"))
            .push(info_line("Format", if d.format_label.is_empty() { "—" } else { &d.format_label }))
            .push(info_line("Duration", &format_duration(d.duration_secs)))
            .push(info_line("Sample Rate", &sr_str))
            .push(info_line("Bitrate", &br_str))
            .push(info_line("Size", &format_file_size(d.file_size_bytes)))
            .push(
                widget::text::caption(file_name)
                    .size(11.0),
            )
            .spacing(4)
            .width(Length::Fill),
    )
    .padding(12)
    .width(Length::Fill)
    .class(cosmic::theme::Container::Card);

    let left_col = widget::column::with_capacity(3)
        .push(art_preview)
        .push(art_buttons)
        .push(file_info_card)
        .spacing(12)
        .align_x(Alignment::Center)
        .width(Length::Fixed(224.0));

    // Right column: Editable metadata fields + multi-line Lyrics editor
    let title_field = labeled_input("Title", "Track title", &d.title, Message::TagEditorSetTitle);
    let artist_field =
        labeled_input("Artist", "Artist name", &d.artist, Message::TagEditorSetArtist);
    let album_field =
        labeled_input("Album", "Album title", &d.album, Message::TagEditorSetAlbum);

    let meta_row = widget::row::with_capacity(3)
        .push(
            labeled_input("Track #", "1", &d.track_number, Message::TagEditorSetTrackNum)
                .apply(widget::container)
                .width(Length::FillPortion(1)),
        )
        .push(
            labeled_input("Year", "2026", &d.year, Message::TagEditorSetYear)
                .apply(widget::container)
                .width(Length::FillPortion(1)),
        )
        .push(
            labeled_input("Genre", "Genre", &d.genre, Message::TagEditorSetGenre)
                .apply(widget::container)
                .width(Length::FillPortion(2)),
        )
        .spacing(10)
        .width(Length::Fill);

    let lyrics_section = widget::column::with_capacity(2)
        .push(
            widget::row::with_capacity(2)
                .push(widget::text::caption_heading("Lyrics"))
                .push(widget::space::horizontal())
                .push(
                    widget::text::caption("Plain text or [mm:ss.xx] synced LRC")
                        .size(11.0),
                )
                .align_y(Alignment::Center)
                .width(Length::Fill),
        )
        .push(
            text_editor::text_editor(&editor.lyrics_content)
                .placeholder("Paste lyrics or [00:12.50] synced LRC lines here…")
                .on_action(Message::TagEditorLyricsAction)
                .height(Length::Fixed(180.0)),
        )
        .spacing(4)
        .width(Length::Fill);

    let right_col = widget::column::with_capacity(5)
        .push(title_field)
        .push(artist_field)
        .push(album_field)
        .push(meta_row)
        .push(lyrics_section)
        .spacing(10)
        .width(Length::Fill);

    let body = widget::row::with_capacity(2)
        .push(left_col)
        .push(right_col)
        .spacing(18)
        .padding([14, 18])
        .width(Length::Fill);

    // Bottom action bar
    let status_el: Element<'a, Message> = match &editor.status_message {
        Some(Ok(msg)) => widget::text::caption(msg).size(12.0).into(),
        Some(Err(err)) => widget::text::caption(format!("Error: {err}"))
            .size(12.0)
            .into(),
        None => widget::text::caption(d.path.display().to_string())
            .size(11.0)
            .into(),
    };

    let save_label = if editor.is_saving {
        "Saving…"
    } else {
        "Save Changes"
    };
    let footer = widget::row::with_capacity(4)
        .push(widget::container(status_el).width(Length::Fill))
        .push(widget::button::standard("Close").on_press(Message::CloseTagEditor))
        .push(
            widget::button::suggested(save_label)
                .on_press_maybe((!editor.is_saving).then_some(Message::TagEditorSave)),
        )
        .spacing(spacing.space_xs)
        .padding([10, 18, 14, 18])
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let full = widget::column::with_capacity(4)
        .push(header)
        .push(widget::scrollable(body).height(Length::Fill))
        .push(widget::divider::horizontal::light())
        .push(footer)
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
                background: Some(Color { a: 0.94, ..bg }.into()),
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

fn labeled_input<'a>(
    label: &'static str,
    placeholder: &'static str,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    widget::column::with_capacity(2)
        .push(widget::text::caption_heading(label))
        .push(
            widget::text_input(placeholder, value)
                .on_input(on_input)
                .width(Length::Fill),
        )
        .spacing(4)
        .width(Length::Fill)
        .into()
}

fn info_line<'a>(label: &'static str, value: &str) -> Element<'a, Message> {
    widget::row::with_capacity(3)
        .push(widget::text::caption(label).size(11.5))
        .push(widget::space::horizontal())
        .push(widget::text::caption_heading(value.to_string()).size(11.5))
        .width(Length::Fill)
        .into()
}
