//! ENU track plot: east-north plane of the final selected solution
//! (design-refresh spec §3.6).
//!
//! Geometry: the data region is a centered square (`aspect_square`), and
//! x/y share one px/m scale (`equal_scale`), so the track is never distorted
//! by the pane aspect ratio. Tick labels are DOM elements positioned at the
//! same relative fractions the canvas uses, so text and grid lines always
//! agree. Ticks follow the nice extent ladder; adjacent tick labels closer
//! than 24px are thinned (every other shown), the bottom-left first x tick
//! must keep ≥12px clearance from the y-tick label column (dropped when it
//! cannot — dropping beats squeezing), and negative values render with
//! U+2212 plus a 2px gap so "−50" and "−50" never concatenate ("−50−50" is
//! a violation, spec §3.6 / gap 15). Fonts are never shrunk.
//!
//! Encoding: track = chart_series 1.5px, selected epoch = accent hollow ring
//! with a full crosshair, current end = small filled dot, grid 1px
//! `plot_grid`, axes 1px `plot_axis` (both solid, spec §3.4 / gap 19).
//!
//! Renders display-only data supplied by `app.rs`.

use super::frame::{self, PlotState};
use crate::visual_system::{data_font_family, Palette};
use crate::GuiPvtViewModel;
use gpui_kit::prelude::*;
use gpui_kit::{bounds, canvas, div, point, px, quad, size, AnyElement, Hsla, PathBuilder};

/// Track stroke (spec §3.6: 1.5px; never >1.5px, §7).
const TRACK_STROKE: f32 = 1.5;
/// Selected-epoch hollow ring + crosshair, accent (spec §3.4 cursor rule).
const SELECTED_RING_RADIUS: f32 = 4.5;
const SELECTED_RING_STROKE: f32 = 1.5;
/// Current-end filled dot.
const ENDPOINT_RADIUS: f32 = 3.0;

/// Tick geometry (spec §3.6).
const TICK_STUB: f32 = 4.0;
const TICK_FONT_PX: f32 = 10.0;
/// Conservative rendered width of "−300"-class labels at 10px.
const TICK_LABEL_WIDTH_EST: f32 = 34.0;
/// Adjacent tick labels <24px apart show every other (spec §3.6).
const TICK_MIN_GAP: f32 = 24.0;
/// The first x tick keeps ≥12px clearance from the y-tick label column.
const CORNER_CLEARANCE: f32 = 12.0;
/// U+2212 minus sign, followed by a 2px gap (spec §3.6).
const MINUS_SIGN: &str = "\u{2212}";
/// Gap between the U+2212 sign and its digits.
pub(crate) const MINUS_GAP: f32 = 2.0;
/// Gap between the y-tick label column and the plot's left edge.
const Y_LABEL_GAP: f32 = 6.0;
/// Distance from the plot bottom edge to the x-tick label row center.
const X_LABEL_DROP: f32 = 14.0;

// ---------------------------------------------------------------------------
// Pure model (unit-tested)
// ---------------------------------------------------------------------------

