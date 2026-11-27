// SPDX-License-Identifier: GPL-3.0-only

//! Separate frosted pop-out Settings window with categorized sidebar navigation.

use crate::app::Message;
use crate::settings::{
    AppSettings, LyricsFontSize, LyricsPriority, M3uPathMode, ReplayGainMode, RowDensity,
    SettingsTab, StartupBehavior, VizFps,
};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Border, Color, Length, window};
use cosmic::prelude::*;
use cosmic::widget;
use std::path::PathBuf;

pub fn view<'a>(
    win_id: window::Id,
    active_tab: SettingsTab,
    settings: &'a AppSettings,
    library_folders: &'a [PathBuf],
    is_scanning_library: bool,
) -> Element<'a, Message> {
    let header = widget::header_bar()
        .title("Settings — Peppermint")
        .on_drag(Message::DragSubWindow(win_id))
        .on_close(Message::CloseSettingsWindow);

    let sidebar = sidebar_nav(active_tab);
    let content = match active_tab {
        SettingsTab::Audio => audio_tab(settings),
        SettingsTab::Appearance => appearance_tab(settings),
        SettingsTab::Library => library_tab(settings, library_folders, is_scanning_library),
        SettingsTab::LyricsAndShortcuts => lyrics_and_shortcuts_tab(settings),
    };

    let body = widget::row::with_capacity(3)
        .push(sidebar)
        .push(widget::divider::vertical::light())
        .push(
            widget::scrollable(
                widget::container(content)
                    .padding([16, 22, 24, 22])
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill);

    let full = widget::column::with_capacity(3)
        .push(header)
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
                background: Some(Color { a: 0.86, ..bg }.into()),
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

fn sidebar_nav<'a>(active: SettingsTab) -> Element<'a, Message> {
    let tabs = [
        (
            SettingsTab::Audio,
            "Audio & Playback",
            "multimedia-volume-control-symbolic",
        ),
        (
            SettingsTab::Appearance,
            "Appearance",
            "preferences-desktop-display-symbolic",
        ),
        (
            SettingsTab::Library,
            "Library & Playlists",
            "folder-music-symbolic",
        ),
        (
            SettingsTab::LyricsAndShortcuts,
            "Lyrics & Shortcuts",
            "input-keyboard-symbolic",
        ),
    ];

    let mut nav_col = widget::column::with_capacity(tabs.len() + 3)
        .spacing(4)
        .padding([14, 10, 14, 10])
        .width(Length::Fixed(204.0))
        .height(Length::Fill);

    for (tab, label, icon_name) in tabs {
        let is_selected = tab == active;
        let item_row = widget::row::with_capacity(2)
            .push(widget::icon::from_name(icon_name).size(16))
            .push(widget::text::body(label).size(13.5))
            .spacing(10)
            .align_y(Alignment::Center);

        let btn = widget::button::custom(item_row)
            .padding([9, 12])
            .width(Length::Fill)
            .class(nav_button_style(is_selected))
            .on_press(Message::SelectSettingsTab(tab));

        nav_col = nav_col.push(btn);
    }

    nav_col = nav_col
        .push(widget::space::vertical())
        .push(widget::divider::horizontal::light())
        .push(
            widget::container(
                widget::button::standard("Reset Defaults")
                    .on_press(Message::SettingsResetDefaults)
                    .width(Length::Fill),
            )
            .padding([8, 2, 0, 2]),
        );

    widget::container(nav_col)
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            let bg: Color = theme.cosmic().primary_container_color().into();
            widget::container::Style {
                background: Some(Color { a: 0.45, ..bg }.into()),
                ..Default::default()
            }
        }))
        .into()
}

