//! Sky plot: polar elevation/azimuth view of the selected epoch (design-refresh spec §3.5).
//!
//! Encoding (gap 18): dot color = elevation band (75-90 / 60-75 / 45-60 /
//! 30-45 / 15-30 / 0-15, low-chroma green→grey ramp interpolated between
//! `healthy` and `unavailable`), point type = tracking state (tracked =
//! filled dot + text_primary PRN label, untracked = hollow 1px-stroke dot +
//! text_tertiary label), and a 2px `healthy` ring around dots of satellites
//! used in the PVT solution (ring count therefore equals the used-satellite
//! count). Elevation ring labels (15°…75°) live only on the S radial ±10°
//! tolerance band; PRN labels sit 10px radially outward from their dot and
//! displace 8px radially in ascending-PRN order when they would collide
//! (text blocks <6px apart collide). Fonts are never shrunk and labels are
//! never merged ("75°G03" is a violation). Dot positions are real satellite
//! geometry and are never moved; only labels are displaced.
//!
//! Renders display-only data supplied by `app.rs`; hover/pin state ownership
//! stays in the application.

use super::frame::{self, PlotState};
use crate::app::GnsscopeApp;
use crate::visual_system::{data_font_family, Palette};
use crate::{GuiChannelStatus, GuiSatelliteGeometry};
use gpui_kit::prelude::*;
use gpui_kit::{
    bounds, canvas, div, point, px, quad, size, AnyElement, Bounds, Context, Hsla, Pixels,
    SharedString, Window,
};

/// Container size of the polar view (spec §3.5: circle ≈290px at reference).
pub(crate) const SKY_CANVAS_SIZE: f32 = 290.0;
/// Horizon radius as a fraction of the container (mockup r=140 of 290).
const SKY_RADIUS_FRACTION: f32 = 0.48;
/// Right-hand legend column width (spec §3.5: ~130px).
const LEGEND_COLUMN_WIDTH: f32 = 130.0;

const DOT_DIAMETER: f32 = 7.0;
const DOT_DIAMETER_ACTIVE: f32 = 9.0;
/// PVT-use ring: "点外 2px 细环" = 2px stroke width (spec §3.5).
const PVT_RING_RADIUS: f32 = 6.2;
const PVT_RING_STROKE: f32 = 2.0;

/// PRN label metrics (data 11px, conservative estimate for "G03").
const PRN_LABEL_WIDTH: f32 = 26.0;
const PRN_LABEL_HEIGHT: f32 = 12.0;
/// Text blocks <6px apart collide (spec §3.5 / gap 15).
const LABEL_MIN_GAP: f32 = 6.0;
/// PRN label starts 10px radially outward from its dot.
const LABEL_BASE_OFFSET: f32 = 10.0;
/// Collisions displace the label 8px radially outward.
const LABEL_DISPLACEMENT: f32 = 8.0;
/// Bounded displacement steps; labels are never shrunk or merged.
const MAX_LABEL_STEPS: usize = 4;
/// Tangential escape steps (the S ring-label chain can require a wider
/// sideways step than the radial ladder because the boxes widen toward the
/// horizon).
const MAX_TANGENTIAL_STEPS: usize = 8;

/// Elevation ring labels only live on the S radial within ±10° (spec §3.5).
const RING_LABEL_ELEVATIONS_DEG: [f64; 5] = [15.0, 30.0, 45.0, 60.0, 75.0];
const S_TOLERANCE_DEG: f64 = 10.0;
const RING_LABEL_HEIGHT: f32 = 16.0;
const RING_LABEL_MIN_WIDTH: f32 = 24.0;

// ---------------------------------------------------------------------------
// Display input
// ---------------------------------------------------------------------------

/// One satellite marker of the selected epoch.
pub(crate) struct SkyMarker {
    pub(crate) prn: u8,
    pub(crate) azimuth_deg: f64,
    pub(crate) elevation_deg: f64,
    pub(crate) residual_m: f64,
    pub(crate) cn0_db_hz: Option<f64>,
    /// Channel is actively tracking this PRN (point-type first clue).
    pub(crate) tracked: bool,
    /// Satellite was used in the PVT solution (ring encoding).
    pub(crate) used_in_solution: bool,
}

// ---------------------------------------------------------------------------
// Pure geometry and encoding model (unit-tested)
// ---------------------------------------------------------------------------

