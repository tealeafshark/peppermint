// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod audio;
mod config;
mod i18n;
mod library;
mod ui;

/// Loads the user's real COSMIC system theme (`ThemeType::System`) even inside Flatpak where
/// `/usr/share/cosmic/com.system76.CosmicTheme.Mode/v1/is_dark` is not mounted.
fn load_cosmic_system_theme() -> cosmic::Theme {
    let is_dark = cosmic::cosmic_theme::ThemeMode::config()
        .ok()
        .and_then(|cfg| cosmic::cosmic_theme::ThemeMode::is_dark(&cfg).ok())
        .unwrap_or(true);

    let mut theme = if is_dark {
        cosmic::theme::system_dark()
    } else {
        cosmic::theme::system_light()
    };
    theme.transparent = theme.cosmic().frosted_windows;
    theme
}

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    let settings = cosmic::app::Settings::default()
        .theme(load_cosmic_system_theme())
        .size(cosmic::iced::Size::new(1080.0, 700.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(460.0)
                .min_height(380.0),
        );

    cosmic::app::run::<app::PeppermintApp>(settings, ())
}