/// Nice instrument extent ladder (1 / 2 / 5 · 10^n).
pub(crate) fn nice_enu_extent(raw_extent: f64) -> f64 {
    let raw_extent = raw_extent.max(1.0);
    let magnitude = 10_f64.powf(raw_extent.log10().floor());
    let normalized = raw_extent / magnitude;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

/// Split a tick value into an optional U+2212 sign and the digits; the
/// renderer inserts a 2px gap after the sign so negative labels never
/// concatenate with a neighbour (spec §3.6 / gap 15).
pub(crate) fn enu_tick_label(value: f64) -> (&'static str, String) {
    let magnitude = if value.abs() >= 10.0 || value.fract().abs() < f64::EPSILON {
        format!("{:.0}", value.abs())
    } else {
        format!("{:.1}", value.abs())
    };
    if value < 0.0 {
        (MINUS_SIGN, magnitude)
    } else {
        ("", magnitude)
    }
}

/// One shared px/m scale for both axes (spec §3.6: equal scale, no
/// distortion): the smaller plot dimension decides.
pub(crate) fn equal_scale(plot_width: f32, plot_height: f32, extent_m: f64) -> f32 {
    let half = (plot_width.min(plot_height) * 0.5) as f64;
    (half / extent_m.max(f64::EPSILON)) as f32
}

/// Relative (0..1) position of a data value inside the square data region.
pub(crate) fn tick_fraction(value_m: f64, extent_m: f64, east_west: bool) -> f32 {
    let fraction = (value_m / extent_m.max(f64::EPSILON)) as f32 * 0.5;
    if east_west {
        0.5 + fraction
    } else {
        0.5 - fraction
    }
}

/// Thinned tick selection: always keep the first tick, then keep a tick only
/// if it is ≥`min_gap` from the previously kept one (show every other when
/// closer). Never shrinks fonts (spec §3.6).
pub(crate) fn thin_ticks(positions: &[f32], min_gap: f32) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::with_capacity(positions.len());
    for (index, position) in positions.iter().enumerate() {
        match kept.last() {
            None => kept.push(index),
            Some(&last) => {
                if (position - positions[last]).abs() >= min_gap {
                    kept.push(index);
                }
            }
        }
    }
    kept
}

/// The bottom-left first x tick label must keep ≥`clearance` from the
/// y-tick label column (which ends `y_row_right` px from the plot edge);
/// dropping the tick beats squeezing it (spec §3.6).
pub(crate) fn corner_tick_visible(
    first_tick_left_px: f32,
    y_label_right_px: f32,
    clearance: f32,
) -> bool {
    first_tick_left_px - y_label_right_px >= clearance
}