fn nav_button_style(selected: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_bg: Color = cosmic.on_bg_color().into();
            widget::button::Style {
                background: if selected {
                    Some(Color { a: 0.22, ..accent }.into())
                } else {
                    None
                },
                border_radius: 8.0.into(),
                border_width: if selected { 1.0 } else { 0.0 },
                border_color: Color { a: 0.48, ..accent },
                text_color: Some(on_bg),
                icon_color: Some(if selected { accent } else { on_bg }),
                ..Default::default()
            }
        }),
        disabled: Box::new(|_| widget::button::Style::default()),
        hovered: Box::new(move |_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_bg: Color = cosmic.on_bg_color().into();
            widget::button::Style {
                background: Some(
                    if selected {
                        Color { a: 0.28, ..accent }
                    } else {
                        Color { a: 0.09, ..on_bg }
                    }
                    .into(),
                ),
                border_radius: 8.0.into(),
                border_width: if selected { 1.0 } else { 0.0 },
                border_color: Color { a: 0.55, ..accent },
                text_color: Some(on_bg),
                icon_color: Some(if selected { accent } else { on_bg }),
                ..Default::default()
            }
        }),
        pressed: Box::new(|_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            widget::button::Style {
                background: Some(Color { a: 0.32, ..accent }.into()),
                border_radius: 8.0.into(),
                text_color: Some(cosmic.on_bg_color().into()),
                icon_color: Some(accent),
                ..Default::default()
            }
        }),
    }
}

fn audio_tab<'a>(s: &'a AppSettings) -> Element<'a, Message> {
    // Section 1: Transitions
    let gapless_row = setting_toggle_row(
        "Gapless Playback",
        "Pre-decode the next track for seamless 0 ms transitions on live albums and mixes",
        s.gapless,
        Message::SettingsSetGapless,
    );

    let crossfade_desc = if s.crossfade_enabled {
        "Blends the end of one track into the next (overrides gapless seam)"
    } else {
        "Smoothly overlap tracks with an equal-power fade between songs"
    };
    let crossfade_row = setting_toggle_row(
        "Crossfade Between Tracks",
        crossfade_desc,
        s.crossfade_enabled,
        Message::SettingsSetCrossfadeEnabled,
    );

    let mut transitions_items: Vec<Element<'a, Message>> = vec![gapless_row, crossfade_row];
    if s.crossfade_enabled {
        transitions_items.push(setting_slider_row(
            "Crossfade Duration",
            "Overlap length at the end of each song",
            format!("{:.1} s", s.crossfade_secs),
            widget::slider(
                1.0..=10.0,
                s.crossfade_secs,
                Message::SettingsSetCrossfadeSecs,
            )
            .step(0.5_f32)
            .width(Length::Fixed(160.0))
            .into(),
        ));
    }

    // Section 2: Loudness & Comfort
    let rg_buttons = segmented_choices(
        &ReplayGainMode::ALL,
        s.replay_gain,
        ReplayGainMode::label,
        Message::SettingsSetReplayGain,
    );
    let rg_row = setting_control_row(
        "ReplayGain Normalization",
        "Balance playback loudness using embedded REPLAYGAIN tags with peak limiting",
        rg_buttons,
    );

    let soft_fade_row = setting_toggle_row(
        "Soft Fade on Pause / Resume",
        "Apply a subtle 55 ms fade to prevent speaker clicks when pausing or resuming",
        s.soft_fade,
        Message::SettingsSetSoftFade,
    );

    // Section 3: Controls & Session
    let prev_choices: [(u64, &'static str); 3] = [(0, "Always prev"), (3, "3 s"), (5, "5 s")];
    let prev_control = segmented_pairs(
        &prev_choices,
        s.prev_restart_secs,
        Message::SettingsSetPrevRestartSecs,
    );
    let prev_row = setting_control_row(
        "Previous Track Threshold",
        "Pressing Previous past this point restarts the current track instead",
        prev_control,
    );

    let startup_control = segmented_choices(
        &StartupBehavior::ALL,
        s.startup_behavior,
        StartupBehavior::label,
        Message::SettingsSetStartupBehavior,
    );
    let startup_row = setting_control_row(
        "On Startup",
        "Choose whether Peppermint restores the track you were last listening to",
        startup_control,
    );

    widget::column::with_capacity(3)
        .push(section_card("Track Transitions", transitions_items))
        .push(section_card("Loudness & Output", vec![rg_row, soft_fade_row]))
        .push(section_card("Playback Behavior", vec![prev_row, startup_row]))
        .spacing(16)
        .width(Length::Fill)
        .into()
}

fn appearance_tab<'a>(s: &'a AppSettings) -> Element<'a, Message> {
    let stage_pct = (s.stage_opacity * 100.0).round() as u32;
    let stage_row = setting_slider_row(
        "Stage Background Opacity",
        "Adjust the frosted glass translucency behind Album Art and the Visualizer",
        format!("{stage_pct}%"),
        widget::slider(
            0.15..=1.0,
            s.stage_opacity,
            Message::SettingsSetStageOpacity,
        )
        .step(0.02_f32)
        .width(Length::Fixed(160.0))
        .into(),
    );

    let playlist_pct = (s.playlist_opacity * 100.0).round() as u32;
    let playlist_row = setting_slider_row(
        "Playlist Sidebar Opacity",
        "Adjust the translucency of the right-hand playlist and library column",
        format!("{playlist_pct}%"),
        widget::slider(
            0.50..=1.0,
            s.playlist_opacity,
            Message::SettingsSetPlaylistOpacity,
        )
        .step(0.02_f32)
        .width(Length::Fixed(160.0))
        .into(),
    );

    let fps_control = segmented_choices(
        &VizFps::ALL,
        s.viz_fps,
        VizFps::label,
        Message::SettingsSetVizFps,
    );
    let fps_row = setting_control_row(
        "Visualizer Frame Rate",
        "Lower frame rates save laptop battery; 60 fps delivers fluid animations",
        fps_control,
    );

    let marquee_row = setting_toggle_row(
        "Hover Title Marquee",
        "Smoothly scroll long track titles back and forth when hovering playlist rows",
        s.hover_marquee,
        Message::SettingsSetHoverMarquee,
    );

    let density_control = segmented_choices(
        &RowDensity::ALL,
        s.row_density,
        RowDensity::label,
        Message::SettingsSetRowDensity,
    );
    let density_row = setting_control_row(
        "Playlist Row Density",
        "Switch between airy cards or tighter rows that fit more songs on screen",
        density_control,
    );

    widget::column::with_capacity(2)
        .push(section_card(
            "Frosted Glass & Visualizer",
            vec![stage_row, playlist_row, fps_row],
        ))
        .push(section_card(
            "Playlist Appearance",
            vec![marquee_row, density_row],
        ))
        .spacing(16)
        .width(Length::Fill)
        .into()
}

