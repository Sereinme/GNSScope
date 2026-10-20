//! Presentation-only global status strip (design-refresh spec §3.11).
//!
//! The strip renders the fixed 28px three-segment contract:
//!
//! * left — the *global* active processing run ("活动解算任务 · 处理中 63%",
//!   or "无活动解算任务" when idle);
//! * middle — the *selected* run's current-epoch context (`GPS L1 C/A ·
//!   4.000 MHz · 跟踪 9 · 导航 7 · 解类型 3D Fix`) with 1px vertical rules;
//! * right — real system telemetry (`CPU · 磁盘 · 内存`), sampled from
//!   platform APIs through a cached sampler with a coarse refresh interval.
//!   When collection is unavailable the segment reads "不可用"; placeholder
//!   numbers are forbidden (spec §7, gap 4).
//!
//! Composition is a pure data layer (`compose_with_sampler`) so the segment
//! contract, separator/typography rules, and the telemetry fallback path are
//! unit-testable with an injected sampler. The application keeps supplying
//! the same `items`/`message` arguments at the existing call site (app.rs
//! ~L7228); presentation keys are re-mapped to the canonical Chinese labels
//! internally so no all-caps English keys leak into the strip (gap 10).
//!
//! The trailing app status message (real transient feedback such as
//! "源 IQ 文件已移动或删除") is preserved after the telemetry segment,
//! separated by a 1px vertical rule; it is app-owned data and dropping it
//! would hide real receiver/library feedback.

use crate::visual_system::{data_font_family, interface_font_family, metrics, Palette};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement, Div, Hsla};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};

// Spec §2.2 typography roles. `visual_system::typography` is not exported to
// `ui/`, so the strip keeps its own private constants (noted in issue 16).
const METADATA: f32 = 12.0;
const CAPTION: f32 = 11.0;
const MEASUREMENT: f32 = 12.0;

/// Coarse telemetry refresh cadence. Between refreshes `live_telemetry`
/// only reads an `Instant` and clones a cached sample, so per-frame cost is
/// trivial and no process is ever spawned (spec §3.11 / issue 16).
const TELEMETRY_REFRESH: Duration = Duration::from_secs(2);
/// CPU usage needs two readings; sysinfo documents this minimum window.
const CPU_WINDOW: Duration = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL;

/// One real telemetry reading. `None` fields render "不可用" individually;
/// `None` from the sampler itself renders a single "不可用" segment.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TelemetrySample {
    /// System-wide CPU usage in percent.
    pub(crate) cpu: Option<f32>,
    /// Used-space percent of the primary (largest non-removable) volume.
    pub(crate) disk: Option<f32>,
    /// System memory used percent.
    pub(crate) memory: Option<f32>,
}

/// Typography role of a strip span (spec §2.2: interface vs data font).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpanFont {
    Interface,
    Data,
}

/// One styled text run inside a segment.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Span {
    pub(crate) text: String,
    pub(crate) font: SpanFont,
    pub(crate) size: f32,
    pub(crate) color: Hsla,
}

impl Span {
    fn interface(text: impl Into<String>, size: f32, color: Hsla) -> Self {
        Self {
            text: text.into(),
            font: SpanFont::Interface,
            size,
            color,
        }
    }

    fn data(text: impl Into<String>, size: f32, color: Hsla) -> Self {
        Self {
            text: text.into(),
            font: SpanFont::Data,
            size,
            color,
        }
    }
}

/// One structural piece of a segment. `Item` pairs a caption label with a
/// data value at a 4px gap (mockup `.sb-item`); `Rule` is the 1px × 12px
/// vertical separator between middle-segment items (mockup `.vrule`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Seg {
    Text(Span),
    Item { label: Span, value: Span },
    Rule,
}

/// The resolved three-segment plan, before element construction.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StripPlan {
    pub(crate) left: Vec<Seg>,
    pub(crate) center: Vec<Seg>,
    pub(crate) right: Vec<Seg>,
    /// Trailing app-owned status message (None when blank).
    pub(crate) message: Option<String>,
}

