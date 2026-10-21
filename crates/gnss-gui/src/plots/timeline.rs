//! Epoch timeline.
//!
//! The band is one object: a **signal profile** of per-bucket peak C/N0 drawn
//! as bars with brighter crests, event notches cut through it, a hairline
//! baseline with a tick ruler under it, and a playhead with a triangular
//! handle on the baseline. At a ~20:1 aspect ratio this is the only shape that
//! carries real information without turning into stripes: the body varies, the
//! markers punctuate it, and the ruler anchors the scale.
//!
//! It renders display-only data supplied by `app.rs`; interaction callbacks and
//! state ownership stay in the application.

use crate::visual_system::{data_font_family, metrics, Palette};
use crate::{GuiDiagnosticEvent, GuiDiagnosticEventKind};
use gpui_kit::prelude::*;
use gpui_kit::{canvas, div, point, px, quad, size, AnyElement, Bounds, PathBuilder, SharedString};

/// The band is one object: a coverage ridge rising from a bottom baseline,
/// event notches cut into it, and a ruler under the baseline. There are no
/// lanes and no second axis line — at a 20:1 aspect ratio any further
/// horizontal striping collapses into noise.
const CAP_SIZE: f32 = 6.0;
/// Ridge geometry: bars stand on `BASELINE_TOP` and may use the whole field
/// above it; the ruler occupies the strip below.
const BASELINE_TOP: f32 = 62.0;
/// Height of the time-label row under the baseline, inside the band.
const RULER_LABELS_HEIGHT: f32 = 16.0;
/// Upper bound on tick marks drawn in one frame.
const TICK_CAP: usize = 480;
/// Maximum event stems drawn inside the visible window.
const STEM_CAP: usize = 64;
/// Minimap envelope resolution (buckets across the record).
pub(crate) const PROFILE_BUCKETS: usize = 120;

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

    pub(crate) fn cursor_time_ms(&self) -> u64 {
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

/// Downsample the per-epoch tracked-satellite series onto `buckets` columns.
/// the record duration. Returns `None` when no real amplitude exists; empty
/// columns stay `None` so the renderer breaks the polyline instead of
/// fabricating zeros.
pub(crate) fn profile_series(
    peak_per_epoch: &[Option<u8>],
    period_ms: u64,
    duration_ms: u64,
    buckets: usize,
) -> Option<Vec<Option<u8>>> {
    if peak_per_epoch.is_empty() || duration_ms == 0 || buckets == 0 {
        return None;
    }
    let period_ms = period_ms.max(1);
    let mut values: Vec<Option<u8>> = Vec::with_capacity(buckets);
    for bucket in 0..buckets {
        // Take the MAX across the bucket's epoch span: a bucket is "covered" if
        // any epoch inside it had satellites tracked, which is what lets a
        // sparse but healthy record still show a continuous ridge.
        let from = (bucket as u64 * duration_ms) / buckets as u64;
        let to = ((bucket as u64 + 1) * duration_ms) / buckets as u64;
        let first = (from / period_ms) as usize;
        let last = ((to.max(from + 1) - 1) / period_ms) as usize;
        // MIN within the bucket: an outage that lasts a fraction of a bucket
        // must still show up, so the profile reports the worst moment it covers.
        let mut value: Option<u8> = None;
        for index in first..=last.min(peak_per_epoch.len().saturating_sub(1)) {
            if let Some(count) = peak_per_epoch.get(index).and_then(|v| *v) {
                value = Some(value.map_or(count, |current: u8| current.min(count)));
            }
        }
        values.push(value);
    }
    values.iter().any(Option::is_some).then_some(values)
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
/// A 8x5 triangle drawn as three stacked rules (widest at the base): the only
/// shape the available border API can render crisply at this size.
fn triangle(color: gpui_kit::Hsla, pointing_up: bool) -> AnyElement {
    let widths = [3.0, 5.0, 7.0];
    let rows: Vec<_> = widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            let width = if pointing_up {
                widths[widths.len() - 1 - index]
            } else {
                *width
            };
            div().h(px(1.6)).w(px(width)).bg(color).rounded(px(0.6))
        })
        .collect();
    div()
        .w(px(7.))
        .flex()
        .flex_col()
        .items_center()
        .children(rows)
        .into_any_element()
}

