// SPDX-License-Identifier: GPL-3.0-only

//! Top-left slide-out 10-band Graphic Equalizer bar with live frequency curve.

use crate::app::Message;
use crate::audio::eq::{
    EQ_BAND_LABELS, EQ_MAX_DB, EQ_MIN_DB, EqPreset, EqSettings, NUM_BANDS, combined_response_db,
};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Border, Color, Length, Point, Rectangle, Renderer, mouse};
use cosmic::prelude::*;
use cosmic::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use cosmic::widget::{self, canvas as canvas_widget};

/// Renders the top-left slide-out EQ control:
/// - When `open == false`, shows a sleek `>` pull-tab tucked at the top-left corner.
/// - When `open == true`, slides out as a frosted floating EQ bar over the transparent stage.
pub fn view<'a>(
    open: bool,
    eq: &'a EqSettings,
    preset_names: &'a [String],
    selected_preset: Option<usize>,
) -> Element<'a, Message> {
    if !open {
        let mut tab_row = widget::row::with_capacity(3)
            .push(widget::icon::from_name("go-next-symbolic").size(16))
            .push(widget::text::caption_heading("EQ").size(12.5))
            .spacing(5)
            .align_y(Alignment::Center);

        if eq.enabled && (eq.preamp_db.abs() > 0.05 || eq.bands_db.iter().any(|&v| v.abs() > 0.05)) {
            tab_row = tab_row.push(widget::text::caption("●").size(10.0));
        }

        let pull_tab = widget::button::custom(tab_row)
            .padding([8, 12, 8, 8])
            .class(cosmic::theme::Button::Custom {
                active: Box::new(|_, theme| {
                    let cosmic = theme.cosmic();
                    let bg: Color = cosmic.primary_container_color().into();
                    let accent: Color = cosmic.accent_color().into();
                    widget::button::Style {
                        background: Some(Color { a: 0.82, ..bg }.into()),
                        border_radius: [0.0, 12.0, 12.0, 0.0].into(),
                        border_width: 1.0,
                        border_color: Color { a: 0.45, ..accent },
                        text_color: Some(cosmic.on_bg_color().into()),
                        icon_color: Some(accent),
                        ..Default::default()
                    }
                }),
                disabled: Box::new(|_| widget::button::Style::default()),
                hovered: Box::new(|_, theme| {
                    let cosmic = theme.cosmic();
                    let bg: Color = cosmic.secondary_container_color().into();
                    let accent: Color = cosmic.accent_color().into();
                    widget::button::Style {
                        background: Some(Color { a: 0.94, ..bg }.into()),
                        border_radius: [0.0, 12.0, 12.0, 0.0].into(),
                        border_width: 1.5,
                        border_color: accent,
                        text_color: Some(cosmic.on_bg_color().into()),
                        icon_color: Some(accent),
                        ..Default::default()
                    }
                }),
                pressed: Box::new(|_, theme| {
                    let cosmic = theme.cosmic();
                    let accent: Color = cosmic.accent_color().into();
                    widget::button::Style {
                        background: Some(Color { a: 0.35, ..accent }.into()),
                        border_radius: [0.0, 12.0, 12.0, 0.0].into(),
                        border_width: 1.5,
                        border_color: accent,
                        text_color: Some(cosmic.on_bg_color().into()),
                        icon_color: Some(accent),
                        ..Default::default()
                    }
                }),
            })
            .on_press(Message::ToggleEqPanel);

        return widget::container(
            widget::tooltip(
                pull_tab,
                widget::text::caption("Slide out Equalizer (Ctrl+E)"),
                widget::tooltip::Position::Right,
            ),
        )
        .padding([14, 0, 0, 0])
        .into();
    }

    let spacing = cosmic::theme::spacing();

    // Slide-out EQ bar header with `<` collapse handle on the left
    let header = widget::row::with_capacity(7)
        .push(
            widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .on_press(Message::ToggleEqPanel)
                .tooltip("Hide Equalizer (Ctrl+E)"),
        )
        .push(widget::text::heading("Equalizer"))
        .push(
            widget::toggler(eq.enabled)
                .label(if eq.enabled {
                    "On".to_string()
                } else {
                    "Bypass".to_string()
                })
                .on_toggle(Message::EqToggleEnabled)
                .size(18.0),
        )
        .push(widget::space::horizontal())
        .push(
            widget::dropdown(preset_names, selected_preset, Message::EqSelectPreset)
                .width(Length::Fixed(160.0)),
        )
        .push(widget::button::standard("Reset").on_press(Message::EqReset))
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);

    // Live frequency response curve preview
    let curve: Element<'a, Message> = canvas_widget(EqCurveProgram {
        settings: eq.clone(),
    })
    .width(Length::Fill)
    .height(Length::Fixed(48.0))
    .into();

    let curve_box = widget::container(curve)
        .padding([4, 8])
        .width(Length::Fill)
        .class(cosmic::theme::Container::Card);

    // Preamp + 10 vertical sliders
    let mut sliders_row = widget::row::with_capacity(NUM_BANDS + 2)
        .spacing(6)
        .align_y(Alignment::Center);

    sliders_row = sliders_row.push(eq_slider_column(
        "PRE",
        eq.preamp_db,
        Message::EqSetPreamp,
        eq.enabled,
    ));

    sliders_row = sliders_row.push(
        widget::container(widget::divider::vertical::default())
            .height(Length::Fixed(104.0))
            .padding([0, 4]),
    );

    for band_idx in 0..NUM_BANDS {
        let label = EQ_BAND_LABELS[band_idx];
        let val = eq.bands_db[band_idx];
        sliders_row = sliders_row.push(eq_slider_column(
            label,
            val,
            move |v| Message::EqSetBand(band_idx, v),
            eq.enabled,
        ));
    }

    let drawer_body = widget::column::with_capacity(3)
        .push(header)
        .push(curve_box)
        .push(
            widget::container(sliders_row)
                .width(Length::Fill)
                .align_x(Horizontal::Center),
        )
        .spacing(spacing.space_xs)
        .width(Length::Fill);

    let frosted_panel = widget::container(drawer_body)
        .padding([12, 16, 14, 12])
        .width(Length::Fill)
        .max_width(580.0)
        .class(cosmic::theme::Container::custom(|theme| {
            let cosmic = theme.cosmic();
            let bg: Color = cosmic.primary_container_color().into();
            let accent: Color = cosmic.accent_color().into();
            widget::container::Style {
                background: Some(Color { a: 0.92, ..bg }.into()),
                border: Border {
                    color: Color { a: 0.35, ..accent },
                    width: 1.0,
                    radius: [0.0, 14.0, 14.0, 0.0].into(),
                },
                ..Default::default()
            }
        }));

    widget::container(frosted_panel)
        .padding([14, 16, 0, 0])
        .into()
}

