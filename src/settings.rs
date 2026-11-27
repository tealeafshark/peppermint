// SPDX-License-Identifier: GPL-3.0-only

//! User-configurable application settings persisted alongside playlist state.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Audio,
    Appearance,
    Library,
    LyricsAndShortcuts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
}

impl ReplayGainMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::Track, Self::Album];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Track => "Track",
            Self::Album => "Album",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StartupBehavior {
    #[default]
    None,
    RestorePaused,
    RestoreAndPlay,
}

impl StartupBehavior {
    pub const ALL: [Self; 3] = [Self::None, Self::RestorePaused, Self::RestoreAndPlay];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Don't restore",
            Self::RestorePaused => "Restore paused",
            Self::RestoreAndPlay => "Restore & play",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VizFps {
    Battery30,
    #[default]
    Smooth45,
    Ultra60,
}

impl VizFps {
    pub const ALL: [Self; 3] = [Self::Battery30, Self::Smooth45, Self::Ultra60];

    pub fn label(self) -> &'static str {
        match self {
            Self::Battery30 => "30 fps",
            Self::Smooth45 => "45 fps",
            Self::Ultra60 => "60 fps",
        }
    }

    pub fn interval_ms(self) -> u64 {
        match self {
            Self::Battery30 => 33,
            Self::Smooth45 => 22,
            Self::Ultra60 => 16,
        }
    }

    pub fn dt_secs(self) -> f32 {
        self.interval_ms() as f32 / 1000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RowDensity {
    Compact,
    #[default]
    Comfortable,
}

impl RowDensity {
    pub const ALL: [Self; 2] = [Self::Compact, Self::Comfortable];

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum M3uPathMode {
    #[default]
    Absolute,
    Relative,
}

impl M3uPathMode {
    pub const ALL: [Self; 2] = [Self::Absolute, Self::Relative];

    pub fn label(self) -> &'static str {
        match self {
            Self::Absolute => "Absolute paths",
            Self::Relative => "Relative paths",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LyricsPriority {
    #[default]
    SidecarFirst,
    EmbeddedFirst,
}

impl LyricsPriority {
    pub const ALL: [Self; 2] = [Self::SidecarFirst, Self::EmbeddedFirst];

    pub fn label(self) -> &'static str {
        match self {
            Self::SidecarFirst => "Sidecar .lrc first",
            Self::EmbeddedFirst => "Embedded tags first",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LyricsFontSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl LyricsFontSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            Self::Small => 0.86,
            Self::Medium => 1.0,
            Self::Large => 1.18,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_crossfade_secs() -> f32 {
    3.0
}

fn default_prev_restart_secs() -> u64 {
    3
}

fn default_stage_opacity() -> f32 {
    0.34
}

fn default_playlist_opacity() -> f32 {
    0.88
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // Audio & Playback
    #[serde(default = "default_true")]
    pub gapless: bool,
    pub crossfade_enabled: bool,
    #[serde(default = "default_crossfade_secs")]
    pub crossfade_secs: f32,
    pub replay_gain: ReplayGainMode,
    #[serde(default = "default_true")]
    pub soft_fade: bool,
    #[serde(default = "default_prev_restart_secs")]
    pub prev_restart_secs: u64,
    pub startup_behavior: StartupBehavior,

    // Appearance & Visualizer
    #[serde(default = "default_stage_opacity")]
    pub stage_opacity: f32,
    #[serde(default = "default_playlist_opacity")]
    pub playlist_opacity: f32,
    pub viz_fps: VizFps,
    #[serde(default = "default_true")]
    pub hover_marquee: bool,
    pub row_density: RowDensity,

    // Library & Playlists
    #[serde(default = "default_true")]
    pub auto_scan_library: bool,
    #[serde(default = "default_true")]
    pub allow_duplicates: bool,
    pub m3u_path_mode: M3uPathMode,

    // Lyrics & Shortcuts
    pub lyrics_priority: LyricsPriority,
    pub lyrics_font_size: LyricsFontSize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            gapless: true,
            crossfade_enabled: false,
            crossfade_secs: default_crossfade_secs(),
            replay_gain: ReplayGainMode::Off,
            soft_fade: true,
            prev_restart_secs: default_prev_restart_secs(),
            startup_behavior: StartupBehavior::None,
            stage_opacity: default_stage_opacity(),
            playlist_opacity: default_playlist_opacity(),
            viz_fps: VizFps::Smooth45,
            hover_marquee: true,
            row_density: RowDensity::Comfortable,
            auto_scan_library: true,
            allow_duplicates: true,
            m3u_path_mode: M3uPathMode::Absolute,
            lyrics_priority: LyricsPriority::SidecarFirst,
            lyrics_font_size: LyricsFontSize::Medium,
        }
    }
}

impl AppSettings {
    pub fn clamped(mut self) -> Self {
        self.crossfade_secs = self.crossfade_secs.clamp(1.0, 10.0);
        self.stage_opacity = self.stage_opacity.clamp(0.15, 1.0);
        self.playlist_opacity = self.playlist_opacity.clamp(0.50, 1.0);
        self.prev_restart_secs = match self.prev_restart_secs {
            0 => 0,
            1..=4 => 3,
            _ => 5,
        };
        self
    }
}
