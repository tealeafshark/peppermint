// SPDX-License-Identifier: GPL-3.0-only

//! Fun, artistic, semi-transparent background audio visualizers rendered behind the Album Art.
//!
//! Includes three expressive modes beyond plain bars/waveforms:
//! - **Aurora Orbit**: Layered organic radial aurora ribbons, beat-reactive halo ripples,
//!   and orbiting firefly starlight particles.
//! - **Kaleidoscope**: Rotating symmetrical peppermint mandala petals & crystalline geometry.
//! - **Nebula**: Soft drifting translucent plasma clouds connected by luminous constellations.

use crate::app::Message;
use crate::audio::visualizer::{NUM_VIZ_BANDS, VizFrameState, VizMode};
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Alignment, Color, Length, Point, Rectangle, Renderer, mouse};
use cosmic::prelude::*;
use cosmic::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use cosmic::widget::{self, canvas as canvas_widget};
use std::f32::consts::TAU;

/// Renders the semi-transparent visualizer canvas filling the open background behind Album Art.
pub fn background_canvas<'a>(mode: VizMode, state: &'a VizFrameState) -> Element<'a, Message> {
    if mode == VizMode::Off {
        return widget::space::horizontal()
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    canvas_widget(VisualizerProgram {
        mode,
        viz: state.clone(),
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Subtle top-right pill button to cycle visualizer styles (`Aurora` / `Kaleidoscope` / `Nebula` / `Off`).
pub fn mode_pill<'a>(mode: VizMode) -> Element<'a, Message> {
    let icon_name = match mode {
        VizMode::Off => "view-conceal-symbolic",
        _ => "starred-symbolic",
    };

    let content = widget::row::with_capacity(2)
        .push(widget::icon::from_name(icon_name).size(14))
        .push(widget::text::caption_heading(mode.label()).size(12.0))
        .spacing(5)
        .align_y(Alignment::Center);

    let pill = widget::button::custom(content)
        .padding([7, 10, 7, 12])
        .class(cosmic::theme::Button::Custom {
            active: Box::new(|_, theme| {
                let cosmic = theme.cosmic();
                let bg: Color = cosmic.primary_container_color().into();
                let accent: Color = cosmic.accent_color().into();
                widget::button::Style {
                    background: Some(Color { a: 0.72, ..bg }.into()),
                    border_radius: [12.0, 0.0, 0.0, 12.0].into(),
                    border_width: 1.0,
                    border_color: Color { a: 0.32, ..accent },
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
                    background: Some(Color { a: 0.90, ..bg }.into()),
                    border_radius: [12.0, 0.0, 0.0, 12.0].into(),
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
                    background: Some(Color { a: 0.30, ..accent }.into()),
                    border_radius: [12.0, 0.0, 0.0, 12.0].into(),
                    border_width: 1.5,
                    border_color: accent,
                    text_color: Some(cosmic.on_bg_color().into()),
                    icon_color: Some(accent),
                    ..Default::default()
                }
            }),
        })
        .on_press(Message::CycleVizMode);

    widget::container(widget::tooltip(
        pill,
        widget::text::caption("Switch Visualizer Style (V)"),
        widget::tooltip::Position::Left,
    ))
    .width(Length::Fill)
    .align_x(Horizontal::Right)
    .padding([14, 0, 0, 0])
    .into()
}

struct VisualizerProgram {
    mode: VizMode,
    viz: VizFrameState,
}

impl canvas::Program<Message, cosmic::Theme, Renderer> for VisualizerProgram {
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
        let accent: Color = theme.cosmic().accent_color().into();
        let palette = build_palette(accent);

        match self.mode {
            VizMode::AuroraOrbit => draw_aurora_orbit(&mut frame, bounds, &self.viz, &palette),
            VizMode::Kaleidoscope => draw_kaleidoscope(&mut frame, bounds, &self.viz, &palette),
            VizMode::Nebula => draw_nebula(&mut frame, bounds, &self.viz, &palette),
            VizMode::Off => {}
        }

        vec![frame.into_geometry()]
    }
}

/// Harmonious 4-color palette anchored on the user's COSMIC accent + fresh mint/coral/violet tones.
struct VizPalette {
    c0: Color, // COSMIC accent
    c1: Color, // Peppermint teal
    c2: Color, // Warm coral / magenta
    c3: Color, // Soft sky / lavender
}

fn build_palette(accent: Color) -> VizPalette {
    let mint = Color::from_rgb(0.20, 0.85, 0.65);
    let coral = Color::from_rgb(0.95, 0.42, 0.56);
    let violet = Color::from_rgb(0.54, 0.46, 0.96);
    VizPalette {
        c0: accent,
        c1: mix_color(accent, mint, 0.62),
        c2: mix_color(accent, coral, 0.68),
        c3: mix_color(accent, violet, 0.60),
    }
}

impl VizPalette {
    fn sample(&self, t: f32, alpha: f32) -> Color {
        let t = t.rem_euclid(1.0) * 4.0;
        let base = if t < 1.0 {
            mix_color(self.c0, self.c1, t)
        } else if t < 2.0 {
            mix_color(self.c1, self.c2, t - 1.0)
        } else if t < 3.0 {
            mix_color(self.c2, self.c3, t - 2.0)
        } else {
            mix_color(self.c3, self.c0, t - 3.0)
        };
        Color {
            a: alpha.clamp(0.0, 1.0),
            ..base
        }
    }
}

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

/// Mode 1: **Aurora Orbit** — Layered translucent organic aurora ribbons radiating from behind
/// the album art, beat-triggered shockwave ripples, and orbiting firefly particles.
fn draw_aurora_orbit(frame: &mut Frame, bounds: Rectangle, viz: &VizFrameState, pal: &VizPalette) {
    let center = Point::new(bounds.width * 0.5, bounds.height * 0.48);
    let scale = bounds.width.min(bounds.height) * 0.5;
    let t = viz.time;

    // 1. Soft breathing core glow behind the Album Art
    let glow_r = scale * (0.48 + viz.bass * 0.24 + viz.beat_flash * 0.08);
    for layer in (0..4).rev() {
        let frac = (layer as f32 + 1.0) / 4.0;
        let r = glow_r * (0.62 + frac * 0.52);
        let alpha = (0.045 + viz.energy * 0.055) * (1.15 - frac * 0.75);
        let c = pal.sample(t * 0.05 + frac * 0.25, alpha);
        frame.fill(&Path::circle(center, r), c);
    }

    // 2. Three layered organic translucent aurora ribbons
    let points_count = 72usize;
    for layer in 0..3 {
        let lf = layer as f32;
        let base_r = scale * (0.42 + lf * 0.13 + viz.bass * (0.14 - lf * 0.02));
        let rot = t * (0.18 + lf * 0.09) * if layer % 2 == 0 { 1.0 } else { -0.8 };
        let lobe_freq = 3.0 + lf * 2.0;

        let mut pts = Vec::with_capacity(points_count);
        for i in 0..points_count {
            let frac = i as f32 / points_count as f32;
            let ang = frac * TAU + rot;
            // Symmetrical frequency lookup so the closed loop meets seamlessly
            let sym = if frac <= 0.5 { frac * 2.0 } else { (1.0 - frac) * 2.0 };
            let band_idx = ((sym * (NUM_VIZ_BANDS - 1) as f32) as usize).min(NUM_VIZ_BANDS - 1);
            let band = viz.bands[band_idx];

            let wave = (ang * lobe_freq + t * (1.1 + lf * 0.4)).sin() * (0.045 + viz.mid * 0.09)
                + (ang * (lobe_freq + 3.0) - t * 1.6).cos() * (0.025 + viz.treble * 0.07);
            let r = base_r * (1.0 + wave + band * (0.26 - lf * 0.04));
            pts.push(Point::new(center.x + ang.cos() * r, center.y + ang.sin() * r));
        }

        let ribbon = smooth_closed_path(&pts);
        let fill_alpha = (0.07 + viz.energy * 0.09 - lf * 0.015).max(0.03);
        let stroke_alpha = (0.20 + viz.energy * 0.30 - lf * 0.04).max(0.08);
        let hue = (t * 0.04 + lf * 0.31) % 1.0;

        frame.fill(&ribbon, pal.sample(hue, fill_alpha));
        frame.stroke(
            &ribbon,
            Stroke::default()
                .with_color(pal.sample(hue + 0.12, stroke_alpha))
                .with_width(1.6 + viz.bass * 1.4),
        );
    }

    // 3. Beat-triggered expanding halo ripples
    for ring in &viz.rings {
        let r = scale * ring.radius;
        let c = pal.sample(ring.hue_shift, ring.alpha * 0.65);
        frame.stroke(
            &Path::circle(center, r),
            Stroke::default().with_color(c).with_width(ring.thickness),
        );
    }

    // 4. Orbiting luminous firefly particles + soft sparkle trails
    for (i, p) in viz.particles.iter().enumerate() {
        let band = viz.bands[i % NUM_VIZ_BANDS];
        let wobble = (p.phase * 1.4).sin() * 0.05;
        let r = scale * (p.radius + wobble + band * 0.14);
        let pos = Point::new(center.x + p.angle.cos() * r, center.y + p.angle.sin() * r);
        let size = p.size * (0.75 + band * 1.1 + viz.treble * 0.5);
        let alpha = (0.18 + band * 0.45 + viz.treble * 0.2).min(0.68);

        // Soft outer halo
        frame.fill(
            &Path::circle(pos, size * 2.2),
            pal.sample(p.hue_shift + t * 0.03, alpha * 0.24),
        );
        // Bright core spark
        frame.fill(
            &Path::circle(pos, size),
            pal.sample(p.hue_shift + t * 0.03, alpha),
        );
    }
}

/// Mode 2: **Peppermint Kaleidoscope** — Playful symmetrical rotating petals, crystalline
/// mandala rings, and reactive starburst filaments blooming around the album art.
fn draw_kaleidoscope(frame: &mut Frame, bounds: Rectangle, viz: &VizFrameState, pal: &VizPalette) {
    let center = Point::new(bounds.width * 0.5, bounds.height * 0.48);
    let scale = bounds.width.min(bounds.height) * 0.5;
    let t = viz.time;

    // Rotating peppermint-swirl petals (8-fold symmetry)
    let petals = 8usize;
    let spin = t * 0.28;
    for ring_idx in 0..3 {
        let rf = ring_idx as f32;
        let inner_r = scale * (0.32 + rf * 0.10);
        let outer_base = scale * (0.56 + rf * 0.16 + viz.bass * 0.18);
        let dir = if ring_idx % 2 == 0 { 1.0 } else { -1.0 };

        for p in 0..petals {
            let band = viz.bands[(p * 3 + ring_idx * 4) % NUM_VIZ_BANDS];
            let a0 = (p as f32 / petals as f32) * TAU + spin * dir + rf * 0.24;
            let spread = (TAU / petals as f32) * (0.28 + band * 0.22);
            let tip_r = outer_base + scale * band * 0.26;
            let ctrl_r = (inner_r + tip_r) * 0.56;

            let p_start = Point::new(center.x + (a0 - spread).cos() * inner_r, center.y + (a0 - spread).sin() * inner_r);
            let p_tip = Point::new(center.x + a0.cos() * tip_r, center.y + a0.sin() * tip_r);
            let p_end = Point::new(center.x + (a0 + spread).cos() * inner_r, center.y + (a0 + spread).sin() * inner_r);
            let c1 = Point::new(center.x + (a0 - spread * 0.9).cos() * ctrl_r, center.y + (a0 - spread * 0.9).sin() * ctrl_r);
            let c2 = Point::new(center.x + (a0 + spread * 0.9).cos() * ctrl_r, center.y + (a0 + spread * 0.9).sin() * ctrl_r);

            let petal_path = Path::new(|b| {
                b.move_to(p_start);
                b.quadratic_curve_to(c1, p_tip);
                b.quadratic_curve_to(c2, p_end);
                b.close();
            });

            let hue = (p as f32 / petals as f32 + t * 0.05 + rf * 0.22) % 1.0;
            let fill_alpha = (0.065 + band * 0.14).min(0.24);
            let stroke_alpha = (0.16 + band * 0.36).min(0.55);

            frame.fill(&petal_path, pal.sample(hue, fill_alpha));
            frame.stroke(
                &petal_path,
                Stroke::default()
                    .with_color(pal.sample(hue + 0.15, stroke_alpha))
                    .with_width(1.3 + viz.beat_flash * 1.2),
            );
        }
    }

    // Beat-reactive polygon web
    let sides = 12usize;
    let poly_r = scale * (0.64 + viz.mid * 0.16 + viz.beat_flash * 0.12);
    let mut poly_pts = Vec::with_capacity(sides);
    for i in 0..sides {
        let band = viz.bands[(i * 2) % NUM_VIZ_BANDS];
        let ang = (i as f32 / sides as f32) * TAU - t * 0.22;
        let r = poly_r * (0.9 + band * 0.24);
        poly_pts.push(Point::new(center.x + ang.cos() * r, center.y + ang.sin() * r));
    }

    let poly_path = Path::new(|b| {
        b.move_to(poly_pts[0]);
        for &pt in &poly_pts[1..] {
            b.line_to(pt);
        }
        b.close();
    });
    frame.stroke(
        &poly_path,
        Stroke::default()
            .with_color(pal.sample(t * 0.08, 0.20 + viz.treble * 0.28))
            .with_width(1.2),
    );

    // Expanding beat rings
    for ring in &viz.rings {
        frame.stroke(
            &Path::circle(center, scale * ring.radius),
            Stroke::default()
                .with_color(pal.sample(ring.hue_shift, ring.alpha * 0.55))
                .with_width(ring.thickness),
        );
    }
}

/// Mode 3: **Cosmic Nebula** — Chill lava-lamp translucent plasma blobs drifting lazily across
/// the open window, connected by delicate shimmering constellation threads.
fn draw_nebula(frame: &mut Frame, bounds: Rectangle, viz: &VizFrameState, pal: &VizPalette) {
    let w = bounds.width;
    let h = bounds.height;
    let scale = w.min(h) * 0.5;
    let t = viz.time * 0.65;

    // 7 drifting translucent nebula clouds
    let mut blob_centers = Vec::with_capacity(7);
    for i in 0..7 {
        let fi = i as f32;
        let band = viz.bands[(i * 4) % NUM_VIZ_BANDS];
        let x = w * (0.5 + 0.34 * (t * (0.42 + fi * 0.09) + fi * 1.4).sin());
        let y = h * (0.48 + 0.30 * (t * (0.35 + fi * 0.07) + fi * 2.1).cos());
        let pos = Point::new(x, y);
        blob_centers.push((pos, band, fi));

        let base_r = scale * (0.28 + (fi % 3.0) * 0.08 + band * 0.20 + viz.bass * 0.12);
        let hue = (fi * 0.17 + t * 0.04) % 1.0;

        // Soft multi-ring radial cloud falloff
        for step in (0..4).rev() {
            let sf = (step as f32 + 1.0) / 4.0;
            let r = base_r * (0.45 + sf * 0.65);
            let alpha = (0.04 + band * 0.065) * (1.2 - sf * 0.78);
            frame.fill(&Path::circle(pos, r), pal.sample(hue, alpha));
        }
    }

    // Delicate constellation threads linking nearby clouds & particles
    let max_dist = scale * 0.95;
    for i in 0..blob_centers.len() {
        for j in (i + 1)..blob_centers.len() {
            let (a, ba, _) = blob_centers[i];
            let (b, bb, _) = blob_centers[j];
            let dist = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            if dist < max_dist {
                let proximity = 1.0 - dist / max_dist;
                let alpha = proximity * (0.10 + (ba + bb) * 0.18 + viz.treble * 0.15);
                frame.stroke(
                    &Path::line(a, b),
                    Stroke::default()
                        .with_color(pal.sample((i + j) as f32 * 0.13 + t * 0.05, alpha))
                        .with_width(1.2),
                );
            }
        }
    }

    // Subtle twinkling stars reacting to treble
    let center = Point::new(w * 0.5, h * 0.48);
    for (i, p) in viz.particles.iter().take(28).enumerate() {
        let band = viz.bands[i % NUM_VIZ_BANDS];
        let rx = w * 0.45 * p.radius;
        let ry = h * 0.44 * p.radius;
        let pos = Point::new(center.x + p.angle.cos() * rx, center.y + p.angle.sin() * ry);
        let twinkle = 0.5 + 0.5 * (p.phase * 1.8).sin();
        let size = (1.6 + p.size * 0.55) * (0.7 + band * 0.9);
        let alpha = (0.12 + twinkle * 0.25 + viz.treble * 0.32).min(0.65);
        frame.fill(
            &Path::circle(pos, size),
            pal.sample(p.hue_shift + t * 0.04, alpha),
        );
    }
}

/// Builds a smooth closed Catmull-Rom-like quadratic spline through `pts`.
fn smooth_closed_path(pts: &[Point]) -> Path {
    Path::new(|b| {
        if pts.len() < 3 {
            return;
        }
        let n = pts.len();
        let first_mid = midpoint(pts[n - 1], pts[0]);
        b.move_to(first_mid);
        for i in 0..n {
            let next = pts[(i + 1) % n];
            let mid = midpoint(pts[i], next);
            b.quadratic_curve_to(pts[i], mid);
        }
        b.close();
    })
}

#[inline]
fn midpoint(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}
