// SPDX-License-Identifier: GPL-3.0-only

//! Main COSMIC Application model for Peppermint.

use crate::audio::engine::AudioEngine;
use crate::audio::eq::{EqPreset, EqSettings};
use crate::audio::visualizer::{VizFrameState, VizMode};
use crate::config::Config;
use crate::fl;
use crate::library::lyrics::{ParsedLyrics, load_lyrics_for_track};
use crate::library::metadata::{
    Track, TrackTagData, read_embedded_artwork, read_full_track_tags, write_track_tags,
};
use crate::library::playlist::{
    AppPersistence, PlaylistTab, RepeatMode, export_m3u8, import_m3u, load_state, save_state,
};
use crate::library::scanner::{collect_tracks_from_paths, scan_library_folders};
use crate::ui;
use crate::ui::tag_editor::TagEditorState;
use cosmic::Application;
use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::dialog::file_chooser;
use cosmic::iced::clipboard::dnd::DndAction;
use cosmic::iced::widget::Stack;
use cosmic::iced::{
    Color, Event, Length, Size, Subscription, event, futures, keyboard, mouse, window,
};
use cosmic::prelude::*;
use cosmic::widget::{self, about::About, menu, text_editor};
use futures::SinkExt;
use rand::Rng;
use rand::seq::SliceRandom;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryViewMode {
    Tracks,
    Albums,
    Artists,
    Folders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    ArtistAlbumTrack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragState {
    pub tab_id: u64,
    pub from_idx: usize,
    pub target_idx: usize,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    AddFiles,
    AddFolder,
    ImportM3u,
    ExportM3u,
    NewTab,
    TrackInfo,
    ToggleLibrary,
    ToggleEq,
    OpenLyricsWindow,
    ToggleStageLyrics,
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::AddFiles => Message::OpenAddFilesDialog,
            MenuAction::AddFolder => Message::OpenAddFolderDialog,
            MenuAction::ImportM3u => Message::OpenImportM3uDialog,
            MenuAction::ExportM3u => Message::OpenExportM3uDialog,
            MenuAction::NewTab => Message::CreateTab,
            MenuAction::TrackInfo => Message::OpenTagEditorForSelection,
            MenuAction::ToggleLibrary => Message::ToggleLibraryPanel,
            MenuAction::ToggleEq => Message::ToggleEqPanel,
            MenuAction::OpenLyricsWindow => Message::OpenLyricsWindow,
            MenuAction::ToggleStageLyrics => Message::ToggleStageLyrics,
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackContextAction {
    PlayNow(u64, usize),
    EditInfo(u64, usize),
    ViewLyrics(u64, usize),
    Remove(u64, usize),
}

impl menu::action::MenuAction for TrackContextAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match *self {
            TrackContextAction::PlayNow(tab_id, index) => Message::PlayTrack { tab_id, index },
            TrackContextAction::EditInfo(tab_id, index) => {
                Message::OpenTagEditorForTrack { tab_id, index }
            }
            TrackContextAction::ViewLyrics(tab_id, index) => {
                Message::OpenLyricsForTrack { tab_id, index }
            }
            TrackContextAction::Remove(tab_id, index) => Message::RemoveTrack { tab_id, index },
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    // Playback
    PlayTrack { tab_id: u64, index: usize },
    PlayPause,
    Stop,
    NextTrack,
    PrevTrack,
    SeekChanged(f64),
    SeekReleased,
    SeekToMs(u64),
    SetVolume(f32),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    PlaybackTick,

    // Equalizer
    ToggleEqPanel,
    EqToggleEnabled(bool),
    EqSetPreamp(f32),
    EqSetBand(usize, f32),
    EqSelectPreset(usize),
    EqReset,

    // Background visualizer & hover marquee
    CycleVizMode,
    VizTick,
    MarqueeTick,

    // Resizable playlist sidebar
    MainWindowResized(window::Id, f32),
    StartResizePlaylist,
    ResizeHandleHover(bool),
    ResizePlaylistMoved(window::Id, f32),
    PointerReleased,

    // Multi-tab playlists
    SelectTab(u64),
    CreateTab,
    CloseTab(u64),
    BeginRenameTab(u64),
    RenameTabInput(String),
    CommitRenameTab,
    CancelRenameTab,
    SetPlaylistFilter(String),
    SortActivePlaylist(SortOrder),
    ShuffleActivePlaylistOrder,
    ClearActivePlaylist,

    // Drag-and-drop & playlist track editing
    PlaylistRowPress(usize),
    PlaylistRowStartDrag(usize),
    PlaylistRowHover(usize),
    PlaylistRowExit(usize),
    MoveTrackRelative { tab_id: u64, index: usize, delta: isize },
    RemoveTrack { tab_id: u64, index: usize },
    AddTracksToActivePlaylist(Vec<Track>),
    AddTrackAndPlay(Track),
    AddPathsToActivePlaylist(Vec<PathBuf>),
    TracksLoadedForTab { tab_id: u64, tracks: Vec<Track> },
    ExternalDndEnter,
    ExternalDndLeave,
    ExternalDndDrop(Vec<u8>),
    ExternalFileTransfer(String),
    WindowFilesDropped(Vec<PathBuf>),

    // Pop-out Tag & File Info Editor
    OpenTagEditorForSelection,
    OpenTagEditorForTrack { tab_id: u64, index: usize },
    OpenTagEditorForPath(PathBuf),
    TagEditorLoaded(TrackTagData),
    CloseTagEditor,
    TagEditorSetTitle(String),
    TagEditorSetArtist(String),
    TagEditorSetAlbum(String),
    TagEditorSetTrackNum(String),
    TagEditorSetYear(String),
    TagEditorSetGenre(String),
    TagEditorLyricsAction(text_editor::Action),
    TagEditorPickArtwork,
    TagEditorArtworkLoaded(Vec<u8>),
    TagEditorRemoveArtwork,
    TagEditorSave,
    TagEditorSaved(Result<Track, String>),

    // Lyrics Viewer (separate pop-out window + optional stage overlay)
    OpenLyricsWindow,
    OpenLyricsForTrack { tab_id: u64, index: usize },
    CloseLyricsWindow,
    ToggleStageLyrics,
    LyricsLoaded {
        path: PathBuf,
        lyrics: Option<ParsedLyrics>,
    },

    // Sub-window helpers
    DragSubWindow(window::Id),

    // File dialogs
    OpenAddFilesDialog,
    OpenAddFolderDialog,
    OpenImportM3uDialog,
    OpenExportM3uDialog,
    ExportResult(Result<String, String>),

    // Local Library sidebar
    ToggleLibraryPanel,
    SetLibrarySearch(String),
    SetLibraryViewMode(LibraryViewMode),
    OpenAddLibraryFolderDialog,
    LibraryFolderAdded(PathBuf),
    RemoveLibraryFolder(usize),
    RescanLibrary,
    LibraryScanFinished(Vec<Track>),

    // Keyboard / App chrome
    KeyShortcut(window::Id, keyboard::Key, keyboard::Modifiers),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    LaunchUrl(String),
    Noop,
}

pub struct PeppermintApp {
    core: cosmic::Core,
    context_page: ContextPage,
    about: About,
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    track_ctx_binds: HashMap<menu::KeyBind, TrackContextAction>,
    config: Config,

    // Audio engine & live playback state
    audio: AudioEngine,
    playing: Option<(u64, usize)>,
    current_track: Option<Track>,
    current_artwork: Option<widget::image::Handle>,
    current_lyrics: Option<ParsedLyrics>,
    is_playing: bool,
    is_paused: bool,
    position_secs: f64,
    duration_secs: f64,
    seeking_preview: Option<f64>,
    last_finished_gen: u64,
    status_error: Option<String>,

    // User playback settings
    volume: f32,
    muted: bool,
    shuffle: bool,
    repeat: RepeatMode,

    // 10-band Equalizer
    show_eq: bool,
    eq: EqSettings,
    eq_presets: Vec<EqPreset>,
    eq_preset_names: Vec<String>,

    // Background Visualizer
    viz_mode: VizMode,
    viz_state: VizFrameState,

    // Resizable playlist sidebar & window metrics
    window_width: f32,
    playlist_width: f32,
    resizing_playlist: bool,
    resize_handle_hovered: bool,

    // Multi-tab playlists
    tabs: Vec<PlaylistTab>,
    active_tab_id: u64,
    next_tab_id: u64,
    renaming_tab: Option<(u64, String)>,
    playlist_filter: String,
    selected_row: Option<usize>,
    hovered_row: Option<usize>,
    hover_scroll_secs: f32,
    drag_state: Option<DragState>,
    external_dnd_hover: bool,

    // Pop-out Tag Editor & Lyrics Viewer
    tag_editor: Option<TagEditorState>,
    lyrics_window_id: Option<window::Id>,
    lyrics_preview_track: Option<Track>,
    lyrics_preview_artwork: Option<widget::image::Handle>,
    lyrics_preview_data: Option<ParsedLyrics>,
    show_stage_lyrics: bool,

    // Local library sidebar
    show_library: bool,
    library_folders: Vec<PathBuf>,
    library_tracks: Vec<Track>,
    library_search: String,
    library_view_mode: LibraryViewMode,
    is_scanning_library: bool,
}

impl cosmic::Application for PeppermintApp {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "io.github.tealeafshark.Peppermint";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        mut core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Disable the opaque outer content container so our main stage can be frosted translucent.
        core.window.content_container = false;

        let persisted = load_state();
        let eq_presets = EqPreset::builtins();
        let eq_preset_names = ui::eq_panel::build_preset_names(&eq_presets);

        let initial_vol = if persisted.muted {
            0.0
        } else {
            persisted.volume
        };
        let audio = AudioEngine::spawn(&persisted.eq, initial_vol);

        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON).symbolic(false))
            .author("Built by TeaLeafShark")
            .version(env!("CARGO_PKG_VERSION"))
            .license(env!("CARGO_PKG_LICENSE"));

        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|ctx| Config::get_entry(&ctx).unwrap_or_else(|(_, c)| c))
            .unwrap_or_default();

        let folders_to_scan = persisted.library_folders.clone();
        let is_scanning_library = !folders_to_scan.is_empty();

        let mut app = Self {
            core,
            context_page: ContextPage::default(),
            about,
            key_binds: HashMap::new(),
            track_ctx_binds: HashMap::new(),
            config,

            audio,
            playing: None,
            current_track: None,
            current_artwork: None,
            current_lyrics: None,
            is_playing: false,
            is_paused: false,
            position_secs: 0.0,
            duration_secs: 0.0,
            seeking_preview: None,
            last_finished_gen: 0,
            status_error: None,

            volume: persisted.volume,
            muted: persisted.muted,
            shuffle: persisted.shuffle,
            repeat: persisted.repeat,

            show_eq: false,
            eq: persisted.eq,
            eq_presets,
            eq_preset_names,

            viz_mode: persisted.viz_mode,
            viz_state: VizFrameState::default(),

            window_width: 1080.0,
            playlist_width: persisted.playlist_width.clamp(240.0, 540.0),
            resizing_playlist: false,
            resize_handle_hovered: false,

            tabs: persisted.tabs,
            active_tab_id: persisted.active_tab_id,
            next_tab_id: persisted.next_tab_id,
            renaming_tab: None,
            playlist_filter: String::new(),
            selected_row: None,
            hovered_row: None,
            hover_scroll_secs: 0.0,
            drag_state: None,
            external_dnd_hover: false,

            tag_editor: None,
            lyrics_window_id: None,
            lyrics_preview_track: None,
            lyrics_preview_artwork: None,
            lyrics_preview_data: None,
            show_stage_lyrics: persisted.show_stage_lyrics,

            show_library: false,
            library_folders: persisted.library_folders,
            library_tracks: Vec::new(),
            library_search: String::new(),
            library_view_mode: LibraryViewMode::Tracks,
            is_scanning_library,
        };

        let title_task = app.update_title();
        let scan_task = if is_scanning_library {
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || scan_library_folders(&folders_to_scan))
                        .await
                        .unwrap_or_default()
                },
                |tracks| cosmic::Action::App(Message::LibraryScanFinished(tracks)),
            )
        } else {
            Task::none()
        };

        (app, Task::batch([title_task, scan_task]))
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu::bar(vec![
            menu::Tree::with_children(
                menu::root(fl!("file")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![
                        menu::Item::Button(fl!("add-files"), None, MenuAction::AddFiles),
                        menu::Item::Button(fl!("add-folder"), None, MenuAction::AddFolder),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("track-info"), None, MenuAction::TrackInfo),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("new-playlist"), None, MenuAction::NewTab),
                        menu::Item::Button(fl!("import-playlist"), None, MenuAction::ImportM3u),
                        menu::Item::Button(fl!("export-playlist"), None, MenuAction::ExportM3u),
                    ],
                ),
            ),
            menu::Tree::with_children(
                menu::root(fl!("view")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![
                        menu::Item::Button(fl!("library"), None, MenuAction::ToggleLibrary),
                        menu::Item::Button(fl!("equalizer"), None, MenuAction::ToggleEq),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("lyrics-window"), None, MenuAction::OpenLyricsWindow),
                        menu::Item::Button(fl!("stage-lyrics"), None, MenuAction::ToggleStageLyrics),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("about"), None, MenuAction::About),
                    ],
                ),
            ),
        ]);

        vec![menu_bar.into()]
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }
        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
        })
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Self::Message> {
        if self.tag_editor.as_ref().is_some_and(|e| e.window_id == id) {
            return Some(Message::CloseTagEditor);
        }
        if self.lyrics_window_id == Some(id) {
            return Some(Message::CloseLyricsWindow);
        }
        None
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        if let Some(ref editor) = self.tag_editor
            && editor.window_id == id
        {
            return ui::tag_editor::view(editor);
        }
        if self.lyrics_window_id == Some(id) {
            let (track, art, lyrics, pos_ms) = if self.lyrics_preview_track.is_some() {
                (
                    self.lyrics_preview_track.as_ref(),
                    self.lyrics_preview_artwork.as_ref(),
                    self.lyrics_preview_data.as_ref(),
                    0u64,
                )
            } else {
                (
                    self.current_track.as_ref(),
                    self.current_artwork.as_ref(),
                    self.current_lyrics.as_ref(),
                    (self.position_secs * 1000.0) as u64,
                )
            };
            return ui::lyrics_view::window_view(id, track, art, lyrics, pos_ms, self.is_playing);
        }
        widget::space::horizontal().into()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let active_tab = self.active_tab();
        let display_pos = self.seeking_preview.unwrap_or(self.position_secs);
        let playlist_w = self.effective_playlist_width();

        // Center stage: free-floating Album Art, or bold Stage Lyrics when Lyrics is active in the rotation
        let center_stage: Element<'_, Message> = if self.viz_mode == VizMode::Lyrics {
            ui::lyrics_view::stage_view(
                self.current_track.as_ref(),
                self.current_artwork.as_ref(),
                self.current_lyrics.as_ref(),
                (display_pos * 1000.0) as u64,
            )
        } else {
            ui::player_bar::album_stage(
                self.current_track.as_ref(),
                self.current_artwork.as_ref(),
                self.status_error.as_deref(),
            )
        };

        // Top-left slide-out 10-band EQ bar + Lyrics pop-out tab directly beneath it
        let selected_preset = self
            .eq_preset_names
            .iter()
            .position(|n| n == &self.eq.preset_name)
            .or_else(|| Some(self.eq_preset_names.len().saturating_sub(1)));

        let eq_overlay = ui::eq_panel::view(
            self.show_eq,
            &self.eq,
            &self.eq_preset_names,
            selected_preset,
            self.lyrics_window_id.is_some(),
        );

        let viz_bg = ui::visualizer::background_canvas(self.viz_mode, &self.viz_state);
        let viz_pill = ui::visualizer::mode_pill(self.viz_mode);

        let transparent_stage = widget::container(
            Stack::new()
                .push(viz_bg)
                .push(center_stage)
                .push(viz_pill)
                .push(eq_overlay)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            let bg: Color = theme.cosmic().bg_color().into();
            widget::container::Style {
                background: Some(Color { a: 0.34, ..bg }.into()),
                ..Default::default()
            }
        }));

        // Bottom big clean media controls anchored below the transparent stage
        let bottom_controls = ui::player_bar::controls_bar(
            self.is_playing,
            display_pos,
            self.duration_secs,
            self.volume,
            self.muted,
            self.shuffle,
            self.repeat,
        );

        let left_stage = widget::column::with_capacity(3)
            .push(transparent_stage)
            .push(widget::divider::horizontal::light())
            .push(bottom_controls)
            .width(Length::Fill)
            .height(Length::Fill);

        // Draggable vertical resize splitter (clamped to 240px .. 50% of window width)
        let handle_active = self.resizing_playlist || self.resize_handle_hovered;
        let resize_divider = widget::mouse_area(
            widget::container(widget::space::horizontal())
                .width(Length::Fixed(6.0))
                .height(Length::Fill)
                .class(cosmic::theme::Container::custom(move |theme| {
                    let cosmic = theme.cosmic();
                    let color: Color = if handle_active {
                        Color {
                            a: 0.65,
                            ..Color::from(cosmic.accent_color())
                        }
                    } else {
                        Color {
                            a: 0.22,
                            ..Color::from(cosmic.on_bg_color())
                        }
                    };
                    widget::container::Style {
                        background: Some(color.into()),
                        ..Default::default()
                    }
                })),
        )
        .on_press(Message::StartResizePlaylist)
        .on_enter(Message::ResizeHandleHover(true))
        .on_exit(Message::ResizeHandleHover(false))
        .interaction(mouse::Interaction::ResizingHorizontally);

        // Right full-height Playlist column (resizable up to 50% width)
        let tabs_header = ui::playlist_tabs::view(
            &self.tabs,
            self.active_tab_id,
            self.playing.map(|(tid, _)| tid),
            self.is_playing,
            self.renaming_tab.as_ref().map(|(id, s)| (*id, s.as_str())),
            self.show_library,
        );

        let right_body: Element<'_, Message> = if self.show_library {
            ui::library_panel::view(
                &self.library_tracks,
                &self.library_folders,
                &self.library_search,
                self.library_view_mode,
                self.is_scanning_library,
            )
        } else {
            ui::playlist_view::view(
                active_tab,
                self.playing,
                self.is_playing,
                self.selected_row,
                self.hovered_row,
                self.hover_scroll_secs,
                playlist_w,
                self.drag_state,
                self.external_dnd_hover,
                &self.playlist_filter,
                &self.track_ctx_binds,
            )
        };

        let right_panel = widget::container(
            widget::column::with_capacity(2)
                .push(tabs_header)
                .push(right_body)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fixed(playlist_w))
        .height(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            let bg: Color = theme.cosmic().primary_container_color().into();
            widget::container::Style {
                background: Some(Color { a: 0.88, ..bg }.into()),
                ..Default::default()
            }
        }));

        let layout = widget::row::with_capacity(3)
            .push(left_stage)
            .push(resize_divider)
            .push(right_panel)
            .width(Length::Fill)
            .height(Length::Fill);

        // Stable DnD ID across animated frames so Wayland & Flatpak portal drops always match.
        widget::dnd_destination::DndDestination::with_id(
            layout,
            widget::Id::new("peppermint-drop-target"),
            vec![Cow::Borrowed("text/uri-list")],
        )
        .drag_id(1)
        .preferred_action(DndAction::Copy)
        .on_enter(|_, _, _| Message::ExternalDndEnter)
        .on_leave(|| Message::ExternalDndLeave)
        .on_file_transfer(Message::ExternalFileTransfer)
        .on_finish(|_mime, data, _action, _, _| Message::ExternalDndDrop(data))
        .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subs = vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
            event::listen_with(|evt, status, win_id| match evt {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::PointerReleased)
                }
                Event::Window(window::Event::Resized(size)) => {
                    Some(Message::MainWindowResized(win_id, size.width))
                }
                Event::Window(window::Event::FileDropped(paths)) => {
                    Some(Message::WindowFilesDropped(paths))
                }
                Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                    if status == event::Status::Ignored =>
                {
                    Some(Message::KeyShortcut(win_id, key, modifiers))
                }
                _ => None,
            }),
        ];

        // Track cursor X only while actively dragging the playlist divider
        if self.resizing_playlist {
            subs.push(event::listen_with(|evt, _status, win_id| match evt {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::ResizePlaylistMoved(win_id, position.x))
                }
                _ => None,
            }));
        }

        // Lightweight playback telemetry tick while playing or paused
        if self.is_playing || self.is_paused || self.playing.is_some() {
            subs.push(Subscription::run(|| {
                cosmic::iced::stream::channel(
                    2,
                    |mut emitter: futures::channel::mpsc::Sender<Message>| async move {
                        let mut interval = tokio::time::interval(Duration::from_millis(120));
                        loop {
                            interval.tick().await;
                            let _ = emitter.send(Message::PlaybackTick).await;
                        }
                    },
                )
            }));
        }

        // Background visualizer animation loop (also drives hover title marquee smoothly)
        if self.viz_mode != VizMode::Off {
            if self.is_playing {
                subs.push(Subscription::run(|| {
                    cosmic::iced::stream::channel(
                        2,
                        |mut emitter: futures::channel::mpsc::Sender<Message>| async move {
                            let mut interval = tokio::time::interval(Duration::from_millis(22));
                            loop {
                                interval.tick().await;
                                let _ = emitter.send(Message::VizTick).await;
                            }
                        },
                    )
                }));
            } else {
                subs.push(Subscription::run(|| {
                    cosmic::iced::stream::channel(
                        2,
                        |mut emitter: futures::channel::mpsc::Sender<Message>| async move {
                            let mut interval = tokio::time::interval(Duration::from_millis(60));
                            loop {
                                interval.tick().await;
                                let _ = emitter.send(Message::VizTick).await;
                            }
                        },
                    )
                }));
            }
        } else if self.hovered_row.is_some() {
            // Dedicated marquee timer when visualizer is Off and user is hovering a playlist row.
            subs.push(Subscription::run(|| {
                cosmic::iced::stream::channel(
                    2,
                    |mut emitter: futures::channel::mpsc::Sender<Message>| async move {
                        let mut interval = tokio::time::interval(Duration::from_millis(90));
                        loop {
                            interval.tick().await;
                            let _ = emitter.send(Message::MarqueeTick).await;
                        }
                    },
                )
            }));
        }

        Subscription::batch(subs)
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            // --- Playback ---
            Message::PlayTrack { tab_id, index } => {
                let lyrics_task = self.start_playing(tab_id, index);
                return Task::batch([self.update_title(), lyrics_task]);
            }
            Message::PlayPause => {
                if self.is_playing {
                    self.audio.pause();
                    self.is_playing = false;
                    self.is_paused = true;
                } else if self.is_paused {
                    self.audio.resume();
                    self.is_playing = true;
                    self.is_paused = false;
                } else {
                    let tab_id = self.active_tab_id;
                    let idx = self.selected_row.unwrap_or(0);
                    if !self.active_tab().tracks.is_empty() {
                        let clamped = idx.min(self.active_tab().tracks.len() - 1);
                        let lyrics_task = self.start_playing(tab_id, clamped);
                        return Task::batch([self.update_title(), lyrics_task]);
                    }
                }
            }
            Message::Stop => {
                self.audio.stop();
                self.is_playing = false;
                self.is_paused = false;
                self.position_secs = 0.0;
                self.seeking_preview = None;
                return self.update_title();
            }
            Message::NextTrack => {
                let lyrics_task = self.advance_track(true);
                return Task::batch([self.update_title(), lyrics_task]);
            }
            Message::PrevTrack => {
                if self.position_secs > 3.0 {
                    self.audio.seek(Duration::ZERO);
                    self.position_secs = 0.0;
                } else {
                    let lyrics_task = self.previous_track();
                    return Task::batch([self.update_title(), lyrics_task]);
                }
            }
            Message::SeekChanged(secs) => {
                self.seeking_preview = Some(secs);
            }
            Message::SeekReleased => {
                if let Some(target) = self.seeking_preview.take() {
                    self.position_secs = target;
                    self.audio.seek(Duration::from_secs_f64(target.max(0.0)));
                }
            }
            Message::SeekToMs(ms) => {
                let secs = ms as f64 / 1000.0;
                self.position_secs = secs;
                self.seeking_preview = None;
                self.audio.seek(Duration::from_millis(ms));
            }
            Message::SetVolume(vol) => {
                self.volume = vol.clamp(0.0, 1.0);
                if self.volume > 0.001 {
                    self.muted = false;
                }
                self.audio
                    .set_volume(if self.muted { 0.0 } else { self.volume });
                self.persist();
            }
            Message::ToggleMute => {
                self.muted = !self.muted;
                self.audio
                    .set_volume(if self.muted { 0.0 } else { self.volume });
                self.persist();
            }
            Message::ToggleShuffle => {
                self.shuffle = !self.shuffle;
                self.persist();
            }
            Message::CycleRepeat => {
                self.repeat = self.repeat.cycle();
                self.persist();
            }
            Message::PlaybackTick => {
                let snap = self.audio.snapshot();
                self.is_playing = snap.is_playing;
                self.is_paused = snap.is_paused;
                if self.seeking_preview.is_none() {
                    self.position_secs = snap.position_ms as f64 / 1000.0;
                }
                if snap.duration_ms > 0 {
                    self.duration_secs = snap.duration_ms as f64 / 1000.0;
                }
                self.status_error = snap.last_error;

                if snap.finished_generation > self.last_finished_gen {
                    self.last_finished_gen = snap.finished_generation;
                    let lyrics_task = self.advance_track(false);
                    return Task::batch([self.update_title(), lyrics_task]);
                }
            }

            // --- Equalizer ---
            Message::ToggleEqPanel => {
                self.show_eq = !self.show_eq;
                self.persist();
            }
            Message::EqToggleEnabled(enabled) => {
                self.eq.enabled = enabled;
                self.audio.update_eq(&self.eq);
                self.persist();
            }
            Message::EqSetPreamp(db) => {
                self.eq.preamp_db = db;
                self.eq.preset_name = "Custom".into();
                self.audio.update_eq(&self.eq);
                self.persist();
            }
            Message::EqSetBand(band_idx, db) => {
                if band_idx < self.eq.bands_db.len() {
                    self.eq.bands_db[band_idx] = db;
                    self.eq.preset_name = "Custom".into();
                    self.audio.update_eq(&self.eq);
                    self.persist();
                }
            }
            Message::EqSelectPreset(idx) => {
                if let Some(preset) = self.eq_presets.get(idx) {
                    self.eq.preset_name = preset.name.clone();
                    self.eq.preamp_db = preset.preamp_db;
                    self.eq.bands_db = preset.bands_db;
                    self.eq.enabled = true;
                    self.audio.update_eq(&self.eq);
                    self.persist();
                }
            }
            Message::EqReset => {
                self.eq.preset_name = "Flat".into();
                self.eq.preamp_db = 0.0;
                self.eq.bands_db = [0.0; 10];
                self.audio.update_eq(&self.eq);
                self.persist();
            }

            // --- Background visualizer & hover marquee ---
            Message::CycleVizMode => {
                self.viz_mode = self.viz_mode.cycle();
                self.show_stage_lyrics = self.viz_mode == VizMode::Lyrics;
                self.persist();
                if self.viz_mode == VizMode::Lyrics
                    && self.current_lyrics.is_none()
                    && let Some(ref cur) = self.current_track
                {
                    return spawn_load_lyrics(cur.path.clone());
                }
            }
            Message::VizTick => {
                let dt = if self.is_playing { 0.022 } else { 0.060 };
                self.viz_state
                    .step(self.audio.viz_tap(), self.is_playing, dt);
                if self.hovered_row.is_some() && self.drag_state.is_none() {
                    self.hover_scroll_secs += dt;
                }
            }
            Message::MarqueeTick => {
                if self.hovered_row.is_some() && self.drag_state.is_none() {
                    self.hover_scroll_secs += 0.090;
                }
            }

            // --- Resizable playlist sidebar ---
            Message::MainWindowResized(win_id, width) => {
                if self.core.main_window_id().is_none_or(|id| id == win_id) && width > 100.0 {
                    self.window_width = width;
                }
            }
            Message::StartResizePlaylist => {
                self.resizing_playlist = true;
            }
            Message::ResizeHandleHover(hovered) => {
                self.resize_handle_hovered = hovered;
            }
            Message::ResizePlaylistMoved(win_id, cursor_x) => {
                if self.resizing_playlist && self.core.main_window_id().is_none_or(|id| id == win_id)
                {
                    let desired = self.window_width - cursor_x;
                    let max_w = (self.window_width * 0.50).max(260.0);
                    self.playlist_width = desired.clamp(240.0, max_w);
                }
            }
            Message::PointerReleased => {
                if self.resizing_playlist {
                    self.resizing_playlist = false;
                    self.persist();
                }
                if let Some(drag) = self.drag_state.take() {
                    self.reorder_track(drag.tab_id, drag.from_idx, drag.target_idx);
                }
            }

            // --- Multi-tab playlists ---
            Message::SelectTab(id) => {
                if self.tabs.iter().any(|t| t.id == id) {
                    self.active_tab_id = id;
                    self.show_library = false;
                    self.selected_row = None;
                    self.hovered_row = None;
                    self.hover_scroll_secs = 0.0;
                    self.drag_state = None;
                    self.renaming_tab = None;
                    self.persist();
                }
            }
            Message::CreateTab => {
                let id = self.next_tab_id;
                self.next_tab_id += 1;
                let name = format!("Playlist {}", self.tabs.len() + 1);
                self.tabs.push(PlaylistTab::new(id, name));
                self.active_tab_id = id;
                self.show_library = false;
                self.selected_row = None;
                self.hovered_row = None;
                self.drag_state = None;
                self.persist();
            }
            Message::CloseTab(id) => {
                if self.tabs.len() <= 1 {
                    return Task::none();
                }
                if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
                    self.tabs.remove(pos);
                    if self.playing.is_some_and(|(pid, _)| pid == id) {
                        self.playing = None;
                    }
                    if self.active_tab_id == id {
                        let next_pos = pos.min(self.tabs.len() - 1);
                        self.active_tab_id = self.tabs[next_pos].id;
                        self.selected_row = None;
                        self.hovered_row = None;
                    }
                    self.persist();
                }
            }
            Message::BeginRenameTab(id) => {
                if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                    self.renaming_tab = Some((id, tab.name.clone()));
                }
            }
            Message::RenameTabInput(text) => {
                if let Some((_, ref mut draft)) = self.renaming_tab {
                    *draft = text;
                }
            }
            Message::CommitRenameTab => {
                if let Some((id, draft)) = self.renaming_tab.take() {
                    let trimmed = draft.trim();
                    if !trimmed.is_empty()
                        && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
                    {
                        tab.name = trimmed.to_string();
                        self.persist();
                    }
                }
            }
            Message::CancelRenameTab => {
                self.renaming_tab = None;
            }
            Message::SetPlaylistFilter(q) => {
                self.playlist_filter = q;
                self.drag_state = None;
                self.hovered_row = None;
            }
            Message::SortActivePlaylist(order) => {
                let active_id = self.active_tab_id;
                let playing_track_id = self
                    .playing
                    .filter(|(tid, _)| *tid == active_id)
                    .and_then(|(_, idx)| self.active_tab().tracks.get(idx).map(|t| t.id));

                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == active_id) {
                    match order {
                        SortOrder::ArtistAlbumTrack => {
                            tab.tracks.sort_by(|a, b| {
                                a.artist
                                    .to_lowercase()
                                    .cmp(&b.artist.to_lowercase())
                                    .then_with(|| a.album.to_lowercase().cmp(&b.album.to_lowercase()))
                                    .then_with(|| {
                                        a.track_number
                                            .unwrap_or(u32::MAX)
                                            .cmp(&b.track_number.unwrap_or(u32::MAX))
                                    })
                                    .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                            });
                        }
                    }
                    if let Some(tid) = playing_track_id
                        && let Some(new_idx) = tab.tracks.iter().position(|t| t.id == tid)
                    {
                        self.playing = Some((active_id, new_idx));
                    }
                }
                self.persist();
            }
            Message::ShuffleActivePlaylistOrder => {
                let active_id = self.active_tab_id;
                let playing_track_id = self
                    .playing
                    .filter(|(tid, _)| *tid == active_id)
                    .and_then(|(_, idx)| self.active_tab().tracks.get(idx).map(|t| t.id));

                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == active_id) {
                    let mut rng = rand::rng();
                    tab.tracks.shuffle(&mut rng);
                    if let Some(tid) = playing_track_id
                        && let Some(new_idx) = tab.tracks.iter().position(|t| t.id == tid)
                    {
                        self.playing = Some((active_id, new_idx));
                    }
                }
                self.persist();
            }
            Message::ClearActivePlaylist => {
                let active_id = self.active_tab_id;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == active_id) {
                    tab.tracks.clear();
                }
                if self.playing.is_some_and(|(tid, _)| tid == active_id) {
                    self.playing = None;
                }
                self.selected_row = None;
                self.hovered_row = None;
                self.drag_state = None;
                self.persist();
            }

            // --- Drag-and-drop reordering & row editing ---
            Message::PlaylistRowPress(idx) => {
                self.selected_row = Some(idx);
            }
            Message::PlaylistRowStartDrag(idx) => {
                if self.playlist_filter.trim().is_empty() {
                    self.selected_row = Some(idx);
                    self.hovered_row = None;
                    self.drag_state = Some(DragState {
                        tab_id: self.active_tab_id,
                        from_idx: idx,
                        target_idx: idx,
                    });
                }
            }
            Message::PlaylistRowHover(idx) => {
                if let Some(ref mut drag) = self.drag_state
                    && drag.tab_id == self.active_tab_id
                {
                    drag.target_idx = idx;
                } else if self.hovered_row != Some(idx) {
                    self.hovered_row = Some(idx);
                    self.hover_scroll_secs = 0.0;
                }
            }
            Message::PlaylistRowExit(idx) => {
                if self.hovered_row == Some(idx) {
                    self.hovered_row = None;
                    self.hover_scroll_secs = 0.0;
                }
            }
            Message::MoveTrackRelative {
                tab_id,
                index,
                delta,
            } => {
                let target = index as isize + delta;
                if target >= 0 {
                    self.reorder_track(tab_id, index, target as usize);
                }
            }
            Message::RemoveTrack { tab_id, index } => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                    && index < tab.tracks.len()
                {
                    tab.tracks.remove(index);
                    if let Some((pid, pidx)) = self.playing
                        && pid == tab_id
                    {
                        if pidx == index {
                            self.playing = None;
                        } else if index < pidx {
                            self.playing = Some((pid, pidx - 1));
                        }
                    }
                    if let Some(sel) = self.selected_row {
                        if tab.tracks.is_empty() {
                            self.selected_row = None;
                        } else if sel >= tab.tracks.len() {
                            self.selected_row = Some(tab.tracks.len() - 1);
                        }
                    }
                    self.hovered_row = None;
                    self.persist();
                }
            }
            Message::AddTracksToActivePlaylist(tracks) => {
                let active_id = self.active_tab_id;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == active_id) {
                    for track in tracks {
                        tab.tracks.push(track.clone_with_new_id());
                    }
                    self.persist();
                }
            }
            Message::AddTrackAndPlay(track) => {
                let active_id = self.active_tab_id;
                let new_idx = if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == active_id) {
                    tab.tracks.push(track.clone_with_new_id());
                    Some(tab.tracks.len() - 1)
                } else {
                    None
                };
                if let Some(idx) = new_idx {
                    self.selected_row = Some(idx);
                    self.persist();
                    let lyrics_task = self.start_playing(active_id, idx);
                    return Task::batch([self.update_title(), lyrics_task]);
                }
            }
            Message::AddPathsToActivePlaylist(paths) | Message::WindowFilesDropped(paths) => {
                let tab_id = self.active_tab_id;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || collect_tracks_from_paths(&paths))
                            .await
                            .unwrap_or_default()
                    },
                    move |tracks| {
                        cosmic::Action::App(Message::TracksLoadedForTab { tab_id, tracks })
                    },
                );
            }
            Message::TracksLoadedForTab { tab_id, tracks } => {
                if !tracks.is_empty()
                    && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.tracks.extend(tracks);
                    self.persist();
                }
            }
            Message::ExternalDndEnter => {
                self.external_dnd_hover = true;
            }
            Message::ExternalDndLeave => {
                self.external_dnd_hover = false;
            }
            Message::ExternalDndDrop(data) => {
                self.external_dnd_hover = false;
                let paths = parse_uri_list(&data);
                if !paths.is_empty() {
                    let tab_id = self.active_tab_id;
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || collect_tracks_from_paths(&paths))
                                .await
                                .unwrap_or_default()
                        },
                        move |tracks| {
                            cosmic::Action::App(Message::TracksLoadedForTab { tab_id, tracks })
                        },
                    );
                }
            }
            Message::ExternalFileTransfer(key) => {
                self.external_dnd_hover = false;
                let tab_id = self.active_tab_id;
                return cosmic::command::file_transfer_receive(key).then(move |res| match res {
                    Ok(files) if !files.is_empty() => {
                        let paths: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
                        Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    collect_tracks_from_paths(&paths)
                                })
                                .await
                                .unwrap_or_default()
                            },
                            move |tracks| {
                                cosmic::Action::App(Message::TracksLoadedForTab { tab_id, tracks })
                            },
                        )
                    }
                    _ => Task::none(),
                });
            }

            // --- Pop-out Track Info & Tag Editor ---
            Message::OpenTagEditorForSelection => {
                let maybe_path = self
                    .selected_row
                    .and_then(|idx| self.active_tab().tracks.get(idx))
                    .or(self.current_track.as_ref())
                    .map(|t| t.path.clone());
                if let Some(path) = maybe_path {
                    return self.update(Message::OpenTagEditorForPath(path));
                }
            }
            Message::OpenTagEditorForTrack { tab_id, index } => {
                let maybe_path = self
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .and_then(|t| t.tracks.get(index))
                    .map(|t| t.path.clone());
                if let Some(path) = maybe_path {
                    return self.update(Message::OpenTagEditorForPath(path));
                }
            }
            Message::OpenTagEditorForPath(path) => {
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || read_full_track_tags(&path))
                            .await
                            .expect("tag reader panicked")
                    },
                    |data| cosmic::Action::App(Message::TagEditorLoaded(data)),
                );
            }
            Message::TagEditorLoaded(data) => {
                if let Some(existing) = self.tag_editor.take() {
                    let win_id = existing.window_id;
                    self.tag_editor = Some(TagEditorState::new(win_id, data));
                    return window::gain_focus(win_id);
                }
                let settings = window::Settings {
                    size: Size::new(760.0, 560.0),
                    min_size: Some(Size::new(620.0, 480.0)),
                    decorations: false,
                    transparent: true,
                    ..Default::default()
                };
                let (win_id, open_task) = window::open(settings);
                self.tag_editor = Some(TagEditorState::new(win_id, data));
                let title_task =
                    self.set_window_title("Track Info — Peppermint".to_string(), win_id);
                return Task::batch([
                    open_task.discard(),
                    title_task,
                    window::enable_blur(win_id),
                ]);
            }
            Message::CloseTagEditor => {
                if let Some(editor) = self.tag_editor.take() {
                    return window::close(editor.window_id);
                }
            }
            Message::TagEditorSetTitle(v) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.data.title = v;
                    e.status_message = None;
                }
            }
            Message::TagEditorSetArtist(v) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.data.artist = v;
                    e.status_message = None;
                }
            }
            Message::TagEditorSetAlbum(v) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.data.album = v;
                    e.status_message = None;
                }
            }
            Message::TagEditorSetTrackNum(v) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.data.track_number = v;
                    e.status_message = None;
                }
            }
            Message::TagEditorSetYear(v) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.data.year = v;
                    e.status_message = None;
                }
            }
            Message::TagEditorSetGenre(v) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.data.genre = v;
                    e.status_message = None;
                }
            }
            Message::TagEditorLyricsAction(action) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.lyrics_content.perform(action);
                    e.data.lyrics = e.lyrics_content.text();
                    e.status_message = None;
                }
            }
            Message::TagEditorPickArtwork => {
                return Task::perform(
                    async move {
                        let dialog = file_chooser::open::Dialog::new()
                            .title("Choose Album Artwork (PNG / JPEG)");
                        match dialog.open_file().await {
                            Ok(response) => {
                                if let Ok(path) = response.url().to_file_path() {
                                    std::fs::read(path).ok()
                                } else {
                                    None
                                }
                            }
                            Err(_) => None,
                        }
                    },
                    |maybe_bytes| match maybe_bytes {
                        Some(bytes) => cosmic::Action::App(Message::TagEditorArtworkLoaded(bytes)),
                        None => cosmic::Action::App(Message::Noop),
                    },
                );
            }
            Message::TagEditorArtworkLoaded(bytes) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.artwork_handle = Some(widget::image::Handle::from_bytes(bytes.clone()));
                    e.data.artwork = Some(bytes);
                    e.data.artwork_modified = true;
                    e.status_message = None;
                }
            }
            Message::TagEditorRemoveArtwork => {
                if let Some(ref mut e) = self.tag_editor {
                    e.artwork_handle = None;
                    e.data.artwork = None;
                    e.data.artwork_modified = true;
                    e.status_message = None;
                }
            }
            Message::TagEditorSave => {
                if let Some(ref mut e) = self.tag_editor {
                    e.is_saving = true;
                    e.data.lyrics = e.lyrics_content.text();
                    let payload = e.data.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || write_track_tags(&payload))
                                .await
                                .unwrap_or_else(|err| Err(format!("Save task failed: {err}")))
                        },
                        |res| cosmic::Action::App(Message::TagEditorSaved(res)),
                    );
                }
            }
            Message::TagEditorSaved(res) => {
                if let Some(ref mut e) = self.tag_editor {
                    e.is_saving = false;
                }
                match res {
                    Ok(updated) => {
                        if let Some(ref mut e) = self.tag_editor {
                            e.data.artwork_modified = false;
                            e.status_message = Some(Ok("Saved tags to file!".into()));
                        }
                        let lyrics_task = self.refresh_track_metadata(&updated);
                        self.persist();
                        return Task::batch([self.update_title(), lyrics_task]);
                    }
                    Err(err) => {
                        if let Some(ref mut e) = self.tag_editor {
                            e.status_message = Some(Err(err));
                        }
                    }
                }
            }

            // --- Stylish Lyrics Viewer (Pop-Out Window + Optional Stage Mode) ---
            Message::OpenLyricsWindow => {
                // Switch lyrics window back to tracking currently playing song.
                self.lyrics_preview_track = None;
                self.lyrics_preview_artwork = None;
                self.lyrics_preview_data = None;
                return self.ensure_lyrics_window_open();
            }
            Message::OpenLyricsForTrack { tab_id, index } => {
                let maybe_track = self
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .and_then(|t| t.tracks.get(index))
                    .cloned();
                if let Some(track) = maybe_track {
                    let is_current = self
                        .current_track
                        .as_ref()
                        .is_some_and(|cur| cur.path == track.path);
                    let path = track.path.clone();
                    if is_current {
                        self.lyrics_preview_track = None;
                        self.lyrics_preview_artwork = None;
                        self.lyrics_preview_data = None;
                    } else {
                        self.lyrics_preview_artwork =
                            read_embedded_artwork(&path).map(widget::image::Handle::from_bytes);
                        self.lyrics_preview_track = Some(track);
                        self.lyrics_preview_data = None;
                    }
                    let open_task = self.ensure_lyrics_window_open();
                    let load_task = spawn_load_lyrics(path);
                    return Task::batch([open_task, load_task]);
                }
            }
            Message::CloseLyricsWindow => {
                self.lyrics_preview_track = None;
                self.lyrics_preview_artwork = None;
                self.lyrics_preview_data = None;
                if let Some(win_id) = self.lyrics_window_id.take() {
                    return window::close(win_id);
                }
            }
            Message::ToggleStageLyrics => {
                if self.viz_mode == VizMode::Lyrics {
                    self.viz_mode = VizMode::AuroraOrbit;
                    self.show_stage_lyrics = false;
                } else {
                    self.viz_mode = VizMode::Lyrics;
                    self.show_stage_lyrics = true;
                }
                self.persist();
                if self.viz_mode == VizMode::Lyrics
                    && self.current_lyrics.is_none()
                    && let Some(ref cur) = self.current_track
                {
                    return spawn_load_lyrics(cur.path.clone());
                }
            }
            Message::LyricsLoaded { path, lyrics } => {
                if self
                    .current_track
                    .as_ref()
                    .is_some_and(|t| t.path == path)
                {
                    self.current_lyrics = lyrics.clone();
                }
                if self
                    .lyrics_preview_track
                    .as_ref()
                    .is_some_and(|t| t.path == path)
                {
                    self.lyrics_preview_data = lyrics;
                }
            }

            // --- Sub-window helpers ---
            Message::DragSubWindow(id) => {
                return window::drag(id);
            }

            // --- File dialogs via XDG Desktop Portal ---
            Message::OpenAddFilesDialog => {
                let tab_id = self.active_tab_id;
                return Task::perform(
                    async move {
                        let dialog = file_chooser::open::Dialog::new().title("Add Audio Files");
                        match dialog.open_files().await {
                            Ok(response) => {
                                let paths: Vec<PathBuf> = response
                                    .urls()
                                    .iter()
                                    .filter_map(|u| u.to_file_path().ok())
                                    .collect();
                                tokio::task::spawn_blocking(move || {
                                    collect_tracks_from_paths(&paths)
                                })
                                .await
                                .unwrap_or_default()
                            }
                            Err(_) => Vec::new(),
                        }
                    },
                    move |tracks| {
                        cosmic::Action::App(Message::TracksLoadedForTab { tab_id, tracks })
                    },
                );
            }
            Message::OpenAddFolderDialog => {
                let tab_id = self.active_tab_id;
                return Task::perform(
                    async move {
                        let dialog =
                            file_chooser::open::Dialog::new().title("Add Folder to Playlist");
                        match dialog.open_folder().await {
                            Ok(response) => {
                                let paths: Vec<PathBuf> =
                                    response.url().to_file_path().into_iter().collect();
                                tokio::task::spawn_blocking(move || {
                                    collect_tracks_from_paths(&paths)
                                })
                                .await
                                .unwrap_or_default()
                            }
                            Err(_) => Vec::new(),
                        }
                    },
                    move |tracks| {
                        cosmic::Action::App(Message::TracksLoadedForTab { tab_id, tracks })
                    },
                );
            }
            Message::OpenImportM3uDialog => {
                let tab_id = self.active_tab_id;
                return Task::perform(
                    async move {
                        let dialog =
                            file_chooser::open::Dialog::new().title("Import M3U / M3U8 Playlist");
                        match dialog.open_file().await {
                            Ok(response) => {
                                if let Ok(path) = response.url().to_file_path() {
                                    tokio::task::spawn_blocking(move || {
                                        import_m3u(&path).unwrap_or_default()
                                    })
                                    .await
                                    .unwrap_or_default()
                                } else {
                                    Vec::new()
                                }
                            }
                            Err(_) => Vec::new(),
                        }
                    },
                    move |tracks| {
                        cosmic::Action::App(Message::TracksLoadedForTab { tab_id, tracks })
                    },
                );
            }
            Message::OpenExportM3uDialog => {
                let tab = self.active_tab().clone();
                let suggested = format!("{}.m3u8", tab.name.replace('/', "_"));
                return Task::perform(
                    async move {
                        let dialog = file_chooser::save::Dialog::new()
                            .title("Export Playlist".to_string())
                            .file_name(suggested);
                        match dialog.save_file().await {
                            Ok(response) => {
                                match response.url().and_then(|u| u.to_file_path().ok()) {
                                    Some(path) => export_m3u8(&path, &tab.tracks)
                                        .map(|()| format!("Saved {}", path.display())),
                                    None => Err("Invalid destination path".into()),
                                }
                            }
                            Err(file_chooser::Error::Cancelled) => Ok(String::new()),
                            Err(err) => Err(format!("Export failed: {err}")),
                        }
                    },
                    |res| cosmic::Action::App(Message::ExportResult(res)),
                );
            }
            Message::ExportResult(res) => {
                if let Err(err) = res {
                    self.status_error = Some(err);
                }
            }

            // --- Local Library sidebar ---
            Message::ToggleLibraryPanel => {
                self.show_library = !self.show_library;
                self.persist();
            }
            Message::SetLibrarySearch(q) => {
                self.library_search = q;
            }
            Message::SetLibraryViewMode(mode) => {
                self.library_view_mode = mode;
            }
            Message::OpenAddLibraryFolderDialog => {
                return Task::perform(
                    async move {
                        let dialog =
                            file_chooser::open::Dialog::new().title("Add Music Library Folder");
                        match dialog.open_folder().await {
                            Ok(response) => response.url().to_file_path().ok(),
                            Err(_) => None,
                        }
                    },
                    |maybe_path| match maybe_path {
                        Some(p) => cosmic::Action::App(Message::LibraryFolderAdded(p)),
                        None => cosmic::Action::App(Message::Noop),
                    },
                );
            }
            Message::LibraryFolderAdded(folder) => {
                if !self.library_folders.contains(&folder) {
                    self.library_folders.push(folder);
                    self.persist();
                    return self.trigger_library_rescan();
                }
            }
            Message::RemoveLibraryFolder(idx) => {
                if idx < self.library_folders.len() {
                    self.library_folders.remove(idx);
                    self.persist();
                    return self.trigger_library_rescan();
                }
            }
            Message::RescanLibrary => {
                return self.trigger_library_rescan();
            }
            Message::LibraryScanFinished(tracks) => {
                self.library_tracks = tracks;
                self.is_scanning_library = false;
            }

            // --- Keyboard shortcuts & chrome ---
            Message::KeyShortcut(win_id, key, modifiers) => {
                if self.core.main_window_id().is_none_or(|id| id == win_id) {
                    return self.handle_key_shortcut(key, modifiers);
                }
            }
            Message::ToggleContextPage(page) => {
                if self.context_page == page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = page;
                    self.core.window.show_context = true;
                }
            }
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::LaunchUrl(url) => {
                let _ = open::that_detached(&url);
            }
            Message::Noop => {}
        }

        Task::none()
    }
}