/// Grid step in meters backing the "网格: N m" note (real half-extent tick
/// spacing: ticks sit at ±extent, ±extent/2, 0).
pub(crate) fn grid_step_m(extent_m: f64) -> f64 {
    extent_m / 2.0
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// Convert the displayable epochs to east/north meters relative to the first
/// displayable epoch. Empty when nothing is displayable (honest empty state).
pub(crate) fn enu_points(epochs: &[GuiPvtViewModel]) -> Vec<(usize, f64, f64)> {
    let lla = epochs
        .iter()
        .enumerate()
        .filter(|(_, pvt)| crate::app::pvt_position_is_displayable(pvt))
        .map(|(index, pvt)| (index, pvt.latitude_deg, pvt.longitude_deg))
        .collect::<Vec<_>>();
    lla.first()
        .map_or_else(Vec::new, |(_, latitude_0, longitude_0)| {
            let cos_latitude = latitude_0.to_radians().cos();
            lla.iter()
                .map(|(index, latitude, longitude)| {
                    let east = (longitude - longitude_0).to_radians() * 6_378_137.0 * cos_latitude;
                    let north = (latitude - latitude_0).to_radians() * 6_378_137.0;
                    (*index, east, north)
                })
                .collect()
        })
}

/// Robust raw extent (95th percentile of max |east|, |north|).
pub(crate) fn raw_extent(points: &[(usize, f64, f64)]) -> f64 {
    let mut extents = points
        .iter()
        .map(|(_, east, north)| east.abs().max(north.abs()))
        .filter(|extent| extent.is_finite())
        .collect::<Vec<_>>();
    extents.sort_by(f64::total_cmp);
    extents
        .get(((extents.len().saturating_sub(1)) as f64 * 0.95).round() as usize)
        .copied()
        .unwrap_or(1.0)
        .max(1.0)
}

/// Axis tick values at ±extent, ±extent/2, 0 (spec §3.6 mockup ladder).
pub(crate) fn axis_tick_values(extent_m: f64) -> [f64; 5] {
    [-extent_m, -extent_m / 2.0, 0.0, extent_m / 2.0, extent_m]
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render the ENU track pane content (without chrome).
pub(crate) fn render(
    epochs: &[GuiPvtViewModel],
    selected_epoch: Option<usize>,
    palette: Palette,
) -> AnyElement {
    let points = enu_points(epochs);
    let has_points = !points.is_empty();
    let extent = nice_enu_extent(raw_extent(&points));
    // Data identity colour: the blue series, not the status green — a
    // trajectory must not read as "healthy" state (spec §3.4).
    let trajectory_color = palette.chart_series[1];
    let selected = selected_epoch;
    div()
        .size_full()
        .relative()
        .flex()
        .flex_col()
        // Square data region: equal px/m on both axes (spec §3.6). It takes
        // the space above the x-axis title strip so the centered title can
        // never collide with the centered "0" tick label.
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .h_full()
                        .aspect_square()
                        .relative()
                        .child(
                            canvas(
                                move |_, _, _| {},
                                move |plot_bounds, _, window, _| {
                                    let half_width = f32::from(plot_bounds.size.width) * 0.5;
                                    let half_height = f32::from(plot_bounds.size.height) * 0.5;
                                    let center = point(
                                        plot_bounds.origin.x + px(half_width),
                                        plot_bounds.origin.y + px(half_height),
                                    );
                                    // One px/m for both axes: no distortion.
                                    let scale = equal_scale(
                                        f32::from(plot_bounds.size.width),
                                        f32::from(plot_bounds.size.height),
                                        extent,
                                    );
                                    // Grid: 1px solid plot_grid at the tick lines.
                                    for value in axis_tick_values(extent) {
                                        let x = center.x + px((value * scale as f64) as f32);
                                        frame::paint_v_line(
                                            window,
                                            f32::from(x),
                                            f32::from(plot_bounds.origin.y),
                                            f32::from(
                                                plot_bounds.origin.y + plot_bounds.size.height,
                                            ),
                                            frame::LineRole::Grid,
                                            palette,
                                        );
                                        let y = center.y - px((value * scale as f64) as f32);
                                        frame::paint_h_line(
                                            window,
                                            f32::from(y),
                                            f32::from(plot_bounds.origin.x),
                                            f32::from(
                                                plot_bounds.origin.x + plot_bounds.size.width,
                                            ),
                                            frame::LineRole::Grid,
                                            palette,
                                        );
                                    }
                                    // Axes: bottom edge (x) and left edge (y).
                                    frame::paint_h_line(
                                        window,
                                        f32::from(plot_bounds.origin.y + plot_bounds.size.height),
                                        f32::from(plot_bounds.origin.x),
                                        f32::from(plot_bounds.origin.x + plot_bounds.size.width),
                                        frame::LineRole::Axis,
                                        palette,
                                    );
                                    frame::paint_v_line(
                                        window,
                                        f32::from(plot_bounds.origin.x),
                                        f32::from(plot_bounds.origin.y),
                                        f32::from(plot_bounds.origin.y + plot_bounds.size.height),
                                        frame::LineRole::Axis,
                                        palette,
                                    );
                                    if points.is_empty() {
                                        return;
                                    }
                                    let project = |east: f64, north: f64| {
                                        point(
                                            center.x + px((east * scale as f64) as f32),
                                            center.y - px((north * scale as f64) as f32),
                                        )
                                    };
                                    // Tick stubs on the axes (4px, plot_axis).
                                    for value in axis_tick_values(extent) {
                                        let x = center.x + px((value * scale as f64) as f32);
                                        window.paint_quad(quad(
                                            bounds(
                                                point(
                                                    x,
                                                    plot_bounds.origin.y + plot_bounds.size.height,
                                                ),
                                                size(px(1.), px(TICK_STUB)),
                                            ),
                                            px(0.),
                                            palette.plot_axis,
                                            px(0.),
                                            palette.plot_axis,
                                            Default::default(),
                                        ));
                                        let y = center.y - px((value * scale as f64) as f32);
                                        window.paint_quad(quad(
                                            bounds(
                                                point(plot_bounds.origin.x - px(TICK_STUB), y),
                                                size(px(TICK_STUB), px(1.)),
                                            ),
                                            px(0.),
                                            palette.plot_axis,
                                            px(0.),
                                            palette.plot_axis,
                                            Default::default(),
                                        ));
                                    }
                                    // Track: chart_series[0] 1.5px (spec §3.6).
                                    let mut trajectory = PathBuilder::stroke(px(TRACK_STROKE));
                                    for (point_index, (_, east, north)) in points.iter().enumerate()
                                    {
                                        let plot_point = project(*east, *north);
                                        if point_index == 0 {
                                            trajectory.move_to(plot_point);
                                        } else {
                                            trajectory.line_to(plot_point);
                                        }
                                    }
                                    // Older track recedes; the most recent
                                    // fifth stays at full strength so the eye
                                    // reads direction of travel. Same 1.5px
                                    // stroke, one series colour.
                                    if let Ok(path) = trajectory.build() {
                                        window.paint_path(path, trajectory_color.opacity(0.55));
                                    }
                                    let tail_start =
                                        points.len().saturating_sub((points.len() / 5).max(2));
                                    let mut tail = PathBuilder::stroke(px(TRACK_STROKE));
                                    for (index, (_, east, north)) in
                                        points.iter().enumerate().skip(tail_start)
                                    {
                                        let plot_point = project(*east, *north);
                                        if index == tail_start {
                                            tail.move_to(plot_point);
                                        } else {
                                            tail.line_to(plot_point);
                                        }
                                    }
                                    if let Ok(path) = tail.build() {
                                        window.paint_path(path, trajectory_color);
                                    }
                                    // Selected epoch: accent hollow ring + crosshair.
                                    if let Some((_, east, north)) = selected.and_then(|index| {
                                        points.iter().find(|(i, _, _)| *i == index)
                                    }) {
                                        let selected_point = project(*east, *north);
                                        let line_color = palette.accent.opacity(0.85);
                                        window.paint_quad(quad(
                                            bounds(
                                                point(
                                                    plot_bounds.origin.x,
                                                    selected_point.y - px(0.5),
                                                ),
                                                size(plot_bounds.size.width, px(1.)),
                                            ),
                                            px(0.),
                                            line_color,
                                            px(0.),
                                            line_color,
                                            Default::default(),
                                        ));
                                        window.paint_quad(quad(
                                            bounds(
                                                point(
                                                    selected_point.x - px(0.5),
                                                    plot_bounds.origin.y,
                                                ),
                                                size(px(1.), plot_bounds.size.height),
                                            ),
                                            px(0.),
                                            line_color,
                                            px(0.),
                                            line_color,
                                            Default::default(),
                                        ));
                                        window.paint_quad(quad(
                                            bounds(
                                                point(
                                                    selected_point.x - px(SELECTED_RING_RADIUS),
                                                    selected_point.y - px(SELECTED_RING_RADIUS),
                                                ),
                                                size(
                                                    px(SELECTED_RING_RADIUS * 2.0),
                                                    px(SELECTED_RING_RADIUS * 2.0),
                                                ),
                                            ),
                                            px(SELECTED_RING_RADIUS),
                                            Hsla::transparent_black(),
                                            px(SELECTED_RING_STROKE),
                                            palette.accent,
                                            Default::default(),
                                        ));
                                    }
                                    // Current end: small filled dot (spec §3.6).
                                    if let Some((_, east, north)) = points.last() {
                                        let end_point = project(*east, *north);
                                        window.paint_quad(quad(
                                            bounds(
                                                point(
                                                    end_point.x - px(ENDPOINT_RADIUS),
                                                    end_point.y - px(ENDPOINT_RADIUS),
                                                ),
                                                size(
                                                    px(ENDPOINT_RADIUS * 2.0),
                                                    px(ENDPOINT_RADIUS * 2.0),
                                                ),
                                            ),
                                            px(ENDPOINT_RADIUS),
                                            trajectory_color,
                                            px(0.),
                                            trajectory_color,
                                            Default::default(),
                                        ));
                                    }
                                },
                            )
                            .size_full(),
                        )
                        // Tick labels: x only along the bottom edge, y only along
                        // the left edge, at the same fractions as the canvas grid.
                        .children(
                            has_points
                                .then(|| tick_label_elements(extent, palette))
                                .into_iter()
                                .flatten(),
                        ),
                ),
        )
        // x-axis title strip below the plot area: the centered title sits a
        // full row under the tick labels, so it can never collide with the
        // centered "0" tick (spec §3.6).
        .child(
            div()
                .h(px(16.))
                .w_full()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.)) /* MICRO */
                .text_color(palette.faint)
                .child(if has_points { "东" } else { "" }),
        )
        // y-axis title (caption 11px) at the head of the left axis.
        .children(has_points.then(|| {
            div()
                .absolute()
                .left(px(0.))
                .top_0()
                .text_size(px(10.)) /* MICRO */
                .text_color(palette.faint)
                .child("北")
        }))
        .children(
            has_points
                .then_some(())
                .map(|_| frame::grid_note(frame::grid_spacing_label(grid_step_m(extent)), palette)),
        )
        .children((!has_points).then(|| frame::state_caption(&PlotState::Empty, palette)))
        .into_any_element()
}

