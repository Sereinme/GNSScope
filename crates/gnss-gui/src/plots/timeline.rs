//! Epoch timeline drawing for the confirmed instrument (design-refresh spec §3.10).
//!
//! This module owns the frozen 85px track layer contract, the three-level tick
//! ladder, event cap shapes, the playhead callout flip, boundary-caption
//! placement, and the right-cluster minimap drawing. It renders display-only
//! data supplied by `app.rs`; interaction callbacks and state ownership stay
//! in the application.
//!
//! Pixel-clearance rules are evaluated against `NARROW_TRACK_REF`, the
//! narrowest track width observed across the acceptance window matrix
//! (1568/1440/1100/800), so clearance decisions stay conservative on wider
//! windows instead of clipping.

use crate::ui::Localize;
use crate::visual_system::{data_font_family, metrics, Palette};
use crate::{GuiDiagnosticEvent, GuiDiagnosticEventKind};
use gpui_kit::prelude::*;
use gpui_kit::{canvas, div, point, px, quad, size, AnyElement, Bounds, PathBuilder, SharedString};

/// Vertical layer contract of the 85px track (spec §3.10 / issue 13).
const LANE_TOP: f32 = 19.0;
const LANE_HEIGHT: f32 = 42.0;
const STEM_TOP: f32 = 27.0;
const STEM_HEIGHT: f32 = 34.0;
const CAP_RING: f32 = 8.0;
const BASELINE_TOP: f32 = 73.0;
const PROGRESS_HEIGHT: f32 = 3.0;
const CALLOUT_HEIGHT: f32 = 15.0;
const CALLOUT_GAP: f32 = 10.0;
/// Cursor fraction beyond which the callout flips to the left of the playhead.
/// Chosen so the chip plus gap clears the right edge at the narrowest track
/// (1 - (10 + 160) / 585 = 0.70 with margin).
const CALLOUT_FLIP: f32 = 0.68;
/// Conservative rendered width of "00:02:04.8 · 历元 1248" at 11px.
const CALLOUT_WIDTH_EST: f32 = 160.0;
/// Conservative rendered width of "处理边界 63%" at 11px.
const CAPTION_WIDTH_EST: f32 = 96.0;
/// Narrowest track width in the acceptance matrix (1100x800 layout).
const NARROW_TRACK_REF: f32 = 585.0;
/// Right padding of the track area; captions may borrow it before the divider.
const TRACK_RIGHT_PAD: f32 = 12.0;
/// Upper bound on tick marks drawn in one frame.
const TICK_CAP: usize = 480;
/// Maximum event stems drawn inside the visible window.
const STEM_CAP: usize = 64;
/// Minimap envelope resolution (buckets across the record).
pub(crate) const ENVELOPE_BUCKETS: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventKind {
    Acquisition,
    Tracking,
    Navigation,
    Loss,
}

impl EventKind {
    fn color(self, palette: Palette) -> gpui_kit::Hsla {
        match self {
            Self::Acquisition => palette.attention,
            Self::Tracking | Self::Navigation => palette.healthy,
            Self::Loss => palette.danger,
        }
    }