impl PeppermintApp {
    fn active_tab(&self) -> &PlaylistTab {
        self.tabs
            .iter()
            .find(|t| t.id == self.active_tab_id)
            .unwrap_or(&self.tabs[0])
    }

    fn effective_playlist_width(&self) -> f32 {
        let max_w = (self.window_width * 0.50).max(260.0);
        self.playlist_width.clamp(240.0, max_w)
    }

    fn persist(&self) {
        let state = AppPersistence {
            tabs: self.tabs.clone(),
            active_tab_id: self.active_tab_id,
            next_tab_id: self.next_tab_id,
            library_folders: self.library_folders.clone(),
            volume: self.volume,
            muted: self.muted,
            shuffle: self.shuffle,
            repeat: self.repeat,
            eq: self.eq.clone(),
            show_library: self.show_library,
            show_eq: self.show_eq,
            viz_mode: self.viz_mode,
            playlist_width: self.playlist_width,
            show_stage_lyrics: self.show_stage_lyrics,
        };
        save_state(&state);
    }

    fn ensure_lyrics_window_open(&mut self) -> Task<cosmic::Action<Message>> {
        if let Some(win_id) = self.lyrics_window_id {
            return window::gain_focus(win_id);
        }
        let settings = window::Settings {
            size: Size::new(460.0, 620.0),
            min_size: Some(Size::new(320.0, 380.0)),
            decorations: false,
            transparent: true,
            ..Default::default()
        };
        let (win_id, open_task) = window::open(settings);
        self.lyrics_window_id = Some(win_id);
        let title_task = self.set_window_title("Lyrics — Peppermint".to_string(), win_id);
        let load_task = if self.current_lyrics.is_none()
            && self.lyrics_preview_track.is_none()
            && let Some(ref cur) = self.current_track
        {
            spawn_load_lyrics(cur.path.clone())
        } else {
            Task::none()
        };
        Task::batch([
            open_task.discard(),
            title_task,
            window::enable_blur(win_id),
            load_task,
        ])
    }