type Item = (&'static str, String, Hsla);

/// Renders the strip at the existing app call site. Telemetry comes from the
/// cached live sampler; tests call [`compose_with_sampler`] with an injected
/// sampler instead. `compact` (narrow window) drops the telemetry segment
/// and the trailing message entirely — degrading whole segments beats
/// mid-glyph clipping them against each other.
pub(crate) fn render(
    palette: Palette,
    toggle: AnyElement,
    items: impl IntoIterator<Item = Item>,
    message: String,
    compact: bool,
) -> impl IntoElement {
    let plan = compose_with_sampler(
        items.into_iter().collect(),
        message,
        palette,
        compact,
        live_telemetry,
    );
    strip_shell(palette, toggle, plan)
}

/// Builds the plan by resolving `sampler` exactly once (injected in tests).
pub(crate) fn compose_with_sampler<F>(
    items: Vec<Item>,
    message: String,
    palette: Palette,
    compact: bool,
    sampler: F,
) -> StripPlan
where
    F: FnOnce() -> Option<TelemetrySample>,
{
    compose(items, message, palette, compact, sampler())
}

fn compose(
    items: Vec<Item>,
    message: String,
    palette: Palette,
    compact: bool,
    sample: Option<TelemetrySample>,
) -> StripPlan {
    let zh = items
        .first()
        .map(|(label, _, _)| has_cjk(label))
        .unwrap_or(true);

    let mut iter = items.into_iter();
    let first = iter.next();
    let left = left_segments(first, zh, palette);
    let center = center_segments(iter.collect(), zh, palette);
    // Narrow windows drop the telemetry segment and the trailing message
    // rather than letting them collide mid-glyph with the center segment.
    let right = if compact {
        Vec::new()
    } else {
        right_segments(sample, zh, palette)
    };
    let message = (!compact && !message.trim().is_empty()).then_some(message);

    StripPlan {
        left,
        center,
        right,
        message,
    }
}

fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// Left segment: canonical "活动解算任务 · <state> <pct>%" while a run is
/// active, "无活动解算任务" (text tertiary) when idle. The app passes the
/// state and percentage inside one value string ("处理中  63%" / "Idle").
fn left_segments(first: Option<Item>, zh: bool, palette: Palette) -> Vec<Seg> {
    let Some((label, value, supplied_color)) = first else {
        // No data at all: an em dash, never fabricated activity.
        return vec![Seg::Text(Span::interface("—", METADATA, palette.faint))];
    };
    match parse_active(&value) {
        Some((state, percent)) => {
            // CONTEXT.md canonical term overrides the app's shorter label in
            // Chinese; the English label is passed through unchanged.
            let label_text = if zh { "活动解算任务" } else { label };
            vec![
                Seg::Text(Span::interface(label_text, METADATA, palette.muted)),
                Seg::Text(Span::interface("·", METADATA, palette.faint)),
                Seg::Text(Span::interface(
                    state.clone(),
                    METADATA,
                    state_color(&state, supplied_color, palette),
                )),
                Seg::Text(Span::data(percent, MEASUREMENT, palette.accent)),
            ]
        }
        None => {
            let idle = if zh {
                "无活动解算任务"
            } else {
                "No active run"
            };
            vec![Seg::Text(Span::interface(idle, METADATA, palette.faint))]
        }
    }
}

/// Splits "处理中  63%" into ("处理中", "63%"). Values without a trailing
/// percent (e.g. "Idle") are treated as idle.
fn parse_active(value: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let (last, state) = parts.split_last()?;
    if state.is_empty() || !last.ends_with('%') {
        return None;
    }
    Some((state.join(" "), last.to_string()))
}

/// Spec §2.5: 处理中 owns the accent cyan; unknown states keep the color the
/// application supplied (the text itself is always the second cue).
fn state_color(state: &str, supplied: Hsla, palette: Palette) -> Hsla {
    match state {
        "处理中" | "Processing" | "Running" => palette.accent,
        _ => supplied,
    }
}

/// Middle segment: selected-run context. The application passes stable
/// machine keys (SIGNAL/RATE/TRACK/NAV/FIX); those keys are mapped to the
/// canonical Chinese labels and never rendered (gap 10: no all-caps English
/// keys). Missing values render "—", never 0 (spec §7.9).
fn center_segments(items: Vec<Item>, zh: bool, palette: Palette) -> Vec<Seg> {
    let mut segs: Vec<Seg> = Vec::new();
    for (key, value, color) in items {
        if !segs.is_empty() {
            segs.push(Seg::Rule);
        }
        let value = if value.trim().is_empty() {
            Span::interface("—", MEASUREMENT, palette.faint)
        } else {
            Span::data(value, MEASUREMENT, color)
        };
        let seg = match key {
            // Signal id and sample rate are self-describing values.
            "SIGNAL" | "RATE" => Seg::Text(value),
            "TRACK" | "NAV" | "FIX" => {
                let label = match key {
                    "TRACK" => {
                        if zh {
                            "跟踪"
                        } else {
                            "Tracking"
                        }
                    }
                    "NAV" => {
                        if zh {
                            "导航"
                        } else {
                            "Navigation"
                        }
                    }
                    _ => {
                        if zh {
                            "解类型"
                        } else {
                            "Solution"
                        }
                    }
                };
                Seg::Item {
                    label: Span::interface(label, CAPTION, palette.faint),
                    value,
                }
            }
            // Unknown key: show only the value so no raw key text leaks.
            _ => Seg::Text(value),
        };
        segs.push(seg);
    }
    segs
}

/// Right segment: real telemetry only. `None` (sampler unavailable) renders
/// a single "不可用"; inside a sample, each unavailable metric renders
/// "不可用" in its own value slot. Fabricated numbers are forbidden.
fn right_segments(sample: Option<TelemetrySample>, zh: bool, palette: Palette) -> Vec<Seg> {
    let unavailable = || {
        Seg::Text(Span::interface(
            if zh { "不可用" } else { "Unavailable" },
            METADATA,
            palette.faint,
        ))
    };
    let Some(sample) = sample else {
        return vec![unavailable()];
    };
    let metric = |value: Option<f32>| match value {
        Some(percent) => Span::data(format!("{percent:.0}%"), MEASUREMENT, palette.muted),
        None => Span::interface(
            if zh { "不可用" } else { "Unavailable" },
            MEASUREMENT,
            palette.faint,
        ),
    };
    let disk_label = if zh { "磁盘" } else { "Disk" };
    let memory_label = if zh { "内存" } else { "Memory" };
    let dot = || Seg::Text(Span::interface("·", CAPTION, palette.faint));
    vec![
        Seg::Item {
            label: Span::interface("CPU", CAPTION, palette.faint),
            value: metric(sample.cpu),
        },
        dot(),
        Seg::Item {
            label: Span::interface(disk_label, CAPTION, palette.faint),
            value: metric(sample.disk),
        },
        dot(),
        Seg::Item {
            label: Span::interface(memory_label, CAPTION, palette.faint),
            value: metric(sample.memory),
        },
    ]
}

// ---------------------------------------------------------------------------
// Live telemetry sampler (real values only; cached; coarse refresh)
// ---------------------------------------------------------------------------

struct TelemetryCache {
    system: System,
    disks: Disks,
    baseline_at: Instant,
    sample: Option<TelemetrySample>,
    sampled_at: Option<Instant>,
}

impl TelemetryCache {
    fn new() -> Self {
        let mut system = System::new();
        // Baseline reading: establishes the CPU usage window. The CPU value
        // itself only becomes available after a second reading (>= CPU_WINDOW).
        system.refresh_cpu_usage();
        system.refresh_memory();
        let disks = Disks::new_with_refreshed_list();
        Self {
            system,
            disks,
            baseline_at: Instant::now(),
            sample: None,
            sampled_at: None,
        }
    }

    fn poll(&mut self, now: Instant) -> Option<TelemetrySample> {
        let window_elapsed = now.duration_since(self.baseline_at) >= CPU_WINDOW;
        if let Some(at) = self.sampled_at {
            if now.duration_since(at) < TELEMETRY_REFRESH {
                // Cached reading: no sysinfo work happens on most frames.
                return self.sample.clone();
            }
        } else if !window_elapsed {
            // Memory and disk are point-in-time values and are real from the
            // first frame; the CPU percentage honestly reads "不可用" until a
            // real measurement window exists.
            return Some(self.point_sample(None));
        }

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.disks.refresh();
        let sample = self.point_sample(Some(self.system.global_cpu_usage()));
        self.sample = Some(sample.clone());
        self.sampled_at = Some(now);
        Some(sample)
    }

    fn point_sample(&self, cpu: Option<f32>) -> TelemetrySample {
        let memory = (self.system.total_memory() > 0)
            .then(|| 100.0 * self.system.used_memory() as f64 / self.system.total_memory() as f64);
        TelemetrySample {
            cpu,
            disk: primary_volume_usage(&self.disks),
            memory: memory.map(|p| p as f32),
        }
    }
}

/// Used-space percent of the primary storage volume: the largest
/// non-removable disk (removable media and empty lists yield `None` →
/// "不可用").
fn primary_volume_usage(disks: &Disks) -> Option<f32> {
    let disk = disks
        .list()
        .iter()
        .filter(|disk| !disk.is_removable())
        .max_by_key(|disk| disk.total_space())?;
    let total = disk.total_space();
    if total == 0 {
        return None;
    }
    let used = total.saturating_sub(disk.available_space());
    Some((100.0 * used as f64 / total as f64) as f32)
}

/// Cached live telemetry entry point. The first call builds the baseline
/// cache; subsequent calls take a mutex, read an `Instant`, and clone the
/// cached sample until the coarse refresh interval elapses. A poisoned lock
/// degrades honestly to `None` → "不可用".
fn live_telemetry() -> Option<TelemetrySample> {
    static CACHE: Mutex<Option<TelemetryCache>> = Mutex::new(None);
    let mut guard = CACHE.lock().ok()?;
    let cache = guard.get_or_insert_with(TelemetryCache::new);
    cache.poll(Instant::now())
}

// ---------------------------------------------------------------------------
// Element construction (presentation only; all decisions live above)
// ---------------------------------------------------------------------------

fn span_element(span: &Span) -> Div {
    let element = div()
        // Narrow windows shrink a flexible segment's spans instead of
        // hard-clipping them mid-glyph at the segment boundary.
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(span.size))
        .text_color(span.color);
    match span.font {
        SpanFont::Interface => element.font_family(interface_font_family()),
        SpanFont::Data => element.font_family(data_font_family()),
    }
    .child(span.text.clone())
}

