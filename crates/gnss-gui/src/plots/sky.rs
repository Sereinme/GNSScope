//! Sky plot: polar elevation/azimuth view of the selected epoch (design-refresh spec §3.5).
//!
//! Encoding (gap 18): dot color = elevation band (75-90 / 60-75 / 45-60 /
//! 30-45 / 15-30 / 0-15, low-chroma green→grey ramp interpolated between
//! `healthy` and `unavailable`), point type = tracking state (tracked =
//! filled dot + text_primary PRN label, untracked = hollow 1px-stroke dot +
//! text_tertiary label), and a 2px `healthy` ring around dots of satellites
//! used in the PVT solution (ring count therefore equals the used-satellite
//! count). Elevation ring labels (15°…75°) live only on the S radial ±10°
//! tolerance band; PRN labels sit with their near edge 10px beyond the
//! marker along the radial (the label box's projection is accounted for, so
//! a label never overlaps its own dot/ring at any azimuth) and displace 8px
//! radially in ascending-PRN order when they would collide (text blocks <6px
//! apart collide). Fonts are never shrunk and labels are never merged
//! ("75°G03" is a violation). Dot positions are real satellite geometry and
//! are never moved; only labels are displaced. The horizon radius fraction
//! (0.42) is chosen so the cardinal labels (10px beyond the horizon) stay
//! inside the fixed 290px container: a larger circle overflows the plot
//! well and clips S at the pane bottom while pushing N into the header.
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
pub(crate) const SKY_CANVAS_SIZE: f32 = 312.0;
/// Horizon radius as a fraction of the container. 0.40 (not the mockup's
/// 0.48): the cardinal labels sit 10px beyond the horizon and must remain
/// INSIDE this fixed 290px box — the actual plot well can be shorter than
/// the box plus padding, and any overflow clips S at the bottom and pushes
/// N into the pane header. With 0.42 every cardinal fits with ≥6px to
/// spare, so the box never needs to outgrow the circle container.
const SKY_RADIUS_FRACTION: f32 = 0.40;
/// Right-hand legend column width (spec §3.5: ~130px).
const LEGEND_COLUMN_WIDTH: f32 = 130.0;

const DOT_DIAMETER: f32 = 7.0;
const DOT_DIAMETER_ACTIVE: f32 = 9.0;
/// PVT-use ring: "点外 2px 细环" = 2px stroke width (spec §3.5).
const PVT_RING_RADIUS: f32 = 7.4;
const PVT_RING_STROKE: f32 = 1.6;
/// Surface-colored separator between the dot and the halo ring.
const PVT_GAP_RADIUS: f32 = 5.6;
const PVT_GAP_STROKE: f32 = 2.4;

/// PRN label metrics (data 11px, conservative estimate for "G03").
const PRN_LABEL_WIDTH: f32 = 26.0;
const PRN_LABEL_HEIGHT: f32 = 12.0;
/// Text blocks <6px apart collide (spec §3.5 / gap 15).
const LABEL_MIN_GAP: f32 = 6.0;
/// Gap between the marker's outer edge (dot or PVT ring) and the label's
/// near edge (spec §3.5: "PRN 标签置于点外 10px 沿径向").
const LABEL_DOT_GAP: f32 = 10.0;
/// Marker extent the label must clear: the dot's radius. The PVT halo is
/// drawn as a thin ring outside the dot, so clearing the dot plus the spec's
/// 10px gap already keeps the label off the halo.
const MARKER_EXTENT: f32 = DOT_DIAMETER_ACTIVE * 0.5;
/// Collisions displace the label 8px radially outward.
const LABEL_DISPLACEMENT: f32 = 8.0;
/// Bounded displacement steps; labels are never shrunk or merged.
const MAX_LABEL_STEPS: usize = 4;