fn cap(kind: EventKind, palette: Palette) -> AnyElement {
    let color = kind.color(palette);
    let ring = |round: bool| {
        div()
            .size(px(CAP_SIZE))
            .when(round, |this| this.rounded_full())
            .bg(palette.surface_alt)
            .flex()
            .items_center()
            .justify_center()
    };
    match kind {
        EventKind::Acquisition => ring(false)
            .child(div().size(px(4.)).bg(color))
            .into_any_element(),
        EventKind::Tracking => ring(true)
            .child(div().size(px(4.)).rounded_full().bg(color))
            .into_any_element(),
        EventKind::Navigation => ring(true)
            .child(
                div()
                    .size(px(4.))
                    .rounded_full()
                    .border_1()
                    .border_color(color)
                    .bg(palette.surface_alt),
            )
            .into_any_element(),
        EventKind::Loss => {
            // A glyph triangle reads as an alarm even at 6px.
            div()
                .size(px(CAP_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .child(triangle(color, false))
                .into_any_element()
        }
    }
}

fn tick_element(level: TickLevel, percent: f32, palette: Palette, top: f32) -> impl IntoElement {
    let (top, height) = match level {
        TickLevel::Minor => (top, 3.0),
        TickLevel::Mid => (top, 4.0),
        TickLevel::Major => (top, 5.0),
    };
    let color = palette
        .border_strong
        .opacity(if level == TickLevel::Major { 0.9 } else { 0.5 });
    div()
        .absolute()
        .left(gpui_kit::relative(percent))
        .top(px(top))
        .w(px(1.))
        .h(px(height))
        .bg(color)
}

/// Vertical position of the fix-floor label, in track coordinates. Kept beside
/// the profile geometry so the label and the dashed rule never drift apart.
fn floor_label_top() -> f32 {
    const TOP: f32 = 8.0;
    let height = BASELINE_TOP - TOP;
    let floor_y = height - height * (0.08 + 0.84 * (4.0 / 12.0));
    TOP + floor_y - 6.0
}

/// The coverage ridge: one bar per displayed bucket, height = tracked-satellite
/// count on a fixed 0..=12 domain. Real counts only; a bucket with no channel
/// data keeps a gap, and a run with no coverage draws no ridge at all.
fn satellite_profile(profile: Option<&[Option<u8>]>, palette: Palette) -> Option<AnyElement> {
    let values = profile?.to_vec();
    if values.iter().all(Option::is_none) {
        return None;
    }
    let top = 8.0;
    let height = BASELINE_TOP - top;
    // Fixed domain so runs stay comparable: 0..=12 tracked satellites.
    let fill = palette.muted.opacity(0.20);
    let line = palette.muted.opacity(0.70);
    let floor = palette.danger.opacity(0.35);
    let domain = 12.0_f32;
    Some(
        canvas(
            move |_, _, _| {},
            move |bounds: Bounds<gpui_kit::Pixels>, _, window, _| {
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                if w <= 2.0 || h <= 2.0 {
                    return;
                }
                let n = values.len().max(2);
                let origin = bounds.origin;
                let mut area = PathBuilder::fill();
                let mut stroke = PathBuilder::stroke(px(1.0));
                let mut run: Vec<(f32, f32)> = Vec::new();
                let flush =
                    |run: &mut Vec<(f32, f32)>, area: &mut PathBuilder, line: &mut PathBuilder| {
                        if run.is_empty() {
                            return;
                        }
                        area.move_to(point(origin.x + px(run[0].0), origin.y + px(h)));
                        for (x, y) in run.iter() {
                            area.line_to(point(origin.x + px(*x), origin.y + px(*y)));
                        }
                        let last = run[run.len() - 1];
                        area.line_to(point(origin.x + px(last.0), origin.y + px(h)));
                        area.close();
                        line.move_to(point(origin.x + px(run[0].0), origin.y + px(run[0].1)));
                        for (x, y) in run.iter().skip(1) {
                            line.line_to(point(origin.x + px(*x), origin.y + px(*y)));
                        }
                        run.clear();
                    };
                for (index, value) in values.iter().enumerate() {
                    let Some(value) = value else {
                        flush(&mut run, &mut area, &mut stroke);
                        continue;
                    };
                    let x = (index as f32 / n as f32) * w;
                    // 8..92% of the field: zero satellites sits near the floor.
                    let ratio = (f32::from(*value) / domain).clamp(0.0, 1.0);
                    let y = h - h * (0.08 + 0.84 * ratio);
                    run.push((x, y));
                }
                flush(&mut run, &mut area, &mut stroke);
                if let Ok(path) = area.build() {
                    window.paint_path(path, fill);
                }
                if let Ok(path) = stroke.build() {
                    window.paint_path(path, line);
                }
                // Fix floor: a dashed rule at 4 satellites (the 3D-fix minimum)
                // so the line's height has something to be measured against.
                let floor_y = h - h * (0.08 + 0.84 * (4.0 / domain));
                let mut dash_x = 0.0;
                while dash_x < w {
                    window.paint_quad(quad(
                        Bounds {
                            origin: point(origin.x + px(dash_x), origin.y + px(floor_y)),
                            size: size(px(3.0), px(1.)),
                        },
                        0.0,
                        floor,
                        px(0.0),
                        floor,
                        Default::default(),
                    ));
                    dash_x += 7.0;
                }
            },
        )
        .absolute()
        .left_0()
        .right_0()
        .top(px(top))
        .h(px(height))
        .into_any_element(),
    )
}

/// The timeline band: a coverage ridge standing on a baseline, event notches
/// cut through it, the processing boundary where the ridge ends, and a playhead
/// with a ruler handle.
pub(crate) fn track(
    view: &TrackView,
    profile: Option<&[Option<u8>]>,
    seek: AnyElement,
    palette: Palette,
) -> AnyElement {
    let (start_ms, span_ms) = view.window_ms();
    let ladder = tick_ladder(span_ms);
    let stems = event_window(view);
    let processed = view.processed.clamp(0.0, 1.0);
    let show_boundary = (0.005..0.995).contains(&processed);

    div()
        .relative()
        .h(px(metrics::TIMELINE_TRACK_HEIGHT))
        .w_full()
        // 1. Coverage ridge.
        .children(satellite_profile(profile, palette))
        // 2. Baseline: the one line everything stands on.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(BASELINE_TOP))
                .h(px(1.))
                .bg(palette.plot_axis),
        )
        // 3. Ruler ticks sit just below the baseline, inside the track so they
        //    line up with the bars exactly.
        .children(
            ticks(ladder, start_ms, span_ms)
                .into_iter()
                .map(|(percent, level)| tick_element(level, percent, palette, BASELINE_TOP + 1.0)),
        )
        // 4. Event notches: a 1px rule through the body with the cap on top.
        .children(
            stems
                .into_iter()
                .enumerate()
                .map(|(index, (percent, kind))| {
                    div()
                        .id(SharedString::from(format!("timeline-stem-{index}")))
                        .absolute()
                        .left(gpui_kit::relative(percent))
                        .top(px(0.))
                        .w(px(1.))
                        .h(px(BASELINE_TOP))
                        .bg(kind.color(palette).opacity(0.92))
                        .child(
                            div()
                                .absolute()
                                .left(px(-(CAP_SIZE - 1.0) / 2.0))
                                .top_0()
                                .child(cap(kind, palette)),
                        )
                }),
        )
        // 5. Processing boundary: where the ridge stops.
        .children(show_boundary.then(|| {
            div()
                .absolute()
                .left(gpui_kit::relative(processed))
                .top_0()
                .h(px(BASELINE_TOP))
                .w(px(1.))
                .bg(palette.border_strong)
                .child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .left(px(5.))
                        .px(px(3.))
                        .bg(palette.surface)
                        .font_family(data_font_family())
                        .text_size(px(10.))
                        .text_color(palette.muted)
                        .whitespace_nowrap()
                        .child(format!("{:.0}%", processed * 100.0)),
                )
        }))
        // 6. Playhead: a 9px accent wash, the 1.5px cursor, and a triangle
        //    handle on the baseline.
        .child(
            div()
                .absolute()
                .left(gpui_kit::relative(view.cursor_percent))
                .ml(px(-4.5))
                .top_0()
                .h(px(BASELINE_TOP))
                .w(px(9.))
                .bg(palette.accent.opacity(0.10))
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .top_0()
                        .h(px(BASELINE_TOP))
                        .w(px(1.5))
                        .bg(palette.accent),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(0.5))
                        .top(px(BASELINE_TOP - 5.0))
                        .child(triangle(palette.accent, true)),
                ),
        )
        // 6b. Ruler labels live inside the band, under the baseline, so the
        //     seek layer below covers them too.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(BASELINE_TOP + 8.0))
                .h(px(RULER_LABELS_HEIGHT))
                .child(axis_labels(view, palette, RULER_LABELS_HEIGHT)),
        )
        // 6c. The fix floor is named: without a number the dashed rule is
        //     decoration. `4` is the 3D-fix minimum this line encodes.
        .child(
            div()
                .absolute()
                .left(px(3.))
                .top(px(floor_label_top()))
                .font_family(data_font_family())
                .text_size(px(9.))
                .text_color(palette.danger.opacity(0.75))
                .child("4"),
        )
        // 7. Transparent seek slider spanning body and ruler alike: dragging the
        //    scale is the primary gesture, so the whole band seeks.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top_0()
                .bottom_0()
                .cursor_ew_resize()
                .child(seek),
        )
        .into_any_element()
}