fn eq_slider_column<'a>(
    label: &'static str,
    value_db: f32,
    on_change: impl Fn(f32) -> Message + 'a,
    enabled: bool,
) -> Element<'a, Message> {
    let db_text = if value_db >= 0.05 {
        format!("+{:.1}", value_db)
    } else if value_db <= -0.05 {
        format!("{:.1}", value_db)
    } else {
        "0.0".to_string()
    };

    let slider = widget::vertical_slider(EQ_MIN_DB..=EQ_MAX_DB, value_db, on_change)
        .step(0.5)
        .height(Length::Fixed(96.0))
        .width(18.0);

    let db_label = widget::text::caption(db_text).size(10.0);
    let freq_label = if enabled {
        widget::text::caption_heading(label).size(10.5)
    } else {
        widget::text::caption(label).size(10.5)
    };

    widget::column::with_capacity(3)
        .push(db_label)
        .push(slider)
        .push(freq_label)
        .spacing(3)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}

/// Builds the list of dropdown preset labels (builtins + "Custom").
pub fn build_preset_names(builtins: &[EqPreset]) -> Vec<String> {
    let mut names: Vec<String> = builtins.iter().map(|p| p.name.clone()).collect();
    names.push("Custom".to_string());
    names
}

struct EqCurveProgram {
    settings: EqSettings,
}

impl canvas::Program<Message, cosmic::Theme, Renderer> for EqCurveProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let mid_y = h * 0.5;

        let cosmic_theme = theme.cosmic();
        let accent: Color = cosmic_theme.accent_color().into();
        let subtle: Color = cosmic_theme.on_bg_color().into();
        let grid_color = Color { a: 0.16, ..subtle };
        let zero_line_color = Color { a: 0.30, ..subtle };

        for &db in &[6.0_f32, 0.0, -6.0] {
            let y = mid_y - (db / EQ_MAX_DB) * (h * 0.44);
            let line = Path::line(Point::new(0.0, y), Point::new(w, y));
            let color = if db == 0.0 {
                zero_line_color
            } else {
                grid_color
            };
            frame.stroke(&line, Stroke::default().with_color(color).with_width(1.0));
        }

        let samples = 64usize;
        let f_min = 40.0_f64.ln();
        let f_max = 18_000.0_f64.ln();

        let mut points = Vec::with_capacity(samples + 1);
        for i in 0..=samples {
            let t = i as f64 / samples as f64;
            let freq = (f_min + t * (f_max - f_min)).exp();
            let db = combined_response_db(&self.settings, freq).clamp(EQ_MIN_DB, EQ_MAX_DB);
            let x = (t as f32) * w;
            let y = mid_y - (db / EQ_MAX_DB) * (h * 0.44);
            points.push(Point::new(x, y));
        }

        if points.len() >= 2 {
            let fill_path = Path::new(|b| {
                b.move_to(Point::new(0.0, mid_y));
                for &pt in &points {
                    b.line_to(pt);
                }
                b.line_to(Point::new(w, mid_y));
                b.close();
            });
            let fill_color = if self.settings.enabled {
                Color { a: 0.22, ..accent }
            } else {
                Color { a: 0.10, ..subtle }
            };
            frame.fill(&fill_path, fill_color);

            let stroke_path = Path::new(|b| {
                b.move_to(points[0]);
                for &pt in &points[1..] {
                    b.line_to(pt);
                }
            });
            let line_color = if self.settings.enabled {
                accent
            } else {
                Color { a: 0.45, ..subtle }
            };
            frame.stroke(
                &stroke_path,
                Stroke::default().with_color(line_color).with_width(2.0),
            );
        }

        vec![frame.into_geometry()]
    }
}