    /// Updates matching tracks in all playlists, library, and active player after tag save.
    fn refresh_track_metadata(&mut self, updated: &Track) -> Task<cosmic::Action<Message>> {
        for tab in &mut self.tabs {
            for t in &mut tab.tracks {
                if t.path == updated.path {
                    let old_id = t.id;
                    *t = updated.clone();
                    t.id = old_id;
                }
            }
        }
        for t in &mut self.library_tracks {
            if t.path == updated.path {
                let old_id = t.id;
                *t = updated.clone();
                t.id = old_id;
            }
        }
        let mut reload_lyrics = false;
        if let Some(ref mut cur) = self.current_track
            && cur.path == updated.path
        {
            let old_id = cur.id;
            *cur = updated.clone();
            cur.id = old_id;
            self.current_artwork =
                read_embedded_artwork(&updated.path).map(widget::image::Handle::from_bytes);
            reload_lyrics = true;
        }
        if let Some(ref mut prev) = self.lyrics_preview_track
            && prev.path == updated.path
        {
            *prev = updated.clone();
            self.lyrics_preview_artwork =
                read_embedded_artwork(&updated.path).map(widget::image::Handle::from_bytes);
            reload_lyrics = true;
        }
        if reload_lyrics {
            spawn_load_lyrics(updated.path.clone())
        } else {
            Task::none()
        }
    }