/// Polar position inside the unit container: north up, east right.
/// `None` when the geometry is not finite or below the horizon.
pub(crate) fn sky_plot_position(azimuth_deg: f64, elevation_deg: f64) -> Option<(f32, f32)> {
    if !azimuth_deg.is_finite()
        || !elevation_deg.is_finite()
        || !(0.0..=90.0).contains(&elevation_deg)
    {
        return None;
    }
    let radial_fraction = ((90.0 - elevation_deg) / 90.0) as f32;
    let azimuth_rad = azimuth_deg.to_radians();
    Some((
        0.5 + SKY_RADIUS_FRACTION * radial_fraction * azimuth_rad.sin() as f32,
        0.5 - SKY_RADIUS_FRACTION * radial_fraction * azimuth_rad.cos() as f32,
    ))
}

/// Elevation band index 0..=5 for 75-90 / 60-75 / 45-60 / 30-45 / 15-30 /
/// 0-15 (spec §3.5). Values outside 0..=90 are clamped.
pub(crate) fn elevation_band(elevation_deg: f64) -> usize {
    let clamped = elevation_deg.clamp(0.0, 90.0);
    (((90.0 - clamped) / 15.0) as usize).min(5)
}

/// Low-chroma green→grey band ramp interpolated between `healthy` and
/// `unavailable` in the HSL components (saturation and lightness fall toward
/// the neutral unavailable grey; the green hue is kept and desaturated, so
/// the ramp never wanders into the cyan/violet status hues of §7).
pub(crate) fn elevation_band_color(band: usize, palette: Palette) -> Hsla {
    let t = (band.min(5) as f32) / 5.0;
    Hsla {
        h: palette.healthy.h,
        s: palette.healthy.s + (palette.unavailable.s - palette.healthy.s) * t,
        l: palette.healthy.l + (palette.unavailable.l - palette.healthy.l) * t,
        a: 1.0,
    }
}

/// An axis-aligned text block used for collision checks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LabelBox {
    pub(crate) cx: f32,
    pub(crate) cy: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl LabelBox {
    /// Two text blocks closer than `LABEL_MIN_GAP` on both axes collide.
    pub(crate) fn collides_with(self, other: LabelBox) -> bool {
        (self.cx - other.cx).abs() < (self.width + other.width) * 0.5 + LABEL_MIN_GAP
            && (self.cy - other.cy).abs() < (self.height + other.height) * 0.5 + LABEL_MIN_GAP
    }
}

/// A resolved PRN label position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedLabel {
    pub(crate) prn: u8,
    pub(crate) cx: f32,
    pub(crate) cy: f32,
    /// Radial offset from the dot (10 + k·8).
    pub(crate) offset: f32,
    /// Tangential offset perpendicular to the radial (±k·8); used when the
    /// radial path is fully blocked by the ring-label chain so the label
    /// steps sideways out of the S ±10° tolerance band (spec §3.5).
    pub(crate) tangential: f32,
}

/// Ring-label obstacle boxes on the S radial ±10° tolerance band (spec §3.5).
/// Each box spans the band width at its radius so PRN labels never enter the
/// tolerance band around a ring label.
pub(crate) fn ring_label_boxes(canvas_size: f32) -> Vec<LabelBox> {
    let center = canvas_size * 0.5;
    let radius = canvas_size * SKY_RADIUS_FRACTION;
    RING_LABEL_ELEVATIONS_DEG
        .into_iter()
        .map(|elevation| {
            let ring_radius = radius * ((90.0 - elevation) / 90.0) as f32;
            let band_width = (ring_radius * 2.0 * (S_TOLERANCE_DEG.to_radians().sin() as f32))
                .max(RING_LABEL_MIN_WIDTH);
            LabelBox {
                cx: center,
                cy: center + ring_radius + 5.0,
                width: band_width,
                height: RING_LABEL_HEIGHT,
            }
        })
        .collect()
}