    fn category(self) -> &'static str {
        match self {
            Self::Acquisition => "ACQ",
            Self::Tracking => "TRK",
            Self::Navigation => "NAV",
            Self::Loss => "LOSS",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackEvent {
    pub(crate) time_ms: u64,
    pub(crate) kind: EventKind,
}

/// Map a persisted diagnostic event to a timeline stem category.
///
/// Acquisition and reacquisition share the ACQ cap; both failure kinds map to
/// the LOSS cap; channel state is TRK and navigation decoding is NAV. The
/// header legend carries the identity, stems carry cap shape and color only.
pub(crate) fn track_event(event: &GuiDiagnosticEvent) -> TrackEvent {
    let kind = match event.kind {
        GuiDiagnosticEventKind::Acquisition | GuiDiagnosticEventKind::Reacquisition => {
            EventKind::Acquisition
        }
        GuiDiagnosticEventKind::ChannelState => EventKind::Tracking,
        GuiDiagnosticEventKind::Navigation => EventKind::Navigation,
        GuiDiagnosticEventKind::AcquisitionFailed | GuiDiagnosticEventKind::ReacquisitionFailed => {
            EventKind::Loss
        }
    };
    TrackEvent {
        time_ms: event.now_ms,
        kind,
    }
}

pub(crate) struct TrackView {
    /// Full record duration of the visible run, milliseconds (>= 1).
    pub(crate) duration_ms: u64,
    /// Global fraction of the visible window start, 0..1.
    pub(crate) visible_start: f32,
    /// Visible window length as a fraction of the record, (0, 1].
    pub(crate) visible_span: f32,
    /// Cursor position inside the visible window, 0..1.
    pub(crate) cursor_percent: f32,
    /// 1-based selected epoch number for the callout.
    pub(crate) epoch_number: usize,
    /// Real processed progress of the selected run, 0..1.
    pub(crate) processed: f32,
    /// All persisted events of the selected run (window filtering happens here).
    pub(crate) events: Vec<TrackEvent>,
}

impl TrackView {
    fn window_ms(&self) -> (u64, u64) {
        let start = (self.duration_ms as f64 * self.visible_start as f64) as u64;
        let span = ((self.duration_ms as f64 * self.visible_span as f64) as u64).max(1);
        (start, span)
    }

    fn cursor_time_ms(&self) -> u64 {
        let (start, span) = self.window_ms();
        start + (span as f32 * self.cursor_percent) as u64
    }
}

// ---------------------------------------------------------------------------
// Pure geometry helpers (unit-tested below)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TickLadder {
    pub(crate) minor_step_ms: u64,
    pub(crate) mid_step_ms: u64,
    pub(crate) major_step_ms: u64,
}

/// Nice tick steps in seconds. The reference record (960 s) selects
/// major 120 s / mid 60 s / minor 30 s, matching the confirmed mockup.
const NICE_STEPS_S: &[u64] = &[
    1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1_800, 3_600, 7_200,
];

pub(crate) fn tick_ladder(visible_duration_ms: u64) -> TickLadder {
    let seconds = (visible_duration_ms / 1_000).max(1);
    let major_s = *NICE_STEPS_S
        .iter()
        .find(|step| **step * 8 >= seconds)
        .unwrap_or(&18_000);
    let mid_s = *NICE_STEPS_S
        .iter()
        .rev()
        .find(|step| **step * 2 <= major_s)
        .unwrap_or(&1);
    let minor_s = *NICE_STEPS_S
        .iter()
        .rev()
        .find(|step| **step * 4 <= major_s)
        .unwrap_or(&1);
    TickLadder {
        minor_step_ms: minor_s * 1_000,
        mid_step_ms: mid_s * 1_000,
        major_step_ms: major_s * 1_000,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TickLevel {
    Minor,
    Mid,
    Major,
}

/// Tick positions inside `[start_ms, start_ms + span_ms]` as window fractions.
/// Level precedence is major > mid > minor so marks never stack.
pub(crate) fn ticks(ladder: TickLadder, start_ms: u64, span_ms: u64) -> Vec<(f32, TickLevel)> {
    if span_ms == 0 {
        return Vec::new();
    }
    let minor = ladder.minor_step_ms.max(1);
    let mut out = Vec::new();
    let mut t = start_ms.div_ceil(minor) * minor;
    let end = start_ms + span_ms;
    while t <= end {
        let level = if t.is_multiple_of(ladder.major_step_ms) {
            Some(TickLevel::Major)
        } else if t.is_multiple_of(ladder.mid_step_ms) {
            Some(TickLevel::Mid)
        } else {
            Some(TickLevel::Minor)
        };
        if let Some(level) = level {
            // Minor ticks are dropped first when a record is extremely long.
            if level != TickLevel::Minor || out.len() < TICK_CAP {
                out.push((((t - start_ms) as f32) / span_ms as f32, level));
            }
        }
        t += minor;
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabelAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AxisLabel {
    pub(crate) percent: f32,
    pub(crate) time: String,
    pub(crate) align: LabelAlign,
}

/// Axis labels: anchored start and end labels (never clipped by the divider)
/// plus major-tick labels strictly inside the window. Major labels closer
/// than 9% of the window to either edge are dropped because the anchored
/// label covers them (reference 960 s record yields the mockup's 9 labels).
pub(crate) fn axis_label_set(
    start_ms: u64,
    span_ms: u64,
    major_step_ms: u64,
    format: fn(u64) -> String,
) -> Vec<AxisLabel> {
    if span_ms == 0 {
        return Vec::new();
    }
    let mut labels = vec![AxisLabel {
        percent: 0.0,
        time: format(start_ms),
        align: LabelAlign::Left,
    }];
    let major = major_step_ms.max(1);
    let mut t = start_ms.div_ceil(major) * major;
    let end = start_ms + span_ms;
    while t < end {
        let percent = (t - start_ms) as f32 / span_ms as f32;
        if (0.09..=0.91).contains(&percent) {
            labels.push(AxisLabel {
                percent,
                time: format(t),
                align: LabelAlign::Center,
            });
        }
        t += major;
    }
    labels.push(AxisLabel {
        percent: 1.0,
        time: format(end),
        align: LabelAlign::Right,
    });
    labels
}

/// Does the callout flip to the left of the playhead? Conservative: flips
/// before strict necessity so the chip never reaches the divider at any
/// window width in the acceptance matrix.
pub(crate) fn callout_flipped(cursor_percent: f32) -> bool {
    cursor_percent > CALLOUT_FLIP
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundaryCaption {
    /// `true` = caption right of the line, `false` = left of the line.
    pub(crate) right_of_line: bool,
    /// Caption top inside the 85px track (0 = callout row, 46 = lane interior).
    pub(crate) top: f32,
    pub(crate) visible: bool,
}

/// Place the boundary caption so it keeps >= 4px from the playhead callout
/// and never crosses the track edge. Falls back to the stem-free lane
/// interior right of the processed edge, then hides (the boundary line and
/// progress cap remain) when no placement is clear.
pub(crate) fn boundary_caption(boundary: f32, cursor: f32, stems: &[f32]) -> BoundaryCaption {
    let boundary_px = boundary * NARROW_TRACK_REF;
    let cursor_px = cursor * NARROW_TRACK_REF;
    let (callout_left, callout_right) = if callout_flipped(cursor) {
        (
            cursor_px - CALLOUT_GAP - CALLOUT_WIDTH_EST,
            cursor_px - CALLOUT_GAP,
        )
    } else {
        (
            cursor_px + CALLOUT_GAP,
            cursor_px + CALLOUT_GAP + CALLOUT_WIDTH_EST,
        )
    };
    let overlaps_callout =
        |left: f32, right: f32| left < callout_right + 4.0 && right > callout_left - 4.0;
    let right_room = NARROW_TRACK_REF - boundary_px + TRACK_RIGHT_PAD;
    if right_room >= CAPTION_WIDTH_EST
        && !overlaps_callout(boundary_px, boundary_px + CAPTION_WIDTH_EST)
    {
        return BoundaryCaption {
            right_of_line: true,
            top: 0.0,
            visible: true,
        };
    }
    if boundary_px >= CAPTION_WIDTH_EST
        && !overlaps_callout(boundary_px - CAPTION_WIDTH_EST, boundary_px)
    {
        return BoundaryCaption {
            right_of_line: false,
            top: 0.0,
            visible: true,
        };
    }
    if right_room >= CAPTION_WIDTH_EST {
        return BoundaryCaption {
            right_of_line: true,
            top: 46.0,
            visible: true,
        };
    }
    let stem_near_left = stems
        .iter()
        .any(|stem| stem * NARROW_TRACK_REF >= boundary_px - CAPTION_WIDTH_EST - 4.0);
    if !stem_near_left {
        return BoundaryCaption {
            right_of_line: false,
            top: 46.0,
            visible: true,
        };
    }
    BoundaryCaption {
        right_of_line: true,
        top: 0.0,
        visible: false,
    }
}

/// Downsample the per-epoch peak C/N0 series onto `buckets` columns scaled to
/// the record duration. Returns `None` when no real amplitude exists; empty
/// columns stay `None` so the renderer breaks the polyline instead of
/// fabricating zeros.
pub(crate) fn envelope_series(
    epoch_cn0: &[Option<f32>],
    period_ms: u64,
    duration_ms: u64,
    buckets: usize,
) -> Option<Vec<Option<f32>>> {
    if epoch_cn0.is_empty() || duration_ms == 0 || buckets == 0 {
        return None;
    }
    let peak = epoch_cn0.iter().flatten().copied().fold(0.0_f32, f32::max);
    if !peak.is_finite() || peak <= 0.0 {
        return None;
    }
    let period_ms = period_ms.max(1);
    let values = (0..buckets)
        .map(|bucket| {
            // Bucket center time, so columns align with the record timeline.
            let time_ms = ((bucket as u64 * 2 + 1) * duration_ms) / (buckets as u64 * 2);
            let index = (time_ms / period_ms) as usize;
            epoch_cn0
                .get(index)
                .and_then(|value| *value)
                .map(|value| (value / peak).clamp(0.0, 1.0))
        })
        .collect::<Vec<_>>();
    values.iter().any(Option::is_some).then_some(values)
}

/// Vertical placement of one real envelope sample inside the minimap box.
///
/// Sparkline convention: the polyline auto-scales to the observed min..max
/// band of the real per-epoch values. This is an affine display transform of
/// the observed range only — no value is altered, interpolated or
/// synthesized — and it keeps a nearly constant but real series visible
/// instead of collapsing it onto the viewport frame. A degenerate range
/// renders as a flat centered line.
pub(crate) fn envelope_y(value: f32, lo: f32, hi: f32, height: f32) -> f32 {
    /// Vertical inset so the polyline stays inside the accent viewport frame
    /// (frame inset 2px + 1px border + 1px breathing room).
    const BAND_INSET: f32 = 4.0;
    let range = hi - lo;
    if !range.is_finite() || range <= f32::EPSILON || !value.is_finite() {
        return height / 2.0;
    }
    let norm = ((value - lo) / range).clamp(0.0, 1.0);
    let bottom = height - BAND_INSET;
    let top = BAND_INSET;
    bottom - norm * (bottom - top)
}

/// "00:02:04.8" decisecond timestamp for the playhead callout.
pub(crate) fn format_decis(milliseconds: u64) -> String {
    let decis = (milliseconds % 1_000) / 100;
    format!("{}.{decis}", crate::app::format_timeline_time(milliseconds))
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn event_window(view: &TrackView) -> Vec<(f32, EventKind)> {
    let (start_ms, span_ms) = view.window_ms();
    let duration = view.duration_ms.max(1) as f32;
    let span = span_ms as f32;
    let mut stems = view
        .events
        .iter()
        .rev()
        .filter_map(|event| {
            let global = event.time_ms as f32 / duration;
            let start = start_ms as f32 / duration;
            let end = (start_ms as f32 + span) / duration;
            if global < start || global > end {
                return None;
            }
            Some((
                (event.time_ms.saturating_sub(start_ms)) as f32 / span,
                event.kind,
            ))
        })
        .collect::<Vec<_>>();
    stems.truncate(STEM_CAP);
    stems
}

/// Cap shape drawn at the top of an event stem (6px cap in an 8px surface
/// ring, matching `.cap--*` in theme.css; LOSS uses a glyph triangle).
fn cap(kind: EventKind, palette: Palette) -> AnyElement {
    let color = kind.color(palette);
    let ring = |round: bool| {
        div()
            .size(px(CAP_RING))
            .when(round, |this| this.rounded_full())
            .bg(palette.surface)
            .flex()
            .items_center()
            .justify_center()
    };
    match kind {
        EventKind::Acquisition => ring(false)
            .child(div().size(px(6.)).bg(color))
            .into_any_element(),
        EventKind::Tracking => ring(true)
            .child(div().size(px(6.)).rounded_full().bg(color))
            .into_any_element(),
        EventKind::Navigation => ring(true)
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .border_1()
                    .border_color(color)
                    .bg(palette.surface_alt),
            )
            .into_any_element(),
        EventKind::Loss => div()
            .size(px(CAP_RING))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.))
            .text_color(color)
            .child("▲")
            .into_any_element(),
    }
}

fn tick_element(level: TickLevel, percent: f32, palette: Palette) -> impl IntoElement {
    let (top, height, color) = match level {
        TickLevel::Minor => (BASELINE_TOP + 1.0, 4.0, palette.border_strong.opacity(0.9)),
        TickLevel::Mid => (BASELINE_TOP + 1.0, 6.0, palette.border_strong.opacity(0.9)),
        TickLevel::Major => (BASELINE_TOP + 1.0, 10.0, palette.plot_axis.opacity(0.9)),
    };
    div()
        .absolute()
        .left(gpui_kit::relative(percent))
        .top(px(top))
        .w(px(1.))
        .h(px(height))
        .bg(color)
}

/// The 85px track: processed block, event lane, wash band, ticks, progress
/// hairline, stems, captions, boundary and playhead, plus the transparent
/// seek slider overlay supplied by the app.
pub(crate) fn track(
    view: &TrackView,
    seek: AnyElement,
    localize: Localize,
    palette: Palette,
) -> AnyElement {
    let processed = view.processed.clamp(0.0, 1.0);
    // The boundary marker only exists while the record has both processed and
    // unprocessed regions; completed runs show a full processed block only.
    let show_boundary = (0.005..0.995).contains(&processed);
    let stems = event_window(view);
    let (start_ms, span_ms) = view.window_ms();
    let ladder = tick_ladder(span_ms);
    let stem_percents = stems
        .iter()
        .map(|(percent, _)| *percent)
        .collect::<Vec<_>>();
    let boundary = boundary_caption(processed, view.cursor_percent, &stem_percents);
    let cursor_time = format_decis(view.cursor_time_ms());

    div()
        .relative()
        .h(px(metrics::TIMELINE_TRACK_HEIGHT))
        .w_full()
        // 1. Processed region: flat surface-alt block sharing its right edge
        //    with the boundary marker (no gradients, full track height).
        .children((processed > 0.0005).then(|| {
            div()
                .absolute()
                .left_0()
                .top_0()
                .h(px(metrics::TIMELINE_TRACK_HEIGHT))
                .w(gpui_kit::relative(processed))
                .bg(palette.surface_alt)
        }))
        // 2. Event lane band: flat surface-alt, 1px top hairline.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(LANE_TOP))
                .h(px(LANE_HEIGHT))
                .bg(palette.surface_alt)
                .border_t_1()
                .border_color(palette.border),
        )
        // 3. Playhead selection wash band (under ticks, stems and progress).
        .child(
            div()
                .absolute()
                .left(gpui_kit::relative(view.cursor_percent))
                .ml(px(-5.))
                .top_0()
                .h(px(metrics::TIMELINE_TRACK_HEIGHT))
                .w(px(10.))
                .bg(palette.accent_wash),
        )
        // 4. Tick baseline + three-level ladder scaled to the window.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(BASELINE_TOP))
                .h(px(1.))
                .bg(palette.plot_axis.opacity(0.85)),
        )
        .children(
            ticks(ladder, start_ms, span_ms)
                .into_iter()
                .map(|(percent, level)| tick_element(level, percent, palette)),
        )
        // 5. Progress hairline: unavailable-wash groove + accent fill to the
        //    processed edge; the fill end cap is the processing boundary.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(PROGRESS_HEIGHT))
                .bg(palette.unavailable_wash)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(gpui_kit::relative(processed))
                        .bg(palette.accent),
                ),
        )
        // 6. Event stems: 34px line + cap shape/color only (no stem text).
        .children(
            stems
                .into_iter()
                .enumerate()
                .map(|(index, (percent, kind))| {
                    div()
                        .id(SharedString::from(format!("timeline-stem-{index}")))
                        .absolute()
                        .left(gpui_kit::relative(percent))
                        .top(px(STEM_TOP))
                        .w(px(1.))
                        .h(px(STEM_HEIGHT))
                        .bg(kind.color(palette))
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .ml(px(-(CAP_RING - 1.0) / 2.0))
                                .top(px(-4.))
                                .child(cap(kind, palette)),
                        )
                }),
        )
        // 7. Lane identity caption (opaque band-colored backing keeps stem
        //    lines from crossing the text).
        .child(
            div()
                .absolute()
                .left(px(2.))
                .top(px(33.))
                .px(px(2.))
                .bg(palette.surface_alt)
                .text_size(px(11.))
                .text_color(palette.faint)
                .child(localize("事件", "Events")),
        )
        // 8. Processing boundary: 1px tertiary line sharing the processed
        //    block's edge, plus a collision-aware caption.
        .children(show_boundary.then(|| {
            let caption_text = format!(
                "{} {}%",
                localize("处理边界", "Boundary"),
                (processed * 100.0).round() as u32
            );
            div()
                .absolute()
                .left(gpui_kit::relative(processed))
                .top_0()
                .h(px(metrics::TIMELINE_TRACK_HEIGHT))
                .w(px(1.))
                .bg(palette.faint)
                .when(boundary.visible, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top(px(boundary.top))
                            .text_size(px(11.))
                            .text_color(palette.faint)
                            .whitespace_nowrap()
                            .when(boundary.right_of_line, |text| text.left(px(4.)))
                            .when(!boundary.right_of_line, |text| text.right(px(4.)))
                            .child(caption_text),
                    )
                })
        }))
        // 9. Playhead: 1.5px accent line, 8px cap and the two-part callout
        //    with a 1px accent connector (flips inward near the right edge).
        .child(
            div()
                .absolute()
                .left(gpui_kit::relative(view.cursor_percent))
                .ml(px(-0.75))
                .top_0()
                .h(px(metrics::TIMELINE_TRACK_HEIGHT))
                .w(px(1.5))
                .bg(palette.accent)
                .child(
                    div()
                        .absolute()
                        .top(px(-1.))
                        .left(px(-3.5))
                        .text_size(px(9.))
                        .line_height(px(10.))
                        .text_color(palette.accent)
                        .child("▼"),
                )
                .child(callout(view, cursor_time, localize, palette)),
        )
        // 10. Transparent seek slider spanning the track (shared cursor).
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .cursor_ew_resize()
                .child(seek),
        )
        .into_any_element()
}

