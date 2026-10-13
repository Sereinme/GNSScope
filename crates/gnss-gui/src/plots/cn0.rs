//! C/N0 history plot: nine fixed lanes of per-channel carrier-to-noise
//! history (design-refresh spec §3.7).
//!
//! Layout (gap 17): exactly nine lanes at a fixed 28px pitch
//! (`metrics::CN0_LANE_PITCH`) so the grid never reflows when values change.
//! Curves are 1px; the right-hand value column is 48px wide with 11px
//! right-aligned data text and carries the `(dB-Hz)` unit. Lane order is the
//! current C/N0 descending with a stable PRN tiebreak; the pinned PRN keeps
//! the top slot. The x axis spans the real history window with −N…0 ticks
//! (10px) and the axis title `历元（过去 N s）`. Every lane uses one
//! `chart_series` colour (10-cycle; nine lanes are therefore pairwise
//! distinct) and the PRN label shares the curve colour — on light surfaces
//! where that contrast falls below 4.5:1 the label switches to a darker
//! same-hue variant while the curve keeps its palette colour.
//!
//! Data honesty (spec §7): lanes exist for real tracked channels only; lanes
//! without data draw no curve and no value, and a completely empty history
//! renders the shared `暂无数据` caption. Values are never fabricated.

use super::frame::{self, PlotState};
use crate::app::GnsscopeApp;
use crate::visual_system::{data_font_family, Palette};
use crate::GuiChannelStatus;
use gpui_kit::prelude::*;
use gpui_kit::{
    bounds, canvas, div, point, px, quad, size, AnyElement, Context, Hsla, PathBuilder,
    SharedString,
};
use std::collections::BTreeSet;