/// Resolve PRN label positions: start 10px radially outward from the dot,
/// displace 8px radially outward per step in ascending-PRN order when the
/// label would collide with an obstacle (ring labels) or an
/// already-placed PRN label. When the radial path is fully blocked — the S
/// ring-label chain blocks every radial step — the label steps 8px
/// tangentially (perpendicular to the radial, positive side first) until it
/// clears the tolerance band. Every input marker produces exactly one label;
/// fonts are never shrunk and labels are never merged (spec §3.5 / gap 15).
pub(crate) fn place_prn_labels(
    markers: &[(u8, f32, f32)],
    obstacles: &[LabelBox],
    canvas_size: f32,
) -> Vec<PlacedLabel> {
    let center = canvas_size * 0.5;
    let mut sorted = markers.to_vec();
    sorted.sort_by_key(|(prn, _, _)| *prn);
    let mut placed: Vec<PlacedLabel> = Vec::with_capacity(sorted.len());
    let mut placed_boxes: Vec<LabelBox> = Vec::with_capacity(sorted.len());
    for (prn, dot_x, dot_y) in sorted {
        let (mut dir_x, mut dir_y) = (dot_x - center, dot_y - center);
        let length = (dir_x * dir_x + dir_y * dir_y).sqrt();
        if length < f32::EPSILON {
            // Zenith: fall back to the north radial so labels still fan out.
            (dir_x, dir_y) = (0.0, -1.0);
        } else {
            dir_x /= length;
            dir_y /= length;
        }
        // Perpendicular unit vector for the tangential escape.
        let (perp_x, perp_y) = (-dir_y, dir_x);
        let collides_at = |offset: f32, tangential: f32| {
            let candidate = LabelBox {
                cx: dot_x + dir_x * offset + perp_x * tangential,
                cy: dot_y + dir_y * offset + perp_y * tangential,
                width: PRN_LABEL_WIDTH,
                height: PRN_LABEL_HEIGHT,
            };
            obstacles
                .iter()
                .chain(placed_boxes.iter())
                .any(|obstacle| candidate.collides_with(*obstacle))
        };
        // Pass 1: radial displacement (the primary spec rule).
        let mut chosen = (
            LABEL_BASE_OFFSET + LABEL_DISPLACEMENT * MAX_LABEL_STEPS as f32,
            0.0,
        );
        let mut found = false;
        for step in 0..=MAX_LABEL_STEPS {
            let offset = LABEL_BASE_OFFSET + LABEL_DISPLACEMENT * step as f32;
            if !collides_at(offset, 0.0) {
                chosen = (offset, 0.0);
                found = true;
                break;
            }
        }
        // Pass 2: the radial path is fully blocked (ring-label chain); step
        // tangentially out of the S ±10° band at the base radial offset.
        if !found {
            for step in 1..=MAX_TANGENTIAL_STEPS {
                let tangential = LABEL_DISPLACEMENT * step as f32;
                for sign in [1.0, -1.0] {
                    if !collides_at(LABEL_BASE_OFFSET, tangential * sign) {
                        chosen = (LABEL_BASE_OFFSET, tangential * sign);
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }
        }
        let (offset, tangential) = chosen;
        let label_x = dot_x + dir_x * offset + perp_x * tangential;
        let label_y = dot_y + dir_y * offset + perp_y * tangential;
        placed_boxes.push(LabelBox {
            cx: label_x,
            cy: label_y,
            width: PRN_LABEL_WIDTH,
            height: PRN_LABEL_HEIGHT,
        });
        placed.push(PlacedLabel {
            prn,
            cx: label_x,
            cy: label_y,
            offset,
            tangential,
        });
    }
    placed
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render the sky plot for the selected epoch.
pub(crate) fn render(
    satellites: &[GuiSatelliteGeometry],
    channels: &[GuiChannelStatus],
    hovered_prn: Option<u8>,
    pinned_prn: Option<u8>,
    palette: Palette,
    cx: &mut Context<GnsscopeApp>,
) -> AnyElement {
    let markers = satellites
        .iter()
        .filter_map(|satellite| {
            sky_plot_position(satellite.azimuth_deg, satellite.elevation_deg)?;
            let channel = channels.iter().find(|channel| channel.prn == satellite.prn);
            Some(SkyMarker {
                prn: satellite.prn,
                azimuth_deg: satellite.azimuth_deg,
                elevation_deg: satellite.elevation_deg,
                residual_m: satellite.residual_m,
                cn0_db_hz: channel.map(|channel| channel.cn0_db_hz),
                tracked: channel.is_some_and(|channel| channel.tracking_locked),
                used_in_solution: satellite.used_in_solution,
            })
        })
        .collect::<Vec<_>>();
    let geometry_available = !markers.is_empty();
    let active_prn = hovered_prn.or(pinned_prn);
    let active_marker = active_prn.and_then(|prn| markers.iter().find(|marker| marker.prn == prn));
    let detail = active_marker.map(|marker| {
        format!(
            "PRN {:02}  ·  C/N0 {}  ·  方位 {:.1}°  ·  仰角 {:.1}°  ·  残差 {:+.2} m{}",
            marker.prn,
            marker
                .cn0_db_hz
                .map(|value| format!("{value:.1} dB-Hz"))
                .unwrap_or_else(|| "—".to_string()),
            marker.azimuth_deg,
            marker.elevation_deg,
            marker.residual_m,
            if marker.used_in_solution {
                "  ·  PVT"
            } else {
                ""
            },
        )
    });

    // Pixel geometry for canvas painting and DOM labels.
    let radius = SKY_CANVAS_SIZE * SKY_RADIUS_FRACTION;
    let center = SKY_CANVAS_SIZE * 0.5;
    let dots = markers
        .iter()
        .map(|marker| {
            let (fraction_x, fraction_y) =
                sky_plot_position(marker.azimuth_deg, marker.elevation_deg).unwrap_or((0.5, 0.5));
            let band = elevation_band(marker.elevation_deg);
            CanvasDot {
                x: fraction_x * SKY_CANVAS_SIZE,
                y: fraction_y * SKY_CANVAS_SIZE,
                color: elevation_band_color(band, palette),
                tracked: marker.tracked,
                used_in_solution: marker.used_in_solution,
                active: active_prn == Some(marker.prn),
            }
        })
        .collect::<Vec<_>>();
    let obstacles = ring_label_boxes(SKY_CANVAS_SIZE);
    // The canvas closure owns a copy (`CanvasDot` is `Copy`); the label pass
    // below still needs positions from `dots`.
    let canvas_dots = dots.clone();
    let labels = place_prn_labels(
        &markers
            .iter()
            .zip(dots.iter())
            .map(|(marker, dot)| (marker.prn, dot.x, dot.y))
            .collect::<Vec<_>>(),
        &obstacles,
        SKY_CANVAS_SIZE,
    );

    div()
        .size_full()
        .relative()
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .w(px(SKY_CANVAS_SIZE))
                        .h(px(SKY_CANVAS_SIZE))
                        .child(
                            canvas(
                                move |_, _, _| {},
                                move |plot_bounds, _, window, _| {
                                    paint_sky_grid(&plot_bounds, radius, center, palette, window);
                                    paint_sky_dots(&plot_bounds, &canvas_dots, palette, window);
                                },
                            )
                            .size_full(),
                        )
                        // Cardinal tick labels (tick 10px, text_secondary).
                        .children(
                            [
                                ("N", 0.0_f32, -1.0_f32),
                                ("E", 1.0, 0.0),
                                ("S", 0.0, 1.0),
                                ("W", -1.0, 0.0),
                            ]
                            .into_iter()
                            .map(|(label, dir_x, dir_y)| {
                                div()
                                    .absolute()
                                    .left(px(center + dir_x * (radius + 10.0) - 7.0))
                                    .top(px(center + dir_y * (radius + 10.0) - 7.0))
                                    .w(px(14.))
                                    .h(px(14.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.) /* MICRO */)
                                    .text_color(palette.muted)
                                    .child(label)
                            }),
                        )
                        // Elevation ring labels along the S radial only.
                        .children(RING_LABEL_ELEVATIONS_DEG.into_iter().map(|elevation| {
                            let ring_radius = radius * ((90.0 - elevation) / 90.0) as f32;
                            div()
                                .absolute()
                                .left(px(center - RING_LABEL_MIN_WIDTH * 0.5))
                                .top(px(center + ring_radius + 5.0 - 6.0))
                                .w(px(RING_LABEL_MIN_WIDTH))
                                .text_center()
                                .font_family(data_font_family())
                                .text_size(px(10.) /* MICRO */)
                                .text_color(palette.faint)
                                .child(format!("{elevation:.0}°"))
                        }))
                        // PRN labels with hover/pin interactivity.
                        .children(labels.into_iter().map(|label| {
                            let prn = label.prn;
                            let (dot_x, dot_y) = markers
                                .iter()
                                .zip(dots.iter())
                                .find(|(marker, _)| marker.prn == prn)
                                .map(|(_, dot)| (dot.x, dot.y))
                                .unwrap_or((center, center));
                            let marker = markers
                                .iter()
                                .find(|marker| marker.prn == prn)
                                .expect("label marker exists");
                            let active = active_prn == Some(prn);
                            div()
                                .id(SharedString::from(format!("sky-prn-{prn}")))
                                .absolute()
                                .left(px(dot_x - 12.0))
                                .top(px(dot_y - 12.0))
                                .w(px(24.))
                                .h(px(24.))
                                .cursor_pointer()
                                .on_hover(cx.listener(move |app, hovered, _, cx| {
                                    if *hovered {
                                        app.hovered_prn = Some(prn);
                                    } else if app.hovered_prn == Some(prn) {
                                        app.hovered_prn = None;
                                    }
                                    cx.notify();
                                }))
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    app.pinned_prn = (app.pinned_prn != Some(prn)).then_some(prn);
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(label.cx - dot_x + 12.0 - PRN_LABEL_WIDTH * 0.5))
                                        .top(px(label.cy - dot_y + 12.0 - PRN_LABEL_HEIGHT * 0.5))
                                        .w(px(PRN_LABEL_WIDTH))
                                        .h(px(PRN_LABEL_HEIGHT))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .font_family(data_font_family())
                                        .text_size(px(11.) /* MEASUREMENT_SMALL */)
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .text_color(if active {
                                            palette.accent
                                        } else if marker.tracked {
                                            palette.text
                                        } else {
                                            palette.faint
                                        })
                                        .child(format!("G{prn:02}")),
                                )
                        })),
                ),
        )
        // Legend column: only genuinely encoded items (gap 18).
        .child(
            div()
                .w(px(LEGEND_COLUMN_WIDTH))
                .flex_shrink_0()
                .self_start()
                .pt(px(18.))
                .pl(px(12.))
                .text_size(px(11.) /* CAPTION */)
                .text_color(palette.muted)
                .child(div().mb(px(6.)).child("高度角（°）"))
                .children(
                    [
                        ("75–90", 0),
                        ("60–75", 1),
                        ("45–60", 2),
                        ("30–45", 3),
                        ("15–30", 4),
                        ("0–15", 5),
                    ]
                    .into_iter()
                    .map(|(label, band)| {
                        div()
                            .h(px(17.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .w(px(14.))
                                    .h(px(8.))
                                    .bg(elevation_band_color(band, palette)),
                            )
                            .child(
                                div()
                                    .font_family(data_font_family())
                                    .text_size(px(10.) /* MICRO */)
                                    .text_color(palette.faint)
                                    .child(label),
                            )
                    }),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(palette.border)
                        .child(div().mb(px(4.)).child("跟踪状态"))
                        .child(
                            div()
                                .h(px(17.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(div().size(px(6.)).rounded_full().bg(palette.text))
                                .child(div().child("跟踪")),
                        )
                        .child(
                            div()
                                .h(px(17.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .size(px(6.))
                                        .rounded_full()
                                        .bg(palette.surface)
                                        .border_1()
                                        .border_color(palette.text),
                                )
                                .child(div().child("未跟踪")),
                        ),
                ),
        )
        .children((!geometry_available).then(|| frame::state_caption(&PlotState::Empty, palette)))
        .children(detail.map(|detail| {
            div()
                .absolute()
                .bottom(px(0.))
                .left_0()
                .right_0()
                .text_center()
                .text_size(px(10.) /* MICRO */)
                .text_color(palette.text)
                .child(detail)
        }))
        .into_any_element()
}

#[derive(Clone, Copy)]
struct CanvasDot {
    x: f32,
    y: f32,
    color: Hsla,
    tracked: bool,
    used_in_solution: bool,
    active: bool,
}

fn paint_sky_grid(
    plot_bounds: &Bounds<Pixels>,
    radius: f32,
    center: f32,
    palette: Palette,
    window: &mut Window,
) {
    let origin_x = plot_bounds.origin.x + (plot_bounds.size.width - px(SKY_CANVAS_SIZE)) * 0.5;
    let origin_y = plot_bounds.origin.y + (plot_bounds.size.height - px(SKY_CANVAS_SIZE)) * 0.5;
    for ratio in [1.0_f32, 5.0 / 6.0, 2.0 / 3.0, 0.5, 1.0 / 3.0, 1.0 / 6.0] {
        let ring_diameter = radius * 2.0 * ratio;
        let role = if ratio == 1.0 {
            frame::LineRole::Axis
        } else {
            frame::LineRole::Grid
        };
        window.paint_quad(quad(
            bounds(
                point(
                    origin_x + px(center - ring_diameter * 0.5),
                    origin_y + px(center - ring_diameter * 0.5),
                ),
                size(px(ring_diameter), px(ring_diameter)),
            ),
            ring_diameter * 0.5,
            palette.surface,
            px(frame::LINE_STROKE_WIDTH),
            match role {
                frame::LineRole::Grid => palette.plot_grid,
                frame::LineRole::Axis => palette.plot_axis,
            },
            Default::default(),
        ));
    }
    // Cardinal tick marks on the horizon (5px, plot_axis).
    for (dir_x, dir_y) in [(0.0_f32, -1.0_f32), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
        let tick_x = origin_x + px(center + dir_x * radius);
        let tick_y = origin_y + px(center + dir_y * radius);
        if dir_x.abs() < f32::EPSILON {
            window.paint_quad(quad(
                bounds(
                    point(
                        tick_x - px(0.5),
                        tick_y - if dir_y < 0.0 { px(8.) } else { px(0.) },
                    ),
                    size(px(1.), px(5.)),
                ),
                px(0.),
                palette.plot_axis,
                px(0.),
                palette.plot_axis,
                Default::default(),
            ));
        } else {
            window.paint_quad(quad(
                bounds(
                    point(
                        tick_x - if dir_x < 0.0 { px(8.) } else { px(0.) },
                        tick_y - px(0.5),
                    ),
                    size(px(5.), px(1.)),
                ),
                px(0.),
                palette.plot_axis,
                px(0.),
                palette.plot_axis,
                Default::default(),
            ));
        }
    }
}

fn paint_sky_dots(
    plot_bounds: &Bounds<Pixels>,
    dots: &[CanvasDot],
    palette: Palette,
    window: &mut Window,
) {
    let origin_x = plot_bounds.origin.x + (plot_bounds.size.width - px(SKY_CANVAS_SIZE)) * 0.5;
    let origin_y = plot_bounds.origin.y + (plot_bounds.size.height - px(SKY_CANVAS_SIZE)) * 0.5;
    for dot in dots {
        // PVT-use ring: 2px healthy stroke outside the dot (spec §3.5).
        if dot.used_in_solution {
            window.paint_quad(quad(
                bounds(
                    point(
                        origin_x + px(dot.x) - px(PVT_RING_RADIUS),
                        origin_y + px(dot.y) - px(PVT_RING_RADIUS),
                    ),
                    size(px(PVT_RING_RADIUS * 2.0), px(PVT_RING_RADIUS * 2.0)),
                ),
                px(PVT_RING_RADIUS),
                Hsla::transparent_black(),
                px(PVT_RING_STROKE),
                palette.healthy,
                Default::default(),
            ));
        }
        let diameter = if dot.active {
            DOT_DIAMETER_ACTIVE
        } else {
            DOT_DIAMETER
        };
        window.paint_quad(quad(
            bounds(
                point(
                    origin_x + px(dot.x) - px(diameter * 0.5),
                    origin_y + px(dot.y) - px(diameter * 0.5),
                ),
                size(px(diameter), px(diameter)),
            ),
            px(diameter * 0.5),
            if dot.tracked {
                dot.color
            } else {
                palette.surface
            },
            px(if dot.tracked { 0. } else { 1. }),
            dot.color,
            Default::default(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_plot_uses_north_up_and_east_right() {
        let north_horizon = sky_plot_position(0.0, 0.0).unwrap();
        let east_horizon = sky_plot_position(90.0, 0.0).unwrap();
        let zenith = sky_plot_position(225.0, 90.0).unwrap();

        assert!((north_horizon.0 - 0.5).abs() < 1e-6);
        assert!(north_horizon.1 < 0.5);
        assert!(east_horizon.0 > 0.5);
        assert!((east_horizon.1 - 0.5).abs() < 1e-6);
        assert!((zenith.0 - 0.5).abs() < 1e-6);
        assert!((zenith.1 - 0.5).abs() < 1e-6);
        assert!(sky_plot_position(0.0, -0.1).is_none());
    }

    #[test]
    fn elevation_bands_run_high_to_low_green_to_grey() {
        // Right-closed bands: (75,90], (60,75], (45,60], (30,45], (15,30],
        // [0,15] — a satellite exactly on a ring line falls into the lower
        // band, consistently for every boundary.
        assert_eq!(elevation_band(89.0), 0);
        assert_eq!(elevation_band(75.0), 1);
        assert_eq!(elevation_band(74.9), 1);
        assert_eq!(elevation_band(60.0), 2);
        assert_eq!(elevation_band(45.1), 2);
        assert_eq!(elevation_band(45.0), 3);
        assert_eq!(elevation_band(0.0), 5);
        // Out-of-range values clamp to the nearest band (120 → 90 → band 0).
        assert_eq!(elevation_band(120.0), 0);
        assert_eq!(elevation_band(-5.0), 5);

        let palette = Palette::dark();
        let mut previous_saturation = 1.0;
        for band in 0..=5 {
            let color = elevation_band_color(band, palette);
            assert!(color.s <= previous_saturation + 1e-6);
            previous_saturation = color.s;
        }
        let band0 = elevation_band_color(0, palette);
        let band5 = elevation_band_color(5, palette);
        assert!((band0.s - palette.healthy.s).abs() < 1e-6);
        assert!((band5.s - palette.unavailable.s).abs() < 1e-6);
    }

    #[test]
    fn prn_labels_displace_radially_in_ascending_prn_order() {
        // Two dots close together above the center; the lower PRN keeps the
        // 10px base offset, the higher PRN displaces 8px radially outward.
        let center = SKY_CANVAS_SIZE * 0.5;
        let markers = [
            (9_u8, center + 4.0, center - 30.0),
            (5_u8, center, center - 30.0),
        ];
        let placed = place_prn_labels(&markers, &[], SKY_CANVAS_SIZE);

        assert_eq!(placed.len(), 2);
        let prn5 = placed.iter().find(|label| label.prn == 5).unwrap();
        let prn9 = placed.iter().find(|label| label.prn == 9).unwrap();
        assert_eq!(prn5.offset, LABEL_BASE_OFFSET);
        assert!(prn9.offset > LABEL_BASE_OFFSET);
        assert_eq!((prn9.offset - LABEL_BASE_OFFSET) % LABEL_DISPLACEMENT, 0.0);
        let box5 = LabelBox {
            cx: prn5.cx,
            cy: prn5.cy,
            width: PRN_LABEL_WIDTH,
            height: PRN_LABEL_HEIGHT,
        };
        let box9 = LabelBox {
            cx: prn9.cx,
            cy: prn9.cy,
            width: PRN_LABEL_WIDTH,
            height: PRN_LABEL_HEIGHT,
        };
        assert!(!box5.collides_with(box9));
    }

    #[test]
    fn prn_labels_avoid_the_s_ring_label_band() {
        // A satellite due south at 45° would place its label right where the
        // "45°" ring label lives; the whole radial path is blocked by the
        // ring-label chain, so the label steps tangentially out of the band.
        let (dot_x, dot_y) = sky_plot_position(180.0, 45.0).unwrap();
        let dot_x = dot_x * SKY_CANVAS_SIZE;
        let dot_y = dot_y * SKY_CANVAS_SIZE;
        let obstacles = ring_label_boxes(SKY_CANVAS_SIZE);
        let placed = place_prn_labels(&[(3_u8, dot_x, dot_y)], &obstacles, SKY_CANVAS_SIZE);

        assert_eq!(placed.len(), 1);
        let label = &placed[0];
        assert!(
            label.tangential != 0.0,
            "label on the S radial must displace off the ring label band"
        );
        assert_eq!(label.offset, LABEL_BASE_OFFSET);
        let label_box = LabelBox {
            cx: label.cx,
            cy: label.cy,
            width: PRN_LABEL_WIDTH,
            height: PRN_LABEL_HEIGHT,
        };
        assert!(obstacles
            .iter()
            .all(|obstacle| !label_box.collides_with(*obstacle)));
    }

    #[test]
    fn label_placement_never_merges_or_drops_labels() {
        // A pathological cluster: every marker still gets exactly one label
        // (fonts are never shrunk and texts are never merged, spec §3.5).
        let center = SKY_CANVAS_SIZE * 0.5;
        let markers: Vec<(u8, f32, f32)> = (1..=9)
            .map(|prn| (prn, center + (prn as f32) * 0.4, center - 28.0))
            .collect();
        let placed = place_prn_labels(&markers, &[], SKY_CANVAS_SIZE);
        assert_eq!(placed.len(), 9);
        for label in &placed {
            assert_eq!((label.offset - LABEL_BASE_OFFSET) % LABEL_DISPLACEMENT, 0.0);
            assert!(label.offset >= LABEL_BASE_OFFSET);
        }
    }
}