fn library_tab<'a>(
    s: &'a AppSettings,
    library_folders: &'a [PathBuf],
    is_scanning_library: bool,
) -> Element<'a, Message> {
    let mut folder_list = widget::column::with_capacity(library_folders.len() + 2).spacing(6);

    if library_folders.is_empty() {
        folder_list = folder_list.push(
            widget::text::caption("No library folders configured yet. Add your Music folder below.")
                .size(12.5),
        );
    } else {
        for (idx, path) in library_folders.iter().enumerate() {
            let row = widget::row::with_capacity(3)
                .push(widget::icon::from_name("folder-music-symbolic").size(16))
                .push(
                    widget::text::body(path.display().to_string())
                        .size(13.0)
                        .width(Length::Fill),
                )
                .push(
                    widget::button::icon(widget::icon::from_name("user-trash-symbolic"))
                        .icon_size(14)
                        .padding(5)
                        .on_press(Message::RemoveLibraryFolder(idx))
                        .tooltip("Remove folder"),
                )
                .spacing(8)
                .padding([6, 10])
                .align_y(Alignment::Center)
                .width(Length::Fill);

            folder_list = folder_list.push(
                widget::container(row)
                    .width(Length::Fill)
                    .class(cosmic::theme::Container::custom(|theme| {
                        let on_bg: Color = theme.cosmic().on_bg_color().into();
                        widget::container::Style {
                            background: Some(Color { a: 0.05, ..on_bg }.into()),
                            border: Border {
                                radius: 7.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    })),
            );
        }
    }

    let folder_actions = widget::row::with_capacity(3)
        .push(
            widget::button::suggested("Add Music Folder…")
                .on_press(Message::OpenAddLibraryFolderDialog),
        )
        .push(
            widget::button::standard(if is_scanning_library {
                "Scanning…"
            } else {
                "Rescan Library"
            })
            .on_press_maybe((!is_scanning_library).then_some(Message::RescanLibrary)),
        )
        .spacing(8)
        .align_y(Alignment::Center);

    folder_list = folder_list.push(widget::container(folder_actions).padding([4, 0, 0, 0]));

    let auto_scan_row = setting_toggle_row(
        "Auto-Scan Library on Startup",
        "Automatically index watched music folders whenever Peppermint launches",
        s.auto_scan_library,
        Message::SettingsSetAutoScanLibrary,
    );

    let duplicates_row = setting_toggle_row(
        "Allow Duplicate Tracks in Playlists",
        "When off, adding files or folders skips tracks already present in the target tab",
        s.allow_duplicates,
        Message::SettingsSetAllowDuplicates,
    );

    let m3u_control = segmented_choices(
        &M3uPathMode::ALL,
        s.m3u_path_mode,
        M3uPathMode::label,
        Message::SettingsSetM3uPathMode,
    );
    let m3u_row = setting_control_row(
        "M3U8 Export Path Format",
        "Relative paths make exported playlists portable across devices and folders",
        m3u_control,
    );

    widget::column::with_capacity(2)
        .push(section_card(
            "Watched Music Folders",
            vec![folder_list.into(), auto_scan_row],
        ))
        .push(section_card(
            "Playlist Behavior",
            vec![duplicates_row, m3u_row],
        ))
        .spacing(16)
        .width(Length::Fill)
        .into()
}

fn lyrics_and_shortcuts_tab<'a>(s: &'a AppSettings) -> Element<'a, Message> {
    let priority_control = segmented_choices(
        &LyricsPriority::ALL,
        s.lyrics_priority,
        LyricsPriority::label,
        Message::SettingsSetLyricsPriority,
    );
    let priority_row = setting_control_row(
        "Lyrics Source Priority",
        "Choose whether external .lrc files or embedded audio tags take precedence",
        priority_control,
    );

    let size_control = segmented_choices(
        &LyricsFontSize::ALL,
        s.lyrics_font_size,
        LyricsFontSize::label,
        Message::SettingsSetLyricsFontSize,
    );
    let size_row = setting_control_row(
        "Lyrics Text Size",
        "Scales lyrics typography in both the Lyrics window and Stage Lyrics mode",
        size_control,
    );

    let shortcuts: [(&str, &str); 13] = [
        ("Space", "Play / Pause"),
        ("Enter", "Play selected playlist track"),
        ("Delete / Backspace", "Remove selected track"),
        ("Up / Down", "Select previous / next track"),
        ("Alt + Up / Down", "Move selected track up / down"),
        ("Ctrl + T", "Create new playlist tab"),
        ("Ctrl + 1 – 9", "Switch to playlist tab 1–9"),
        ("Ctrl + E", "Toggle 10-band Equalizer"),
        ("Ctrl + L", "Toggle Local Library sidebar"),
        ("Ctrl + I  /  Alt + 3", "Open Track Info & Tag Editor"),
        ("Ctrl + Y", "Open Lyrics Viewer window"),
        ("V", "Cycle background visualizer mode"),
        ("Ctrl + ,", "Open Settings window"),
    ];

    let mut shortcut_rows = widget::column::with_capacity(shortcuts.len()).spacing(6);
    for (keys, desc) in shortcuts {
        let key_badge = widget::container(widget::text::caption_heading(keys).size(11.5))
            .padding([3, 8])
            .class(cosmic::theme::Container::custom(|theme| {
                let on_bg: Color = theme.cosmic().on_bg_color().into();
                widget::container::Style {
                    background: Some(Color { a: 0.10, ..on_bg }.into()),
                    border: Border {
                        color: Color { a: 0.22, ..on_bg },
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }
            }));

        let row = widget::row::with_capacity(3)
            .push(widget::text::body(desc).size(13.0))
            .push(widget::space::horizontal())
            .push(key_badge)
            .align_y(Alignment::Center)
            .width(Length::Fill);

        shortcut_rows = shortcut_rows.push(row);
    }

    widget::column::with_capacity(2)
        .push(section_card(
            "Lyrics Display",
            vec![priority_row, size_row],
        ))
        .push(section_card(
            "Keyboard Shortcuts",
            vec![shortcut_rows.into()],
        ))
        .spacing(16)
        .width(Length::Fill)
        .into()
}