fn callout(
    view: &TrackView,
    cursor_time: String,
    localize: Localize,
    palette: Palette,
) -> AnyElement {
    let flipped = callout_flipped(view.cursor_percent);
    let chip = div()
        .absolute()
        .top(px(0.))
        .when(flipped, |this| this.right(px(CALLOUT_GAP)))
        .when(!flipped, |this| this.left(px(CALLOUT_GAP)))
        .h(px(CALLOUT_HEIGHT))
        .px(px(5.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface_raised)
        // 1px accent connector anchoring the chip to the playhead.
        .child(
            div()
                .absolute()
                .top(px(6.))
                .when(flipped, |this| this.right(px(-11.)))
                .when(!flipped, |this| this.left(px(-11.)))
                .w(px(10.))
                .h(px(1.))
                .bg(palette.accent),
        )
        .child(
            div()
                .font_family(data_font_family())
                .text_size(px(11.))
                .text_color(palette.accent)
                .child(cursor_time),
        )
        .child(
            div()
                .mx(px(4.))
                .text_size(px(11.))
                .text_color(palette.faint)
                .child("·"),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(format!(
                    "{} {}",
                    localize("历元", "Epoch"),
                    view.epoch_number
                )),
        );
    chip.into_any_element()
}

/// Axis label row (18px) beneath the track. The end label is right-aligned to
/// the track edge, leaving the 12px right gutter clear of the divider.
pub(crate) fn axis_labels(view: &TrackView, palette: Palette) -> AnyElement {
    let (start_ms, span_ms) = view.window_ms();
    let ladder = tick_ladder(span_ms);
    div()
        .relative()
        .h(px(metrics::TIMELINE_LABELS_HEIGHT))
        .w_full()
        .children(
            axis_label_set(start_ms, span_ms, ladder.major_step_ms, |time| {
                crate::app::format_timeline_time(time)
            })
            .into_iter()
            .map(|label| {
                let base = div()
                    .absolute()
                    .top(px(3.))
                    .font_family(data_font_family())
                    .text_size(px(10.))
                    .text_color(palette.faint)
                    .whitespace_nowrap();
                match label.align {
                    LabelAlign::Left => base.left(px(0.)).child(label.time),
                    LabelAlign::Center => base
                        .left(gpui_kit::relative(label.percent))
                        .ml(px(-26.))
                        .w(px(52.))
                        .text_center()
                        .child(label.time),
                    LabelAlign::Right => {
                        base.right(px(0.)).w(px(52.)).text_right().child(label.time)
                    }
                }
            }),
        )
        .into_any_element()
}

/// Header event legend: cap shape is the legend; labels are fixed keys.
pub(crate) fn legend(palette: Palette) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .children(
            [
                EventKind::Acquisition,
                EventKind::Tracking,
                EventKind::Navigation,
                EventKind::Loss,
            ]
            .into_iter()
            .map(|kind| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(px(11.))
                    .text_color(palette.faint)
                    .child(cap(kind, palette))
                    .child(kind.category())
            }),
        )
        .into_any_element()
}