/// Elevation ring labels only live on the S radial within ±10° (spec §3.5).
const RING_LABEL_ELEVATIONS_DEG: [f64; 5] = [15.0, 30.0, 45.0, 60.0, 75.0];
const S_TOLERANCE_DEG: f64 = 10.0;
const RING_LABEL_HEIGHT: f32 = 16.0;
const RING_LABEL_MIN_WIDTH: f32 = 24.0;
/// Keep-out inset for clamped PRN labels so nothing clips at the pane edge.
const LABEL_CANVAS_INSET: f32 = 2.0;

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

/// Elevation band color from the palette's ordered six-step ramp. The ramp
/// steps lightness in a single green hue so the bands read as a scale instead
/// of six near-identical greens (design pass); it never borrows the cyan or
/// violet status hues (spec §7).
pub(crate) fn elevation_band_color(band: usize, palette: Palette) -> Hsla {
    palette.elevation_bands[band.min(5)]
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
    /// Radial offset of the label center from the dot (`label_base_offset`
    /// + k·8 displacement steps).
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

/// All obstacles for PRN label placement: the S-radial ring labels plus the
/// four cardinal labels, so a low-elevation satellite's outward label can
/// never cover N/E/S/W (it displaces radially outward past the cardinal).
pub(crate) fn sky_label_obstacles(canvas_size: f32) -> Vec<LabelBox> {
    let mut obstacles = ring_label_boxes(canvas_size);
    let center = canvas_size * 0.5;
    let radius = canvas_size * SKY_RADIUS_FRACTION;
    for (dir_x, dir_y) in [(0.0_f32, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
        obstacles.push(LabelBox {
            cx: center + dir_x * (radius + 10.0),
            cy: center + dir_y * (radius + 10.0),
            width: 14.0,
            height: 14.0,
        });
    }
    obstacles
}

/// Top-left of a cardinal label box inside the circle container. With the
/// 0.42 radius fraction every cardinal stays fully inside the 290px box
/// (the test pins this), so N can never reach the pane header and S never
/// clips at the pane bottom.
pub(crate) fn cardinal_label_origin(dir_x: f32, dir_y: f32) -> (f32, f32) {
    let center = SKY_CANVAS_SIZE * 0.5;
    let radius = SKY_CANVAS_SIZE * SKY_RADIUS_FRACTION;
    (
        center + dir_x * (radius + 10.0) - 7.0,
        center + dir_y * (radius + 10.0) - 7.0,
    )
}

/// Resolve PRN label positions: each label hugs its marker, placed in the
/// first free direction of a fixed preference order (right, left, up, down,
/// then the four diagonals), with its near edge `LABEL_DOT_GAP` beyond the
/// marker extent. A candidate is rejected when it leaves the canvas, enters
/// the S ±10° ring-label band, or collides with a cardinal or an already
/// placed label; the distance then grows in 8px steps. Every input marker
/// produces exactly one label, fonts are never shrunk and labels are never
/// merged (spec §3.5 / gap 15) — the hugging placement replaces the outward
/// radial ladder, which pushed horizon labels off the pane edge.
pub(crate) fn place_prn_labels(
    markers: &[(u8, f32, f32)],
    obstacles: &[LabelBox],
    canvas_size: f32,
) -> Vec<PlacedLabel> {
    let mut sorted = markers.to_vec();
    sorted.sort_by_key(|(prn, _, _)| *prn);
    let mut placed: Vec<PlacedLabel> = Vec::with_capacity(sorted.len());
    let mut placed_boxes: Vec<LabelBox> = Vec::with_capacity(sorted.len());
    let half_w = PRN_LABEL_WIDTH * 0.5;
    let half_h = PRN_LABEL_HEIGHT * 0.5;
    let clearance = MARKER_EXTENT + LABEL_DOT_GAP;

    for (prn, dot_x, dot_y) in sorted {
        // Candidate sides, ordered by how far outward they point from the
        // plot centre: a label fans away from the constellation rather than
        // crossing it, which keeps each label visually attached to its own
        // marker (design pass).
        let mut directions: [(f32, f32, f32); 8] = [
            (1.0, 0.0, 0.0),
            (-1.0, 0.0, 0.0),
            (0.0, -1.0, 0.0),
            (0.0, 1.0, 0.0),
            (1.0, -1.0, 0.0),
            (-1.0, -1.0, 0.0),
            (1.0, 1.0, 0.0),
            (-1.0, 1.0, 0.0),
        ];
        {
            let centre = canvas_size * 0.5;
            let (mut out_x, mut out_y) = (dot_x - centre, dot_y - centre);
            let length = (out_x * out_x + out_y * out_y).sqrt();
            if length > f32::EPSILON {
                out_x /= length;
                out_y /= length;
            } else {
                (out_x, out_y) = (0.0, -1.0);
            }
            for (dir_x, dir_y, score) in directions.iter_mut() {
                let (dir_x, dir_y) = (*dir_x, *dir_y);
                let norm = (dir_x * dir_x + dir_y * dir_y).sqrt();
                *score = (dir_x * out_x + dir_y * out_y) / norm;
            }
            directions.sort_by(|a, b| b.2.total_cmp(&a.2));
        }
        let accepted = |cx: f32, cy: f32| {
            let box_ = LabelBox {
                cx,
                cy,
                width: PRN_LABEL_WIDTH,
                height: PRN_LABEL_HEIGHT,
            };
            let inside = cx - half_w >= LABEL_CANVAS_INSET
                && cx + half_w <= canvas_size - LABEL_CANVAS_INSET
                && cy - half_h >= LABEL_CANVAS_INSET
                && cy + half_h <= canvas_size - LABEL_CANVAS_INSET;
            inside
                && !obstacles.iter().any(|o| box_.collides_with(*o))
                && !placed_boxes.iter().any(|o| box_.collides_with(*o))
        };

        let mut chosen: Option<(f32, f32, f32, f32)> = None;
        'search: for step in 0..=MAX_LABEL_STEPS as u32 {
            let grow = LABEL_DISPLACEMENT * step as f32;
            for (dir_x, dir_y, _) in directions {
                let dx = if dir_x > 0.0 {
                    clearance + half_w + grow
                } else if dir_x < 0.0 {
                    -(clearance + half_w + grow)
                } else {
                    0.0
                };
                let dy = if dir_y > 0.0 {
                    clearance + half_h + grow
                } else if dir_y < 0.0 {
                    -(clearance + half_h + grow)
                } else {
                    0.0
                };
                let cx = dot_x + dx;
                let cy = dot_y + dy;
                if accepted(cx, cy) {
                    chosen = Some((cx, cy, dx, dy));
                    break 'search;
                }
            }
        }
        let (cx, cy, dx, dy) = chosen.unwrap_or_else(|| {
            // Last resort: clamp the first preference into the canvas. This
            // only triggers in a fully packed plot; the label stays visible.
            let cx = (dot_x + clearance + half_w).clamp(
                LABEL_CANVAS_INSET + half_w,
                canvas_size - LABEL_CANVAS_INSET - half_w,
            );
            let cy = dot_y.clamp(
                LABEL_CANVAS_INSET + half_h,
                canvas_size - LABEL_CANVAS_INSET - half_h,
            );
            (cx, cy, cx - dot_x, cy - dot_y)
        });
        placed_boxes.push(LabelBox {
            cx,
            cy,
            width: PRN_LABEL_WIDTH,
            height: PRN_LABEL_HEIGHT,
        });
        placed.push(PlacedLabel {
            prn,
            cx,
            cy,
            offset: dx.hypot(dy),
            tangential: dy,
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
    let obstacles = sky_label_obstacles(SKY_CANVAS_SIZE);
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
                    // Single 290px box: with the 0.42 radius fraction the
                    // cardinal labels fit inside it (see SKY_RADIUS_FRACTION),
                    // so the box never overflows the plot well.
                    div()
                        .relative()
                        .size(px(SKY_CANVAS_SIZE))
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
                        }))
                        // Cardinal tick labels (tick 10px, text_secondary);
                        // `cardinal_label_origin` keeps every cardinal
                        // inside the box.
                        .children(
                            [
                                ("N", 0.0_f32, -1.0_f32),
                                ("E", 1.0, 0.0),
                                ("S", 0.0, 1.0),
                                ("W", -1.0, 0.0),
                            ]
                            .into_iter()
                            .map(|(label, dir_x, dir_y)| {
                                let (left, top) = cardinal_label_origin(dir_x, dir_y);
                                div()
                                    .absolute()
                                    .left(px(left))
                                    .top(px(top))
                                    .w(px(14.))
                                    .h(px(14.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.) /* MICRO */)
                                    .text_color(palette.faint)
                                    .child(label)
                            }),
                        ),
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
        // PVT-use halo (spec §3.5): a thin healthy ring outside the dot. A
        // surface-colored gap ring sits between them so the halo reads as a
        // separate encoding instead of thickening the dot itself.
        if dot.used_in_solution {
            window.paint_quad(quad(
                bounds(
                    point(
                        origin_x + px(dot.x) - px(PVT_GAP_RADIUS),
                        origin_y + px(dot.y) - px(PVT_GAP_RADIUS),
                    ),
                    size(px(PVT_GAP_RADIUS * 2.0), px(PVT_GAP_RADIUS * 2.0)),
                ),
                px(PVT_GAP_RADIUS),
                Hsla::transparent_black(),
                px(PVT_GAP_STROKE),
                palette.surface,
                Default::default(),
            ));
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

        // The six bands must read as an ordered scale: each step is a
        // perceptibly different green, and the ramp steps monotonically in
        // lightness (bright→dim on graphite, dark→light on paper).
        for (name, palette, descending) in [
            ("dark", Palette::dark(), true),
            ("light", Palette::light(), false),
        ] {
            let mut seen = Vec::new();
            let mut previous = None;
            for band in 0..=5 {
                let color = elevation_band_color(band, palette);
                assert!(
                    !seen.contains(&format!("{:.4}", color.l)),
                    "{name} band {band} repeats an earlier lightness"
                );
                seen.push(format!("{:.4}", color.l));
                if let Some(previous) = previous {
                    let delta = color.l - previous;
                    assert!(
                        if descending {
                            delta < -0.03
                        } else {
                            delta > 0.03
                        },
                        "{name} band {band} lightness step {delta:.3} is too small to read"
                    );
                }
                previous = Some(color.l);
            }
        }
    }

    #[test]
    fn prn_labels_displace_in_ascending_prn_order() {
        // Two dots close together: the lower PRN keeps its hugging position,
        // the higher PRN yields and takes a farther slot.
        let center = SKY_CANVAS_SIZE * 0.5;
        let markers = [
            (9_u8, center + 4.0, center - 30.0),
            (5_u8, center, center - 30.0),
        ];
        let placed = place_prn_labels(&markers, &[], SKY_CANVAS_SIZE);

        assert_eq!(placed.len(), 2);
        let prn5 = placed.iter().find(|label| label.prn == 5).unwrap();
        let prn9 = placed.iter().find(|label| label.prn == 9).unwrap();
        // PRN 5 takes the nearest free slot (its outward side, above the
        // dot); PRN 9 has to move to a different one.
        let nearest = MARKER_EXTENT + LABEL_DOT_GAP + PRN_LABEL_HEIGHT * 0.5;
        assert!(
            prn5.offset <= nearest + 1e-3,
            "PRN 5 should take the nearest free slot (got {})",
            prn5.offset
        );
        assert!(
            (prn9.cx - prn5.cx).abs() >= PRN_LABEL_WIDTH + LABEL_MIN_GAP
                || (prn9.cy - prn5.cy).abs() >= PRN_LABEL_HEIGHT + LABEL_MIN_GAP,
            "PRN 9 must not share PRN 5's slot"
        );
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
    fn labels_clear_their_own_marker_at_every_azimuth() {
        // The axis-aligned label box used to swallow its own dot/ring
        // ("G1●2"). Whatever side the label lands on, the gap between the
        // marker's outer edge and the label's near edge must be at least
        // LABEL_DOT_GAP on both axes.
        for azimuth in [0.0_f64, 45.0, 90.0, 135.0, 225.0, 290.0, 320.0] {
            let (fx, fy) = sky_plot_position(azimuth, 45.0).unwrap();
            let (dot_x, dot_y) = (fx * SKY_CANVAS_SIZE, fy * SKY_CANVAS_SIZE);
            let placed = place_prn_labels(&[(1, dot_x, dot_y)], &[], SKY_CANVAS_SIZE);
            let label = &placed[0];
            let gap_x = (label.cx - dot_x).abs() - PRN_LABEL_WIDTH * 0.5;
            let gap_y = (label.cy - dot_y).abs() - PRN_LABEL_HEIGHT * 0.5;
            assert!(
                gap_x.max(gap_y) >= MARKER_EXTENT + LABEL_DOT_GAP - 1e-3,
                "azimuth {azimuth}: label near edge too close to its marker"
            );
        }
    }

    #[test]
    fn cardinal_labels_stay_inside_the_circle_box() {
        // N previously poked 11px above the circle container and visually
        // overlapped the pane header, while S clipped at the pane bottom;
        // every cardinal must fit inside the 290px box now.
        for (dir_x, dir_y) in [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
            let (left, top) = cardinal_label_origin(dir_x, dir_y);
            assert!(left >= 0.0 && top >= 0.0, "cardinal ({dir_x}, {dir_y})");
            assert!(
                left + 14.0 <= SKY_CANVAS_SIZE && top + 14.0 <= SKY_CANVAS_SIZE,
                "cardinal ({dir_x}, {dir_y})"
            );
        }
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
        // A due-south label must not land in the ring-label band; the hugging
        // placement resolves that by stepping off the radial.
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
    fn horizon_labels_never_cover_the_cardinals() {
        // A low-elevation satellite due south used to place its outward
        // label right on top of the S cardinal. With the cardinals as
        // obstacles the label displaces radially outward until it clears.
        let (dot_x, dot_y) = sky_plot_position(180.0, 10.0).unwrap();
        let dot_x = dot_x * SKY_CANVAS_SIZE;
        let dot_y = dot_y * SKY_CANVAS_SIZE;
        let obstacles = sky_label_obstacles(SKY_CANVAS_SIZE);
        let placed = place_prn_labels(&[(32_u8, dot_x, dot_y)], &obstacles, SKY_CANVAS_SIZE);
        assert_eq!(placed.len(), 1);
        let label_box = LabelBox {
            cx: placed[0].cx,
            cy: placed[0].cy,
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
        // Every label keeps a distinct, readable box: no two overlap, and
        // each stays inside the canvas (never clipped at the pane edge).
        let half_w = PRN_LABEL_WIDTH * 0.5;
        let half_h = PRN_LABEL_HEIGHT * 0.5;
        for (index, label) in placed.iter().enumerate() {
            let box_ = LabelBox {
                cx: label.cx,
                cy: label.cy,
                width: PRN_LABEL_WIDTH,
                height: PRN_LABEL_HEIGHT,
            };
            assert!(
                label.cx - half_w >= 0.0
                    && label.cx + half_w <= SKY_CANVAS_SIZE
                    && label.cy - half_h >= 0.0
                    && label.cy + half_h <= SKY_CANVAS_SIZE,
                "label {} leaves the canvas",
                label.prn
            );
            for other in &placed[index + 1..] {
                let other_box = LabelBox {
                    cx: other.cx,
                    cy: other.cy,
                    width: PRN_LABEL_WIDTH,
                    height: PRN_LABEL_HEIGHT,
                };
                assert!(
                    !box_.collides_with(other_box),
                    "labels {} and {} collide",
                    label.prn,
                    other.prn
                );
            }
        }
    }
}