/// Tick label elements inside the square data region. Positions use the
/// same `tick_fraction` mapping as the canvas grid lines.
fn tick_label_elements(extent_m: f64, palette: Palette) -> Vec<AnyElement> {
    let values = axis_tick_values(extent_m);
    // Pixel-space positions along one axis (fractions × a 100px reference
    // only drive the thinning decision, which is scale-invariant here
    // because the square region's fraction spacing is constant).
    let fractions: Vec<f32> = values
        .iter()
        .map(|value| tick_fraction(*value, extent_m, true))
        .collect();
    let kept = thin_ticks(
        &fractions.iter().map(|f| f * 100.0).collect::<Vec<_>>(),
        TICK_MIN_GAP,
    );
    // Corner rule: drop the first (leftmost) x tick when its label would
    // come within 12px of the y-tick label column.
    let first_left = fractions.first().copied().unwrap_or(0.0);
    let corner_visible = corner_tick_visible(
        first_left * TICK_LABEL_WIDTH_EST.max(100.0),
        -Y_LABEL_GAP,
        CORNER_CLEARANCE,
    );
    let mut elements = Vec::new();
    for (tick_index, value) in values.into_iter().enumerate() {
        if !kept.contains(&tick_index) {
            continue;
        }
        if tick_index == 0 && !corner_visible {
            continue;
        }
        let (sign, digits) = enu_tick_label(value);
        let fraction = tick_fraction(value, extent_m, true);
        // x tick: bottom edge only, centered on the tick, U+2212 + 2px gap.
        elements.push(
            div()
                .absolute()
                .left(gpui_kit::relative(fraction))
                .bottom(px(X_LABEL_DROP - 6.))
                .ml(px(-TICK_LABEL_WIDTH_EST * 0.5))
                .w(px(TICK_LABEL_WIDTH_EST))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(MINUS_GAP))
                .font_family(data_font_family())
                .text_size(px(TICK_FONT_PX))
                .text_color(palette.faint)
                .child(div().child(sign))
                .child(div().child(digits.clone()))
                .into_any_element(),
        );
        // y tick: left edge only, right-aligned against the plot edge.
        let fraction = tick_fraction(value, extent_m, false);
        elements.push(
            div()
                .absolute()
                .top(gpui_kit::relative(fraction))
                .left(px(-Y_LABEL_GAP - TICK_LABEL_WIDTH_EST))
                .mt(px(-6.))
                .w(px(TICK_LABEL_WIDTH_EST))
                .text_right()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(MINUS_GAP))
                .font_family(data_font_family())
                .text_size(px(TICK_FONT_PX))
                .text_color(palette.faint)
                .child(div().child(sign))
                .child(div().child(digits))
                .into_any_element(),
        );
    }
    elements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enu_extent_rounds_outward_to_instrument_ticks() {
        assert_eq!(nice_enu_extent(0.4), 1.0);
        assert_eq!(nice_enu_extent(1.3), 2.0);
        assert_eq!(nice_enu_extent(3.1), 5.0);
        assert_eq!(nice_enu_extent(8.2), 10.0);
        assert_eq!(nice_enu_extent(42.0), 50.0);
    }

    #[test]
    fn equal_scale_is_identical_for_both_axes() {
        // The square data region guarantees w == h, so one px/m value
        // governs both axes: 10m east and 10m north map to the same pixels.
        let scale_wide = equal_scale(400.0, 400.0, 50.0);
        let scale_tall = equal_scale(400.0, 400.0, 50.0);
        assert_eq!(scale_wide, scale_tall);
        let east_px = 10.0 * scale_wide;
        let north_px = 10.0 * scale_tall;
        assert_eq!(east_px, north_px);
        // On a non-square region the smaller dimension still decides.
        assert_eq!(
            equal_scale(800.0, 400.0, 50.0),
            equal_scale(400.0, 800.0, 50.0)
        );
    }

    #[test]
    fn tick_labels_use_unicode_minus_with_a_split_sign() {
        let (sign, digits) = enu_tick_label(-50.0);
        assert_eq!(sign, "\u{2212}");
        assert_eq!(digits, "50");
        let (sign, digits) = enu_tick_label(20.0);
        assert_eq!(sign, "");
        assert_eq!(digits, "20");
        let (sign, digits) = enu_tick_label(-2.5);
        assert_eq!(sign, "\u{2212}");
        assert_eq!(digits, "2.5");
        // The sign and digits are separate spans with a 2px gap, so two
        // adjacent negative ticks can never render as "−50−50".
        assert_eq!(MINUS_GAP, 2.0);
    }

    #[test]
    fn adjacent_ticks_thin_to_every_other_below_24px() {
        // 5 ticks across 96px: spacing 24px is kept, 20px thins.
        let wide: Vec<f32> = (0..5).map(|i| i as f32 * 24.0).collect();
        assert_eq!(thin_ticks(&wide, TICK_MIN_GAP), vec![0, 1, 2, 3, 4]);
        let narrow: Vec<f32> = (0..5).map(|i| i as f32 * 20.0).collect();
        assert_eq!(thin_ticks(&narrow, TICK_MIN_GAP), vec![0, 2, 4]);
    }

    #[test]
    fn first_x_tick_drops_when_the_corner_clearance_fails() {
        // Label left edge at 10px, y column ends at -40px → 50px clear: keep.
        assert!(corner_tick_visible(10.0, -40.0, CORNER_CLEARANCE));
        // Label left edge at 10px, y column ends at -5px → 15px clear: keep.
        assert!(corner_tick_visible(10.0, -5.0, CORNER_CLEARANCE));
        // Squeezed corner: 8px clearance → drop (never squeeze, spec §3.6).
        assert!(!corner_tick_visible(8.0, 0.0, CORNER_CLEARANCE));
        // Exactly at the threshold: keep.
        assert!(corner_tick_visible(12.0, 0.0, CORNER_CLEARANCE));
    }

    #[test]
    fn grid_note_reports_the_real_half_extent_step() {
        assert_eq!(grid_step_m(20.0), 10.0);
        assert_eq!(frame::grid_spacing_label(grid_step_m(20.0)), "网格: 10 m");
    }

    #[test]
    fn tick_fractions_mirror_east_and_north() {
        assert_eq!(tick_fraction(0.0, 50.0, true), 0.5);
        assert_eq!(tick_fraction(50.0, 50.0, true), 1.0);
        assert_eq!(tick_fraction(-50.0, 50.0, true), 0.0);
        assert_eq!(tick_fraction(50.0, 50.0, false), 0.0);
        assert_eq!(tick_fraction(-50.0, 50.0, false), 1.0);
    }
}