/// Exactly nine lanes (spec §3.7); the grid is fixed, data may be sparser.
pub(crate) const LANE_COUNT: usize = 9;
/// Lane pitch in pixels (spec §3.7 / gap 17; shared metrics constant).
pub(crate) const LANE_PITCH: f32 = 28.0;
/// Height of the x-axis strip (tick row + axis title).
const AXIS_ROW_HEIGHT: f32 = 26.0;
/// Left PRN-label column width.
const PRN_COLUMN_WIDTH: f32 = 34.0;
/// Right value-label column width (spec §3.7: 48px).
pub(crate) const VALUE_COLUMN_WIDTH: f32 = 48.0;
/// Unit carried by the value column (spec §3.7: merged into the axis title
/// at caption 11px — a separate top strip does not fit the pane's fixed
/// lane grid without pushing the last lane into the axis row).
pub(crate) const VALUE_UNIT: &str = "(dB-Hz)";
/// Curve stroke width (spec §3.7: 1px).
const CURVE_STROKE: f32 = 1.0;
/// Vertical breathing room inside a lane band around the curve.
const LANE_INSET: f32 = 4.0;
/// Tick label font (10px tick role).
const TICK_FONT_PX: f32 = 10.0;
/// Value label font (data 11px).
const VALUE_FONT_PX: f32 = 11.0;
/// Axis title font (caption 11px; also carries the value unit).
const TITLE_FONT_PX: f32 = 11.0;
/// x-axis tick fractions across the history window (−300, −240, …, 0 for a
/// 300 s window).
const TICK_FRACTIONS: [f64; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
/// Minimum auto-scale span per lane, matching the previous behaviour.
const MIN_LANE_SPAN_DB: f64 = 6.0;
/// WCAG AA threshold used for the light-surface label fallback (spec §3.7).
const LABEL_CONTRAST_MIN: f32 = 4.5;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cn0Series {
    pub(crate) prn: u8,
    pub(crate) samples: Vec<Option<f64>>,
}

#[derive(Clone, Copy)]
pub(crate) struct Cn0PlotSelection {
    pub(crate) epoch_period_ms: u64,
    pub(crate) selected_epoch: Option<usize>,
    pub(crate) hovered_prn: Option<u8>,
    pub(crate) pinned_prn: Option<u8>,
}

impl Cn0Series {
    pub(crate) fn value_at(&self, selected_epoch: Option<usize>) -> Option<f64> {
        selected_epoch
            .and_then(|index| self.samples.get(index).copied().flatten())
            .or_else(|| self.samples.iter().rev().flatten().copied().next())
    }
}

/// Build the lane series: only real tracked channels with finite C/N0,
/// ordered by current C/N0 descending with a stable PRN tiebreak; the pinned
/// PRN keeps the top slot. At most `limit` (= nine) lanes are kept.
pub(crate) fn build_cn0_series(
    epoch_channels: &[Vec<GuiChannelStatus>],
    selected_epoch: Option<usize>,
    pinned_prn: Option<u8>,
    limit: usize,
) -> Vec<Cn0Series> {
    let prns = epoch_channels
        .iter()
        .flat_map(|channels| channels.iter())
        .filter(|channel| channel.tracking_locked && channel.cn0_db_hz.is_finite())
        .map(|channel| channel.prn)
        .collect::<BTreeSet<_>>();
    let mut series = prns
        .into_iter()
        .map(|prn| Cn0Series {
            prn,
            samples: epoch_channels
                .iter()
                .map(|channels| {
                    channels
                        .iter()
                        .find(|channel| channel.prn == prn && channel.tracking_locked)
                        .map(|channel| channel.cn0_db_hz)
                        .filter(|value| value.is_finite())
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    series.sort_by(|left, right| {
        let left_pinned = Some(left.prn) == pinned_prn;
        let right_pinned = Some(right.prn) == pinned_prn;
        right_pinned
            .cmp(&left_pinned)
            .then_with(|| {
                right
                    .value_at(selected_epoch)
                    .unwrap_or(f64::NEG_INFINITY)
                    .total_cmp(&left.value_at(selected_epoch).unwrap_or(f64::NEG_INFINITY))
            })
            // Deterministic tiebreak: ascending PRN for equal C/N0.
            .then_with(|| left.prn.cmp(&right.prn))
    });
    series.truncate(limit);
    series
}

/// Honest value text: a real one-decimal C/N0 or an em dash. Missing values
/// are never replaced with `0` (spec §7).
pub(crate) fn cn0_value_text(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_string(), |value| format!("{value:.1}"))
}

// ---------------------------------------------------------------------------
// Colour contrast (pure, unit-tested)
// ---------------------------------------------------------------------------

/// WCAG relative luminance of an sRGB colour.
fn relative_luminance(color: Hsla) -> f32 {
    fn channel(value: f32) -> f32 {
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    let chroma = (1.0 - (2.0 * color.l - 1.0).abs()) * color.s;
    let sector = color.h * 6.0;
    let secondary = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let match_light = color.l - chroma * 0.5;
    0.2126 * channel(r + match_light)
        + 0.7152 * channel(g + match_light)
        + 0.0722 * channel(b + match_light)
}

/// WCAG contrast ratio between two colours (1.0 … 21.0).
fn contrast_ratio(left: Hsla, right: Hsla) -> f32 {
    let left = relative_luminance(left);
    let right = relative_luminance(right);
    let (high, low) = if left > right {
        (left, right)
    } else {
        (right, left)
    };
    (high + 0.05) / (low + 0.05)
}

/// Label colour for a series colour on the given surface. On light surfaces
/// where the palette colour falls below 4.5:1 the label takes a darker
/// same-hue variant; the curve itself keeps the original palette colour
/// (spec §3.7 / gap 17). Dark surfaces pass the palette colour through.
pub(crate) fn cn0_label_color(color: Hsla, surface: Hsla) -> Hsla {
    if relative_luminance(surface) <= 0.5 || contrast_ratio(color, surface) >= LABEL_CONTRAST_MIN {
        return color;
    }
    let mut lightness = color.l;
    let mut candidate = color;
    while lightness > 0.12 && contrast_ratio(candidate, surface) < LABEL_CONTRAST_MIN {
        lightness -= 0.05;
        candidate = Hsla {
            l: lightness,
            ..color
        };
    }
    candidate
}

// ---------------------------------------------------------------------------
// Axis helpers (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Real history duration in whole seconds behind the x axis.
pub(crate) fn history_seconds(sample_count: usize, epoch_period_ms: u64) -> u64 {
    sample_count.saturating_sub(1) as u64 * epoch_period_ms / 1_000
}

/// Axis title carrying the real window length and the value unit (spec
/// §3.7: the `(dB-Hz)` unit rides with the axis title at caption 11px;
/// never a fixed story value on live data).
pub(crate) fn axis_title(history_s: u64) -> String {
    format!("历元（过去 {history_s} s） {VALUE_UNIT}")
}

/// Tick label for a real time offset in seconds; U+2212 minus, no ASCII hyphen.
pub(crate) fn tick_label(offset_s: u64) -> String {
    if offset_s == 0 {
        "0".to_string()
    } else {
        format!("\u{2212}{offset_s}")
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Pane-head scale control. Only genuinely available options are offered;
/// auto-scaling is the sole implemented mode, so the control states it
/// without pretending a fixed scale exists (spec §3.7).
pub(crate) fn scale_control(palette: Palette) -> AnyElement {
    frame::quiet_control("标度: 自动", palette)
}

/// Render the C/N0 history pane content (without chrome).
pub(crate) fn render(
    epoch_channels: &[Vec<GuiChannelStatus>],
    current_channels: &[GuiChannelStatus],
    selection: Cn0PlotSelection,
    palette: Palette,
    cx: &mut Context<GnsscopeApp>,
) -> AnyElement {
    let fallback_history;
    let epoch_channels = if epoch_channels.is_empty() && !current_channels.is_empty() {
        fallback_history = vec![current_channels.to_vec()];
        fallback_history.as_slice()
    } else {
        epoch_channels
    };
    let series = build_cn0_series(
        epoch_channels,
        selection.selected_epoch,
        selection.pinned_prn,
        LANE_COUNT,
    );
    let sample_count = epoch_channels.len().max(1);
    let history_s = history_seconds(sample_count, selection.epoch_period_ms);
    let colors = palette.chart_series;
    // Per-lane colour index keyed by lane position (10-cycle; nine lanes are
    // pairwise distinct).
    let lane_colors: Vec<Hsla> = (0..LANE_COUNT)
        .map(|lane| colors[lane % colors.len()])
        .collect();
    let canvas_series = series.clone();
    div()
        .size_full()
        .relative()
        // Lane area: nine fixed 28px bands above the axis strip.
        .child(
            div()
                .absolute()
                .left(px(0.))
                .right(px(0.))
                .top(px(0.))
                .bottom(px(AXIS_ROW_HEIGHT))
                .child(
                    canvas(move |_, _, _| {}, {
                        // The canvas owns a copy; DOM lane rows below
                        // still need `lane_colors` for label colours.
                        let lane_colors = lane_colors.clone();
                        move |plot_bounds, _, window, _| {
                            let left = plot_bounds.origin.x + px(PRN_COLUMN_WIDTH + LANE_INSET);
                            let right = plot_bounds.origin.x + plot_bounds.size.width
                                - px(VALUE_COLUMN_WIDTH + LANE_INSET);
                            let width = (right - left).max(px(1.));
                            let lane_area_height = px(LANE_PITCH * LANE_COUNT as f32);
                            // Fixed lane grid: nine separators, 1px solid.
                            for lane in 0..=LANE_COUNT {
                                let y = plot_bounds.origin.y + px(LANE_PITCH * lane as f32);
                                frame::paint_h_line(
                                    window,
                                    f32::from(y),
                                    f32::from(plot_bounds.origin.x),
                                    f32::from(plot_bounds.origin.x + plot_bounds.size.width),
                                    frame::LineRole::Grid,
                                    palette,
                                );
                            }
                            // Vertical time grid at the real tick lines.
                            for fraction in TICK_FRACTIONS {
                                let x = left + width * fraction as f32;
                                frame::paint_v_line(
                                    window,
                                    f32::from(x),
                                    f32::from(plot_bounds.origin.y),
                                    f32::from(plot_bounds.origin.y + lane_area_height),
                                    frame::LineRole::Grid,
                                    palette,
                                );
                            }
                            // Selected-epoch cursor (accent, 1px).
                            if let Some(index) = selection
                                .selected_epoch
                                .filter(|index| *index < sample_count)
                            {
                                let ratio = if sample_count <= 1 {
                                    1.0
                                } else {
                                    index as f32 / (sample_count - 1) as f32
                                };
                                let x = left + width * ratio;
                                let color = palette.accent.opacity(0.85);
                                window.paint_quad(quad(
                                    bounds(
                                        point(x, plot_bounds.origin.y),
                                        size(px(1.), lane_area_height),
                                    ),
                                    px(0.),
                                    color,
                                    px(0.),
                                    color,
                                    Default::default(),
                                ));
                            }
                            // Curves: one real sequence per lane, 1px.
                            for (lane, item) in canvas_series.iter().enumerate() {
                                let lane_top = plot_bounds.origin.y + px(LANE_PITCH * lane as f32);
                                let inner_height =
                                    (px(LANE_PITCH) - px(LANE_INSET * 2.0)).max(px(1.));
                                let color = lane_colors[lane];
                                let available =
                                    item.samples.iter().flatten().copied().collect::<Vec<_>>();
                                let data_min =
                                    available.iter().copied().fold(f64::INFINITY, f64::min);
                                let data_max =
                                    available.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                                if !data_min.is_finite() || !data_max.is_finite() {
                                    // No real samples in this lane: draw
                                    // nothing rather than a fake curve.
                                    continue;
                                }
                                let center = (data_min + data_max) * 0.5;
                                let span = (data_max - data_min).max(MIN_LANE_SPAN_DB);
                                let min_y = center - span * 0.5;
                                let max_y = center + span * 0.5;
                                let mut builder = PathBuilder::stroke(px(CURVE_STROKE));
                                let mut segment_started = false;
                                let mut selected_point = None;
                                for (index, sample) in item.samples.iter().enumerate() {
                                    let Some(value) = sample else {
                                        segment_started = false;
                                        continue;
                                    };
                                    let x_ratio = if sample_count <= 1 {
                                        1.0
                                    } else {
                                        index as f32 / (sample_count - 1) as f32
                                    };
                                    let y_ratio =
                                        ((value.clamp(min_y, max_y) - min_y) / span) as f32;
                                    let plot_point = point(
                                        left + width * x_ratio,
                                        lane_top + px(LANE_INSET) + inner_height * (1.0 - y_ratio),
                                    );
                                    if segment_started {
                                        builder.line_to(plot_point);
                                    } else {
                                        builder.move_to(plot_point);
                                        segment_started = true;
                                    }
                                    if selection.selected_epoch == Some(index)
                                        || selection.selected_epoch.is_none()
                                    {
                                        selected_point = Some(plot_point);
                                    }
                                }
                                if let Ok(path) = builder.build() {
                                    window.paint_path(path, color);
                                }
                                if let Some(point) = selected_point {
                                    window.paint_quad(quad(
                                        bounds(
                                            gpui_kit::point(point.x - px(2.5), point.y - px(2.5)),
                                            size(px(5.), px(5.)),
                                        ),
                                        px(2.5),
                                        color,
                                        px(1.),
                                        palette.surface,
                                        Default::default(),
                                    ));
                                }
                            }
                        }
                    })
                    .size_full(),
                ),
        )
        // Per-lane DOM rows: PRN label (curve colour) + right value column.
        .children(series.iter().enumerate().map(|(lane, item)| {
            let prn = item.prn;
            let active = selection.hovered_prn.or(selection.pinned_prn) == Some(prn);
            let value = item.value_at(selection.selected_epoch);
            let color = lane_colors[lane];
            let label_color = cn0_label_color(color, palette.surface);
            div()
                .id(SharedString::from(format!("cn0-line-prn-{prn}")))
                .absolute()
                .left(px(0.))
                .right(px(0.))
                .top(px(LANE_PITCH * lane as f32))
                .h(px(LANE_PITCH))
                .flex()
                .items_center()
                .cursor_pointer()
                .when(active, |this| this.bg(palette.accent.opacity(0.06)))
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
                        .w(px(PRN_COLUMN_WIDTH))
                        .pl(px(4.))
                        .text_size(px(VALUE_FONT_PX))
                        .font_family(data_font_family())
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(if active { palette.accent } else { label_color })
                        .child(format!("G{prn:02}")),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .w(px(VALUE_COLUMN_WIDTH))
                        .pr(px(LANE_INSET))
                        .text_right()
                        .text_size(px(VALUE_FONT_PX))
                        .font_family(data_font_family())
                        .text_color(if active { palette.accent } else { palette.text })
                        .child(cn0_value_text(value)),
                )
        }))
        // Axis strip: tick labels on the real window + axis title carrying
        // the value unit (caption 11px, spec §3.7).
        .child(
            div()
                .absolute()
                .left(px(PRN_COLUMN_WIDTH + LANE_INSET))
                .right(px(VALUE_COLUMN_WIDTH + LANE_INSET))
                .bottom(px(0.))
                .h(px(AXIS_ROW_HEIGHT))
                .flex()
                .flex_col()
                .justify_end()
                .child(
                    div()
                        .relative()
                        .h(px(TICK_FONT_PX + 2.))
                        .font_family(data_font_family())
                        .text_size(px(TICK_FONT_PX))
                        .text_color(palette.faint)
                        .children(TICK_FRACTIONS.iter().map(|fraction| {
                            let offset_s = (history_s as f64 * (1.0 - fraction)) as u64;
                            div()
                                .absolute()
                                .left(gpui_kit::relative(*fraction as f32))
                                .ml(px(-18.))
                                .w(px(36.))
                                .text_center()
                                .child(tick_label(offset_s))
                        })),
                )
                .child(
                    div()
                        .h(px(TITLE_FONT_PX + 4.))
                        .text_center()
                        .text_size(px(TITLE_FONT_PX))
                        .text_color(palette.faint)
                        .child(axis_title(history_s)),
                ),
        )
        // Honest empty state; the fixed lane grid above stays visible.
        .children((series.is_empty()).then(|| frame::state_caption(&PlotState::Empty, palette)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GuiChannelStatus;

    fn channel(prn: u8, cn0: f64) -> GuiChannelStatus {
        GuiChannelStatus {
            prn,
            acquired: true,
            tracking_locked: true,
            nav_locked: true,
            cn0_db_hz: cn0,
            doppler_hz: -1_500.0,
            residual_m: Some(0.0),
        }
    }

    #[test]
    fn exactly_nine_lanes_at_the_fixed_pitch() {
        assert_eq!(LANE_COUNT, 9);
        assert_eq!(metrics_lane_pitch(), 28.0);
        assert_eq!(LANE_PITCH, crate::visual_system::metrics::CN0_LANE_PITCH);
    }

    fn metrics_lane_pitch() -> f32 {
        crate::visual_system::metrics::CN0_LANE_PITCH
    }

    #[test]
    fn lane_order_is_current_cn0_descending_with_prn_tiebreak() {
        let epochs = vec![
            vec![channel(22, 38.0), channel(3, 42.0), channel(8, 42.0)],
            vec![channel(22, 45.0), channel(3, 41.0), channel(8, 44.0)],
        ];
        let series = build_cn0_series(&epochs, Some(1), None, LANE_COUNT);
        // Current (last) values: 22→45, 8→44, 3→41: descending order.
        let prns: Vec<u8> = series.iter().map(|item| item.prn).collect();
        assert_eq!(prns, vec![22, 8, 3]);
    }

    #[test]
    fn equal_cn0_orders_by_ascending_prn() {
        let epochs = vec![vec![channel(9, 41.0), channel(4, 41.0), channel(17, 41.0)]];
        let series = build_cn0_series(&epochs, Some(0), None, LANE_COUNT);
        let prns: Vec<u8> = series.iter().map(|item| item.prn).collect();
        assert_eq!(prns, vec![4, 9, 17]);
    }

    #[test]
    fn pinned_prn_keeps_the_top_lane() {
        let epochs = vec![vec![channel(22, 45.0), channel(3, 39.0)]];
        let series = build_cn0_series(&epochs, Some(0), Some(3), LANE_COUNT);
        assert_eq!(series.first().map(|item| item.prn), Some(3));
    }

    #[test]
    fn value_column_carries_the_dbhz_unit_and_honest_dash() {
        assert_eq!(VALUE_UNIT, "(dB-Hz)");
        // One decimal place; use values with an unambiguous rounding direction.
        assert_eq!(cn0_value_text(Some(41.26)), "41.3");
        assert_eq!(cn0_value_text(Some(41.24)), "41.2");
        // Missing values are an em dash, never a fabricated zero.
        assert_eq!(cn0_value_text(None), "—");
        assert_ne!(cn0_value_text(None), "0");
    }

    #[test]
    fn axis_title_reports_the_real_window_and_ticks_use_unicode_minus() {
        // Spec §3.7 / review F2: the value unit rides with the axis title at
        // caption 11px (TITLE_FONT_PX), not in the 10px tick font.
        assert_eq!(axis_title(300), "历元（过去 300 s） (dB-Hz)");
        assert_eq!(axis_title(45), "历元（过去 45 s） (dB-Hz)");
        assert_eq!(TITLE_FONT_PX, 11.0);
        assert_ne!(TITLE_FONT_PX, TICK_FONT_PX);
        assert_eq!(tick_label(300), "\u{2212}300");
        assert_eq!(tick_label(0), "0");
        assert_eq!(history_seconds(301, 1_000), 300);
    }

    #[test]
    fn nine_series_colors_are_pairwise_distinct() {
        let palette = Palette::light();
        let colors = palette.chart_series;
        let lane_colors: Vec<Hsla> = (0..LANE_COUNT)
            .map(|lane| colors[lane % colors.len()])
            .collect();
        for (index, color) in lane_colors.iter().enumerate() {
            for (other_index, other) in lane_colors.iter().enumerate() {
                if index != other_index {
                    assert_ne!(color, other, "lane {index} vs {other_index}");
                }
            }
        }
    }

    #[test]
    fn light_surface_labels_fall_back_to_a_darker_same_hue() {
        let palette = Palette::light();
        let surface = palette.surface;
        // chart-5 (0xc65f17) is the documented low-contrast light case.
        let series_color = palette.chart_series[4];
        let label = cn0_label_color(series_color, surface);
        if contrast_ratio(series_color, surface) < LABEL_CONTRAST_MIN {
            // Same hue and saturation, strictly darker, now legible.
            assert_eq!(label.h, series_color.h);
            assert_eq!(label.s, series_color.s);
            assert!(label.l < series_color.l);
            assert!(contrast_ratio(label, surface) >= LABEL_CONTRAST_MIN);
        }
        // A colour that already clears the threshold is untouched.
        let strong = palette.chart_series[7];
        if contrast_ratio(strong, surface) >= LABEL_CONTRAST_MIN {
            assert_eq!(cn0_label_color(strong, surface), strong);
        }
        // Dark surfaces pass palette colours through unchanged.
        let dark = Palette::dark();
        let dark_series = dark.chart_series[4];
        assert_eq!(cn0_label_color(dark_series, dark.surface), dark_series);
    }
}