    fn trigger_library_rescan(&mut self) -> Task<cosmic::Action<Message>> {
        let folders = self.library_folders.clone();
        if folders.is_empty() {
            self.library_tracks.clear();
            self.is_scanning_library = false;
            return Task::none();
        }
        self.is_scanning_library = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || scan_library_folders(&folders))
                    .await
                    .unwrap_or_default()
            },
            |tracks| cosmic::Action::App(Message::LibraryScanFinished(tracks)),
        )
    }

    fn start_playing(&mut self, tab_id: u64, index: usize) -> Task<cosmic::Action<Message>> {
        let Some(track) = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|tab| tab.tracks.get(index))
            .cloned()
        else {
            return Task::none();
        };

        let path = track.path.clone();
        self.playing = Some((tab_id, index));
        self.duration_secs = track.duration_secs as f64;
        self.position_secs = 0.0;
        self.seeking_preview = None;
        self.status_error = None;
        self.current_artwork = read_embedded_artwork(&path).map(widget::image::Handle::from_bytes);
        self.current_lyrics = None;
        // Return lyrics window to tracking active playback when a new track starts.
        self.lyrics_preview_track = None;
        self.lyrics_preview_artwork = None;
        self.lyrics_preview_data = None;

        self.audio.play(path.clone(), track.duration_secs);
        self.current_track = Some(track);
        self.is_playing = true;
        self.is_paused = false;

        spawn_load_lyrics(path)
    }

    fn advance_track(&mut self, manual: bool) -> Task<cosmic::Action<Message>> {
        let Some((tab_id, current_idx)) = self.playing else {
            return Task::none();
        };
        let Some(tab_len) = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.tracks.len())
        else {
            return Task::none();
        };
        if tab_len == 0 {
            return Task::none();
        }

        if !manual && self.repeat == RepeatMode::One {
            return self.start_playing(tab_id, current_idx);
        }

        let next_idx = if self.shuffle && tab_len > 1 {
            let mut rng = rand::rng();
            let mut candidate = rng.random_range(0..tab_len);
            if candidate == current_idx {
                candidate = (candidate + 1) % tab_len;
            }
            Some(candidate)
        } else if current_idx + 1 < tab_len {
            Some(current_idx + 1)
        } else if self.repeat == RepeatMode::All || manual {
            Some(0)
        } else {
            None
        };

        if let Some(idx) = next_idx {
            self.start_playing(tab_id, idx)
        } else {
            self.is_playing = false;
            self.is_paused = false;
            self.position_secs = 0.0;
            Task::none()
        }
    }

    fn previous_track(&mut self) -> Task<cosmic::Action<Message>> {
        let Some((tab_id, current_idx)) = self.playing else {
            return Task::none();
        };
        let Some(tab_len) = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.tracks.len())
        else {
            return Task::none();
        };
        if tab_len == 0 {
            return Task::none();
        }

        let prev_idx = if current_idx > 0 {
            current_idx - 1
        } else {
            tab_len - 1
        };
        self.start_playing(tab_id, prev_idx)
    }

    fn reorder_track(&mut self, tab_id: u64, from_idx: usize, to_idx: usize) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        if !tab.move_track(from_idx, to_idx) {
            return;
        }

        if let Some((pid, pidx)) = self.playing
            && pid == tab_id
        {
            let new_pidx = if pidx == from_idx {
                to_idx
            } else if from_idx < to_idx && pidx > from_idx && pidx <= to_idx {
                pidx - 1
            } else if to_idx < from_idx && pidx >= to_idx && pidx < from_idx {
                pidx + 1
            } else {
                pidx
            };
            self.playing = Some((pid, new_pidx));
        }

        if self.active_tab_id == tab_id {
            self.selected_row = Some(to_idx);
        }
        self.persist();
    }

    fn handle_key_shortcut(
        &mut self,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Task<cosmic::Action<Message>> {
        if self.renaming_tab.is_some() {
            if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                self.renaming_tab = None;
            }
            return Task::none();
        }

        if modifiers.control() {
            match key.as_ref() {
                keyboard::Key::Character("t") | keyboard::Key::Character("T") => {
                    return self.update(Message::CreateTab);
                }
                keyboard::Key::Character("e") | keyboard::Key::Character("E") => {
                    return self.update(Message::ToggleEqPanel);
                }
                keyboard::Key::Character("l") | keyboard::Key::Character("L") => {
                    return self.update(Message::ToggleLibraryPanel);
                }
                keyboard::Key::Character("i") | keyboard::Key::Character("I") => {
                    return self.update(Message::OpenTagEditorForSelection);
                }
                keyboard::Key::Character("y") | keyboard::Key::Character("Y") => {
                    return self.update(Message::OpenLyricsWindow);
                }
                keyboard::Key::Character(ch) if ch.len() == 1 => {
                    if let Some(digit) = ch.chars().next().and_then(|c| c.to_digit(10))
                        && (1..=9).contains(&digit)
                        && let Some(tab) = self.tabs.get((digit - 1) as usize)
                    {
                        return self.update(Message::SelectTab(tab.id));
                    }
                }
                _ => {}
            }
            return Task::none();
        }

        if modifiers.alt()
            && matches!(key.as_ref(), keyboard::Key::Character("3"))
        {
            return self.update(Message::OpenTagEditorForSelection);
        }

        match key.as_ref() {
            keyboard::Key::Character(" ") => {
                return self.update(Message::PlayPause);
            }
            keyboard::Key::Character("v") | keyboard::Key::Character("V") => {
                return self.update(Message::CycleVizMode);
            }
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                if let Some(idx) = self.selected_row {
                    return self.update(Message::PlayTrack {
                        tab_id: self.active_tab_id,
                        index: idx,
                    });
                }
            }
            keyboard::Key::Named(keyboard::key::Named::Delete | keyboard::key::Named::Backspace) => {
                if let Some(idx) = self.selected_row {
                    return self.update(Message::RemoveTrack {
                        tab_id: self.active_tab_id,
                        index: idx,
                    });
                }
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                if modifiers.alt() {
                    if let Some(idx) = self.selected_row
                        && idx > 0
                        && self.playlist_filter.trim().is_empty()
                    {
                        return self.update(Message::MoveTrackRelative {
                            tab_id: self.active_tab_id,
                            index: idx,
                            delta: -1,
                        });
                    }
                } else if !self.active_tab().tracks.is_empty() {
                    let next = self
                        .selected_row
                        .map(|i| i.saturating_sub(1))
                        .unwrap_or(0);
                    self.selected_row = Some(next);
                }
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                let len = self.active_tab().tracks.len();
                if modifiers.alt() {
                    if let Some(idx) = self.selected_row
                        && idx + 1 < len
                        && self.playlist_filter.trim().is_empty()
                    {
                        return self.update(Message::MoveTrackRelative {
                            tab_id: self.active_tab_id,
                            index: idx,
                            delta: 1,
                        });
                    }
                } else if len > 0 {
                    let next = self
                        .selected_row
                        .map(|i| (i + 1).min(len - 1))
                        .unwrap_or(0);
                    self.selected_row = Some(next);
                }
            }
            _ => {}
        }

        Task::none()
    }

    fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let title = match &self.current_track {
            Some(track) if self.is_playing || self.is_paused => {
                if track.artist.is_empty() {
                    format!("{} — Peppermint", track.title)
                } else {
                    format!("{} - {} — Peppermint", track.artist, track.title)
                }
            }
            _ => fl!("app-title"),
        };

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(title, id)
        } else {
            Task::none()
        }
    }
}

fn spawn_load_lyrics(path: PathBuf) -> Task<cosmic::Action<Message>> {
    Task::perform(
        async move {
            let p = path.clone();
            let lyrics = tokio::task::spawn_blocking(move || load_lyrics_for_track(&p))
                .await
                .unwrap_or(None);
            (path, lyrics)
        },
        |(path, lyrics)| cosmic::Action::App(Message::LyricsLoaded { path, lyrics }),
    )
}

/// Parses a `text/uri-list` payload dropped from COSMIC Files (or any Wayland file manager).
fn parse_uri_list(data: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("file://") {
            let decoded = percent_decode_uri(stripped);
            out.push(PathBuf::from(decoded));
        } else {
            out.push(PathBuf::from(trimmed));
        }
    }
    out
}

fn percent_decode_uri(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(val) = u8::from_str_radix(hex, 16)
        {
            out.push(val);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