fn section_card<'a>(title: &'static str, items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut inner = widget::column::with_capacity(items.len() * 2).spacing(12);
    for (idx, item) in items.into_iter().enumerate() {
        if idx > 0 {
            inner = inner.push(widget::divider::horizontal::light());
        }
        inner = inner.push(item);
    }

    let card = widget::container(inner)
        .padding([14, 16])
        .width(Length::Fill)
        .class(cosmic::theme::Container::Card);

    widget::column::with_capacity(2)
        .push(widget::text::caption_heading(title).size(13.0))
        .push(card)
        .spacing(6)
        .width(Length::Fill)
        .into()
}

fn setting_toggle_row<'a>(
    title: &'static str,
    subtitle: &'static str,
    value: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    setting_control_row(
        title,
        subtitle,
        widget::toggler(value).on_toggle(on_toggle).size(20.0).into(),
    )
}

fn setting_slider_row<'a>(
    title: &'static str,
    subtitle: &'static str,
    value_label: String,
    slider: Element<'a, Message>,
) -> Element<'a, Message> {
    let right = widget::row::with_capacity(2)
        .push(
            widget::text::caption_heading(value_label)
                .size(12.5)
                .width(Length::Fixed(46.0))
                .align_x(Horizontal::Right),
        )
        .push(slider)
        .spacing(10)
        .align_y(Alignment::Center);

    setting_control_row(title, subtitle, right.into())
}