fn seg_element(seg: &Seg, palette: Palette) -> Div {
    match seg {
        Seg::Text(span) => span_element(span),
        // Label/value pair: caption label + data value, 4px internal gap
        // (mockup `.sb-item`).
        Seg::Item { label, value } => div()
            .flex()
            .items_baseline()
            .gap(px(4.))
            .child(span_element(label))
            .child(span_element(value)),
        // 1px × 12px vertical rule between middle-segment items.
        Seg::Rule => div().w(px(1.)).h(px(12.)).bg(palette.border),
    }
}

/// Builds the 28px strip: toggle, left segment (flex), centered context
/// segment, right telemetry segment with the trailing app status message.
fn strip_shell(palette: Palette, toggle: AnyElement, plan: StripPlan) -> Div {
    div()
        .h(px(metrics::STATUS_BAR_HEIGHT))
        .flex_shrink_0()
        // A Carbon status band: one tone step up from the field, closed by a
        // hairline so the shell reads as header / body / footer.
        .bg(palette.surface_alt)
        .border_t_1()
        .border_color(palette.border)
        .flex()
        .items_center()
        .text_color(palette.text)
        .font_family(interface_font_family())
        .child(toggle)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(6.))
                .children(plan.left.into_iter().map(|seg| seg_element(&seg, palette))),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .children(
                    plan.center
                        .into_iter()
                        .map(|seg| seg_element(&seg, palette)),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .children(plan.right.into_iter().map(|seg| seg_element(&seg, palette)))
                .children(plan.message.map(|message| {
                    div()
                        .flex_shrink_0()
                        .max_w(px(360.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().w(px(1.)).h(px(12.)).bg(palette.border))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(CAPTION))
                                .text_color(palette.faint)
                                .child(message),
                        )
                })),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette::dark()
    }

    /// Flattens a segment to its rendered text (items as "label value").
    fn seg_text(seg: &Seg) -> String {
        match seg {
            Seg::Text(span) => span.text.clone(),
            Seg::Item { label, value } => format!("{} {}", label.text, value.text),
            Seg::Rule => "·".to_string(),
        }
    }

    fn plan_texts(segs: &[Seg]) -> Vec<String> {
        segs.iter().map(seg_text).collect()
    }

    fn active_items(palette: Palette) -> Vec<Item> {
        vec![
            ("活动运行", "处理中  63%".to_string(), palette.healthy),
            ("SIGNAL", "GPS L1 C/A".to_string(), palette.text),
            ("RATE", "4.000 MHz".to_string(), palette.text),
            ("TRACK", "9".to_string(), palette.text),
            ("NAV", "7".to_string(), palette.text),
            ("FIX", "3D Fix".to_string(), palette.healthy),
        ]
    }

    #[test]
    fn left_segment_follows_active_run_contract() {
        let palette = palette();
        let plan =
            compose_with_sampler(active_items(palette), String::new(), palette, false, || {
                None
            });
        assert_eq!(
            plan_texts(&plan.left),
            vec!["活动解算任务", "·", "处理中", "63%"]
        );
        let spans: Vec<&Span> = plan
            .left
            .iter()
            .filter_map(|seg| match seg {
                Seg::Text(span) => Some(span),
                _ => None,
            })
            .collect();
        // Label metadata 12px interface, text secondary.
        assert_eq!(spans[0].size, METADATA);
        assert_eq!(spans[0].font, SpanFont::Interface);
        assert_eq!(spans[0].color, palette.muted);
        // State word: accent cyan belongs to 处理中 (spec §2.5), overriding
        // the app-supplied healthy color.
        assert_eq!(spans[2].text, "处理中");
        assert_eq!(spans[2].color, palette.accent);
        assert_ne!(spans[2].color, palette.healthy);
        // Percent: data font 12px accent.
        assert_eq!(spans[3].text, "63%");
        assert_eq!(spans[3].font, SpanFont::Data);
        assert_eq!(spans[3].size, MEASUREMENT);
        assert_eq!(spans[3].color, palette.accent);
    }

    #[test]
    fn left_segment_idle_and_missing_data_states() {
        let palette = palette();
        // Idle (value without a percent) → canonical tertiary text, zh.
        let idle = compose_with_sampler(
            vec![("活动运行", "Idle".to_string(), palette.muted)],
            String::new(),
            palette,
            false,
            || None,
        );
        assert_eq!(plan_texts(&idle.left), vec!["无活动解算任务"]);
        match &idle.left[0] {
            Seg::Text(span) => {
                assert_eq!(span.color, palette.faint);
                assert_eq!(span.font, SpanFont::Interface);
            }
            other => panic!("unexpected idle segment {other:?}"),
        }
        // English locale (no CJK in the label) keeps the English label and
        // renders "No active run".
        let idle_en = compose_with_sampler(
            vec![("Active run", "Running".to_string(), palette.muted)],
            String::new(),
            palette,
            false,
            || None,
        );
        assert_eq!(plan_texts(&idle_en.left), vec!["No active run"]);
        // No items at all: em dash, never fabricated activity.
        let empty = compose_with_sampler(Vec::new(), String::new(), palette, false, || None);
        assert_eq!(plan_texts(&empty.left), vec!["—"]);
    }

    #[test]
    fn center_segment_maps_keys_to_canonical_labels_with_rules() {
        let palette = palette();
        let plan =
            compose_with_sampler(active_items(palette), String::new(), palette, false, || {
                None
            });
        let texts = plan_texts(&plan.center);
        assert_eq!(
            texts,
            vec![
                "GPS L1 C/A",
                "·",
                "4.000 MHz",
                "·",
                "跟踪 9",
                "·",
                "导航 7",
                "·",
                "解类型 3D Fix"
            ]
        );
        // No raw all-caps machine keys leak (gap 10).
        let joined = texts.join(" ");
        for key in ["TRACK", "NAV", "FIX", "SIGNAL", "RATE"] {
            assert!(!joined.contains(key), "raw key {key} leaked: {joined}");
        }
        // Rules are exactly the separator positions and are 1px dividers.
        for (i, seg) in plan.center.iter().enumerate() {
            assert_eq!(*seg == Seg::Rule, i % 2 == 1, "rule at index {i}");
        }
        // Label typography: caption 11px interface tertiary; values data 12px.
        let labeled: Vec<(&Span, &Span)> = plan
            .center
            .iter()
            .filter_map(|seg| match seg {
                Seg::Item { label, value } => Some((label, value)),
                _ => None,
            })
            .collect();
        assert_eq!(labeled.len(), 3);
        for (label, value) in labeled {
            assert_eq!(label.size, CAPTION);
            assert_eq!(label.font, SpanFont::Interface);
            assert_eq!(label.color, palette.faint);
            assert_eq!(value.size, MEASUREMENT);
            assert_eq!(value.font, SpanFont::Data);
        }
        // The healthy solution-type color is whatever the app supplied.
        match plan.center.last() {
            Some(Seg::Item { value, .. }) => assert_eq!(value.color, palette.healthy),
            other => panic!("unexpected final segment {other:?}"),
        }
    }

    #[test]
    fn center_segment_missing_values_render_dash_not_zero() {
        let palette = palette();
        let plan = compose_with_sampler(
            vec![
                ("活动运行", "处理中  63%".to_string(), palette.healthy),
                ("TRACK", String::new(), palette.text),
                ("SIGNAL", String::new(), palette.text),
            ],
            String::new(),
            palette,
            false,
            || None,
        );
        let texts = plan_texts(&plan.center);
        assert_eq!(texts, vec!["跟踪 —", "·", "—"]);
        assert!(texts.iter().all(|text| text != "0" && text != "0%"));
        // The dash is tertiary and never a fabricated numeric value.
        match &plan.center[0] {
            Seg::Item { label, value } => {
                assert_eq!(label.text, "跟踪");
                assert_eq!(value.text, "—");
                assert_eq!(value.color, palette.faint);
            }
            other => panic!("unexpected segment {other:?}"),
        }
        match &plan.center[2] {
            Seg::Text(span) => {
                assert_eq!(span.text, "—");
                assert_eq!(span.color, palette.faint);
            }
            other => panic!("unexpected segment {other:?}"),
        }
    }

    #[test]
    fn right_segment_reads_unavailable_when_sampler_unavailable() {
        let palette = palette();
        // Injected sampler returning None → a single "不可用" segment.
        let plan =
            compose_with_sampler(active_items(palette), String::new(), palette, false, || {
                None
            });
        assert_eq!(plan_texts(&plan.right), vec!["不可用"]);
        match &plan.right[0] {
            Seg::Text(span) => {
                assert_eq!(span.size, METADATA);
                assert_eq!(span.font, SpanFont::Interface);
                assert_eq!(span.color, palette.faint);
            }
            other => panic!("unexpected telemetry segment {other:?}"),
        }
        // English locale fallback.
        let plan_en = compose_with_sampler(
            vec![("Active run", "Running".to_string(), palette.healthy)],
            String::new(),
            palette,
            false,
            || None,
        );
        assert_eq!(plan_texts(&plan_en.right), vec!["Unavailable"]);
    }

    #[test]
    fn right_segment_reports_real_sample_values() {
        let palette = palette();
        let sample = TelemetrySample {
            cpu: Some(18.4),
            disk: Some(32.0),
            memory: Some(41.2),
        };
        let plan =
            compose_with_sampler(active_items(palette), String::new(), palette, false, || {
                Some(sample.clone())
            });
        assert_eq!(
            plan_texts(&plan.right),
            vec!["CPU 18%", "·", "磁盘 32%", "·", "内存 41%"]
        );
        // Values: data font 12px; labels: caption 11px tertiary.
        let items: Vec<(&Span, &Span)> = plan
            .right
            .iter()
            .filter_map(|seg| match seg {
                Seg::Item { label, value } => Some((label, value)),
                _ => None,
            })
            .collect();
        assert_eq!(items.len(), 3);
        for (label, value) in items {
            assert_eq!(label.size, CAPTION);
            assert_eq!(label.color, palette.faint);
            assert_eq!(value.font, SpanFont::Data);
            assert_eq!(value.size, MEASUREMENT);
            assert_eq!(value.color, palette.muted);
        }
        // Partially unavailable sample: the missing metric alone reads
        // "不可用"; the real ones keep their percentages.
        let partial = TelemetrySample {
            cpu: None,
            disk: Some(32.0),
            memory: Some(41.2),
        };
        let plan =
            compose_with_sampler(active_items(palette), String::new(), palette, false, || {
                Some(partial)
            });
        assert_eq!(
            plan_texts(&plan.right),
            vec!["CPU 不可用", "·", "磁盘 32%", "·", "内存 41%"]
        );
    }

    #[test]
    fn message_is_kept_only_when_non_blank() {
        let palette = palette();
        let with_message = compose_with_sampler(
            active_items(palette),
            "源 IQ 文件已移动或删除".to_string(),
            palette,
            false,
            || None,
        );
        assert_eq!(
            with_message.message.as_deref(),
            Some("源 IQ 文件已移动或删除")
        );
        let blank = compose_with_sampler(
            active_items(palette),
            "   ".to_string(),
            palette,
            false,
            || None,
        );
        assert_eq!(blank.message, None);
    }

    #[test]
    fn compact_drops_telemetry_and_message_instead_of_colliding() {
        // Narrow windows must degrade whole segments: at 800px the telemetry
        // and trailing message used to collide mid-glyph with the center
        // segment ("解类型 3D Fix63%"). Compact drops both entirely.
        let palette = palette();
        let sample = TelemetrySample {
            cpu: Some(18.4),
            disk: Some(32.0),
            memory: Some(41.2),
        };
        let wide = compose_with_sampler(
            active_items(palette),
            "确定性预览".to_string(),
            palette,
            false,
            || Some(sample.clone()),
        );
        assert!(!wide.right.is_empty());
        assert_eq!(wide.message.as_deref(), Some("确定性预览"));

        let compact = compose_with_sampler(
            active_items(palette),
            "确定性预览".to_string(),
            palette,
            true,
            || Some(sample),
        );
        assert!(compact.right.is_empty());
        assert_eq!(compact.message, None);
        // Left and center survive intact.
        assert_eq!(compact.left, wide.left);
        assert_eq!(compact.center, wide.center);
    }

    #[test]
    fn strip_shell_is_32px_band_closed_by_a_hairline() {
        let palette = palette();
        let plan =
            compose_with_sampler(active_items(palette), String::new(), palette, false, || {
                None
            });
        let toggle = div().into_any_element();
        let mut shell = strip_shell(palette, toggle, plan);
        let style = shell.style();
        match style.size.height {
            Some(gpui_kit::Length::Definite(gpui_kit::DefiniteLength::Absolute(
                gpui_kit::AbsoluteLength::Pixels(height),
            ))) => assert_eq!(height, px(metrics::STATUS_BAR_HEIGHT)),
            other => panic!("unexpected strip height {other:?}"),
        }
        assert_eq!(metrics::STATUS_BAR_HEIGHT, 32.0);
        // A Carbon footer band: one tone step up, closed by a top hairline.
        assert_eq!(
            style.border_widths.top,
            Some(gpui_kit::AbsoluteLength::Pixels(px(1.)))
        );
        assert_eq!(style.background, Some(palette.surface_alt.into()));
        assert!(style.box_shadow.as_ref().is_none_or(Vec::is_empty));
    }

    #[test]
    fn span_typography_follows_text_roles() {
        let palette = palette();
        let caption = Span::interface("跟踪", CAPTION, palette.faint);
        let mut element = span_element(&caption);
        let style = element.style();
        assert_eq!(
            style.text.font_size,
            Some(gpui_kit::AbsoluteLength::Pixels(px(CAPTION)))
        );
        assert_eq!(style.text.font_family, Some(interface_font_family().into()));
        assert_eq!(style.text.color, Some(palette.faint));

        let data = Span::data("63%", MEASUREMENT, palette.accent);
        let mut element = span_element(&data);
        let style = element.style();
        assert_eq!(
            style.text.font_size,
            Some(gpui_kit::AbsoluteLength::Pixels(px(MEASUREMENT)))
        );
        assert_eq!(style.text.font_family, Some(data_font_family().into()));
        assert_eq!(style.text.color, Some(palette.accent));
    }

    #[test]
    fn live_sampler_returns_real_memory_and_disk_values() {
        // Real sysinfo call: memory and disk are point-in-time and must be
        // available immediately; the CPU window may still be open.
        let sample = live_telemetry();
        let sample = sample.expect("live telemetry must produce a sample");
        let memory = sample.memory.expect("memory must be measurable");
        assert!(
            (0.0..=100.0).contains(&memory),
            "memory {memory} out of range"
        );
        if let Some(disk) = sample.disk {
            assert!((0.0..=100.0).contains(&disk), "disk {disk} out of range");
        }
        if let Some(cpu) = sample.cpu {
            assert!((0.0..=100.0).contains(&cpu), "cpu {cpu} out of range");
        }
    }
}