/// Right-cluster minimap (144x18): real amplitude envelope when the selected
/// run carries per-epoch C/N0, otherwise an honest 不可用 hatch. The accent
/// viewport frame is inset 2px and follows the visible window.
pub(crate) fn minimap(
    view: &TrackView,
    envelope: Option<&[Option<f32>]>,
    localize: Localize,
    palette: Palette,
) -> AnyElement {
    let width = metrics::TIMELINE_TRANSPORT_WIDTH;
    let height = 18.0;
    let envelope = envelope.map(<[Option<f32>]>::to_vec);
    let frame_start = view.visible_start;
    let frame_span = view.visible_span;
    let hatch = envelope.is_none();
    let draw_envelope = envelope.clone();
    div()
        .relative()
        .w(px(width))
        .h(px(height))
        .bg(palette.surface_alt)
        // The canvas is a leaf element: without an explicit size gpui lays it
        // out as 144x0 in this column and the paint closure early-returns, so
        // pin it to the minimap box (144x18) instead of relying on stretch.
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds: Bounds<gpui_kit::Pixels>, _, window, _| {
                    paint_minimap(
                        bounds,
                        draw_envelope.as_deref(),
                        frame_start,
                        frame_span,
                        hatch,
                        palette,
                        window,
                    );
                },
            )
            .w(px(width))
            .h(px(height)),
        )
        .when(hatch, |this| {
            this.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(3.))
                    .text_size(px(10.))
                    .text_center()
                    .text_color(palette.faint)
                    .child(localize("不可用", "Unavailable")),
            )
        })
        .into_any_element()
}