fn setting_control_row<'a>(
    title: &'static str,
    subtitle: &'static str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    let labels = widget::column::with_capacity(2)
        .push(widget::text::body(title).size(13.5))
        .push(widget::text::caption(subtitle).size(11.5))
        .spacing(2)
        .width(Length::Fill);

    widget::row::with_capacity(2)
        .push(labels)
        .push(control)
        .spacing(16)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

fn segmented_choices<'a, T: Copy + PartialEq + 'static>(
    options: &'static [T],
    selected: T,
    label_fn: fn(T) -> &'static str,
    on_select: fn(T) -> Message,
) -> Element<'a, Message> {
    let mut row = widget::row::with_capacity(options.len())
        .spacing(4)
        .align_y(Alignment::Center);

    for &opt in options {
        let is_active = opt == selected;
        let btn = widget::button::custom(widget::text::caption_heading(label_fn(opt)).size(12.0))
            .padding([6, 10])
            .class(pill_choice_style(is_active))
            .on_press(on_select(opt));
        row = row.push(btn);
    }

    row.into()
}

fn segmented_pairs<'a, T: Copy + PartialEq + 'static>(
    options: &[(T, &'static str)],
    selected: T,
    on_select: fn(T) -> Message,
) -> Element<'a, Message> {
    let mut row = widget::row::with_capacity(options.len())
        .spacing(4)
        .align_y(Alignment::Center);

    for &(opt, label) in options {
        let is_active = opt == selected;
        let btn = widget::button::custom(widget::text::caption_heading(label).size(12.0))
            .padding([6, 10])
            .class(pill_choice_style(is_active))
            .on_press(on_select(opt));
        row = row.push(btn);
    }

    row.into()
}

fn pill_choice_style(active: bool) -> cosmic::theme::Button {
    cosmic::theme::Button::Custom {
        active: Box::new(move |_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_bg: Color = cosmic.on_bg_color().into();
            widget::button::Style {
                background: Some(
                    if active {
                        Color { a: 0.25, ..accent }
                    } else {
                        Color { a: 0.07, ..on_bg }
                    }
                    .into(),
                ),
                border_radius: 7.0.into(),
                border_width: 1.0,
                border_color: if active {
                    Color { a: 0.72, ..accent }
                } else {
                    Color { a: 0.14, ..on_bg }
                },
                text_color: Some(on_bg),
                ..Default::default()
            }
        }),
        disabled: Box::new(|_| widget::button::Style::default()),
        hovered: Box::new(move |_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            let on_bg: Color = cosmic.on_bg_color().into();
            widget::button::Style {
                background: Some(
                    if active {
                        Color { a: 0.32, ..accent }
                    } else {
                        Color { a: 0.13, ..on_bg }
                    }
                    .into(),
                ),
                border_radius: 7.0.into(),
                border_width: 1.0,
                border_color: if active {
                    accent
                } else {
                    Color { a: 0.25, ..on_bg }
                },
                text_color: Some(on_bg),
                ..Default::default()
            }
        }),
        pressed: Box::new(|_, theme| {
            let cosmic = theme.cosmic();
            let accent: Color = cosmic.accent_color().into();
            widget::button::Style {
                background: Some(Color { a: 0.36, ..accent }.into()),
                border_radius: 7.0.into(),
                border_width: 1.0,
                border_color: accent,
                text_color: Some(cosmic.on_bg_color().into()),
                ..Default::default()
            }
        }),
    }
}