/// Legend: one cap glyph plus its category word per event kind, in the same
/// shapes the track draws.
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
                    .gap(px(5.))
                    .text_size(px(11.))
                    .text_color(palette.muted)
                    .child(cap(kind, palette))
                    .child(kind.category())
            }),
        )
        .into_any_element()
}

/// Ruler labels under the band: `00:02:00`-style times every major tick, with
/// the ends pinned to the track edges.
pub(crate) fn axis_labels(view: &TrackView, palette: Palette, height: f32) -> AnyElement {
    let (start_ms, span_ms) = view.window_ms();
    let ladder = tick_ladder(span_ms);
    div()
        .relative()
        .h(px(height))
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
                    .text_size(px(11.))
                    .text_color(palette.muted)
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
                        base.right(px(9.)).w(px(52.)).text_right().child(label.time)
                    }
                }
            }),
        )
        .into_any_element()
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
    fn profile_series_reports_the_bucket_minimum_and_keeps_gaps() {
        // A count profile reports the WORST moment inside each bucket, so a
        // short outage cannot hide between samples.
        let per_epoch = [Some(9), Some(7), None, None, Some(8), None];
        let series = profile_series(&per_epoch, 100, 600, 3).expect("real counts");
        assert_eq!(series.len(), 3);
        assert_eq!(series[0], Some(7), "bucket minimum, not the first sample");
        assert_eq!(
            series[1], None,
            "an epoch without tracked satellites is a gap"
        );
        assert_eq!(series[2], Some(8));
        assert!(profile_series(&[None, None], 100, 200, 2).is_none());
        assert!(profile_series(&[], 100, 200, 2).is_none());
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