fn paint_minimap(
    bounds: Bounds<gpui_kit::Pixels>,
    envelope: Option<&[Option<f32>]>,
    frame_start: f32,
    frame_span: f32,
    hatch: bool,
    palette: Palette,
    window: &mut gpui_kit::Window,
) {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    if w <= 2.0 || h <= 2.0 {
        return;
    }
    if hatch {
        // Diagonal unavailable hatch; no synthetic waveform is ever drawn.
        let step = 7.0;
        let mut x = step;
        while x < w {
            let x_end = x - (h - 2.0);
            if x_end > 1.0 {
                if let Ok(path) = {
                    let mut builder = PathBuilder::stroke(px(1.0));
                    builder.move_to(point(bounds.origin.x + px(x), bounds.origin.y + px(1.0)));
                    builder.line_to(point(
                        bounds.origin.x + px(x_end),
                        bounds.origin.y + px(h - 1.0),
                    ));
                    builder.build()
                } {
                    window.paint_path(path, palette.border_strong.opacity(0.7));
                }
            }
            x += step;
        }
    }
    if let Some(envelope) = envelope {
        let n = envelope.len().max(2);
        let lo = envelope.iter().flatten().cloned().fold(f32::MAX, f32::min);
        let hi = envelope.iter().flatten().cloned().fold(f32::MIN, f32::max);
        let mut builder = PathBuilder::stroke(px(1.0));
        let mut pen_down = false;
        for (index, value) in envelope.iter().enumerate() {
            let Some(value) = value else {
                pen_down = false;
                continue;
            };
            let x = px(((index as f32 + 0.5) / n as f32) * w);
            let y = px(envelope_y(*value, lo, hi, h));
            if pen_down {
                builder.line_to(point(bounds.origin.x + x, bounds.origin.y + y));
            } else {
                builder.move_to(point(bounds.origin.x + x, bounds.origin.y + y));
                pen_down = true;
            }
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, palette.faint);
        }
    }
    // Accent viewport frame, inset 2px so it never touches the minimap edge.
    let frame_x = 2.0 + frame_start * (w - 4.0);
    let frame_w = ((frame_span * (w - 4.0)).max(2.0)).min(w - 4.0 - (frame_x - 2.0).max(0.0));
    window.paint_quad(quad(
        Bounds {
            origin: point(bounds.origin.x + px(frame_x), bounds.origin.y + px(2.0)),
            size: size(px(frame_w.max(2.0)), px(h - 4.0)),
        },
        0.0,
        gpui_kit::transparent_black(),
        px(1.0),
        palette.accent,
        Default::default(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_record_selects_the_mockup_tick_ladder() {
        // 960 s reference record: minor 4px/30 s, mid 6px/60 s, major 120 s.
        let ladder = tick_ladder(960_000);
        assert_eq!(ladder.minor_step_ms, 30_000);
        assert_eq!(ladder.mid_step_ms, 60_000);
        assert_eq!(ladder.major_step_ms, 120_000);
    }

    #[test]
    fn tick_ladder_grows_with_record_duration() {
        let short = tick_ladder(240_000);
        assert_eq!(short.major_step_ms, 30_000);
        let long = tick_ladder(86_400_000);
        assert!(long.major_step_ms > 3_600_000);
        assert!(long.minor_step_ms < long.mid_step_ms);
        assert!(long.mid_step_ms < long.major_step_ms);
    }

    #[test]
    fn ticks_cover_the_window_with_level_precedence() {
        let ladder = tick_ladder(960_000);
        let ticks = ticks(ladder, 0, 960_000);
        // 33 marks over 0..960 s: 9 majors (120 s), 8 mids (odd 60 s) and
        // 16 minors (odd 30 s); marks never stack across levels.
        assert_eq!(ticks.len(), 33);
        assert!(ticks
            .iter()
            .any(|(p, l)| (*p - 1.0).abs() < 1e-6 && *l == TickLevel::Major));
        let majors = ticks.iter().filter(|(_, l)| *l == TickLevel::Major).count();
        assert_eq!(majors, 9);
        let mids = ticks.iter().filter(|(_, l)| *l == TickLevel::Mid).count();
        assert_eq!(mids, 8);
        let minors = ticks.iter().filter(|(_, l)| *l == TickLevel::Minor).count();
        assert_eq!(minors, 16);
    }

    #[test]
    fn axis_labels_anchor_both_edges_and_drop_edge_major_duplicates() {
        let labels = axis_label_set(0, 960_000, 120_000, |time| {
            crate::app::format_timeline_time(time)
        });
        assert_eq!(labels.len(), 9);
        assert_eq!(labels[0].align, LabelAlign::Left);
        assert_eq!(labels[0].time, "00:00:00");
        assert_eq!(labels.last().unwrap().align, LabelAlign::Right);
        assert_eq!(labels.last().unwrap().time, "00:16:00");
        assert_eq!(labels[1].time, "00:02:00");
        assert_eq!(labels[1].percent, 0.125);
    }

    #[test]
    fn axis_labels_never_clip_the_right_gutter_when_zoomed_off_step() {
        // Window 61 s..301 s: end label is the window end, not a major tick.
        let labels = axis_label_set(61_000, 240_000, 30_000, |time| {
            crate::app::format_timeline_time(time)
        });
        let last = labels.last().unwrap();
        assert_eq!(last.align, LabelAlign::Right);
        assert_eq!(last.time, "00:05:01");
        assert!(labels
            .iter()
            .filter(|label| label.align == LabelAlign::Center)
            .all(|label| label.percent <= 0.91));
    }

    #[test]
    fn callout_flips_before_reaching_the_divider() {
        assert!(!callout_flipped(0.0));
        assert!(!callout_flipped(0.68));
        assert!(callout_flipped(0.69));
        assert!(callout_flipped(1.0));
    }

    #[test]
    fn boundary_caption_keeps_clear_of_the_callout_and_edges() {
        // Cursor far left: caption sits right of the 63% boundary line.
        let caption = boundary_caption(0.63, 0.13, &[]);
        assert!(caption.visible);
        assert!(caption.right_of_line);
        assert_eq!(caption.top, 0.0);

        // Cursor overlapping the right-side caption span: drop into the lane
        // interior right of the line (stem-free by contract).
        let caption = boundary_caption(0.63, 0.60, &[]);
        assert!(caption.visible);
        assert_eq!(caption.top, 46.0);
        assert!(caption.right_of_line);

        // Near-complete run: caption flips left of the line at the top row.
        let caption = boundary_caption(0.9, 0.3, &[]);
        assert!(caption.visible);
        assert!(!caption.right_of_line);
        assert_eq!(caption.top, 0.0);
    }

    #[test]
    fn envelope_y_auto_scales_the_real_band_and_flattens_degenerate_ranges() {
        // Min/max samples pin the band edges (18px box, 4px inset).
        assert_eq!(envelope_y(10.0, 10.0, 20.0, 18.0), 14.0);
        assert_eq!(envelope_y(20.0, 10.0, 20.0, 18.0), 4.0);
        assert_eq!(envelope_y(15.0, 10.0, 20.0, 18.0), 9.0);
        // A nearly constant real series stays visible instead of collapsing
        // onto the viewport frame, and a degenerate range centers a flat line.
        assert_eq!(envelope_y(0.994, 0.952, 0.994, 18.0), 4.0);
        assert_eq!(envelope_y(0.952, 0.952, 0.994, 18.0), 14.0);
        assert_eq!(envelope_y(42.0, 42.0, 42.0, 18.0), 9.0);
        assert_eq!(envelope_y(f32::NAN, 0.0, 1.0, 18.0), 9.0);
    }

    #[test]
    fn envelope_requires_real_amplitude_and_keeps_gaps_empty() {
        assert!(envelope_series(&[], 100, 1_000, 4).is_none());
        assert!(envelope_series(&[Some(0.0), None], 100, 1_000, 4).is_none());
        let series = envelope_series(&[Some(30.0), Some(40.0), None, Some(20.0)], 100, 400, 4)
            .expect("real amplitude yields a series");
        assert_eq!(series.len(), 4);
        assert_eq!(series[0], Some(0.75));
        assert_eq!(series[1], Some(1.0));
        assert_eq!(
            series[2], None,
            "the epoch without channel data stays an empty column"
        );
        assert_eq!(series[3], Some(0.5));
        // A bucket past the epoch range stays empty rather than zero-filled.
        let series = envelope_series(&[Some(40.0)], 100, 400, 4).unwrap();
        assert_eq!(series[3], None);
    }

    #[test]
    fn diagnostic_event_kinds_map_to_the_four_legend_categories() {
        let event = |kind| GuiDiagnosticEvent {
            now_ms: 0,
            sample_index: 0,
            prn: None,
            kind,
            detail: String::new(),
        };
        assert_eq!(
            track_event(&event(GuiDiagnosticEventKind::Acquisition)).kind,
            EventKind::Acquisition
        );
        assert_eq!(
            track_event(&event(GuiDiagnosticEventKind::Reacquisition)).kind,
            EventKind::Acquisition
        );
        assert_eq!(
            track_event(&event(GuiDiagnosticEventKind::ChannelState)).kind,
            EventKind::Tracking
        );
        assert_eq!(
            track_event(&event(GuiDiagnosticEventKind::Navigation)).kind,
            EventKind::Navigation
        );
        assert_eq!(
            track_event(&event(GuiDiagnosticEventKind::AcquisitionFailed)).kind,
            EventKind::Loss
        );
        assert_eq!(
            track_event(&event(GuiDiagnosticEventKind::ReacquisitionFailed)).kind,
            EventKind::Loss
        );
    }

    #[test]
    fn callout_time_uses_decisecond_instrument_format() {
        assert_eq!(format_decis(0), "00:00:00.0");
        assert_eq!(format_decis(124_800), "00:02:04.8");
        assert_eq!(format_decis(3_661_000), "01:01:01.0");
    }
}
