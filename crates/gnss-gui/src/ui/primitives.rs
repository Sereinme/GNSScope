//! Runs-dock presentation primitives (spec §2.7 progress strips, §3.2 run
//! rows): honest per-state strip semantics and filename middle-ellipsis.
//!
//! These helpers are pure so every run state can be unit tested without a
//! window. Cyan rationing (spec §2.5) is enforced here: accent appears only
//! on the processing fill, `排队` owns the amber track, terminal states use
//! grey/danger fills and never render a full bar (spec §2.7).

use crate::visual_system::Palette;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, Div, Hsla};

/// Strip geometry: 4px high with a 1px radius and square ends (§2.7; no
/// rounded caps, no pill progress).
pub(crate) const STRIP_HEIGHT: f32 = 4.0;
pub(crate) const STRIP_RADIUS: f32 = 1.0;
/// 失败/已取消/已中断 never render a full bar (§2.7 "满格条" prohibition).
const PARTIAL_CAP: f64 = 0.96;
/// Middle-ellipsis budgets: the head preserves roughly the first 10
/// characters plus `…` plus the tail segment that carries `RunNN` and the
/// `.iqrun` extension (§3.2).
pub(crate) const RUN_NAME_MAX_CHARS: usize = 34;
pub(crate) const RUN_NAME_COMPACT_MAX_CHARS: usize = 24;
const ELLIPSIS_HEAD_CHARS: usize = 10;
const ELLIPSIS: char = '…';

/// Presentation state of a run row's progress strip, derived from the
/// localized status text supplied by the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StripState {
    /// 处理中: accent fill on deterministic processed-sample progress.
    Processing,
    /// 已完成: healthy fill at full width.
    Completed,
    /// 失败: danger fill only up to the actual failure progress.
    Failed,
    /// 排队: attention/amber empty track, 0%.
    Queued,
    /// 已取消: unavailable grey fill to the cancelled progress.
    Cancelled,
    /// 已中断: unavailable grey fill to the interrupted progress.
    Interrupted,
    /// 源不可用: results stay browsable; healthy fill (mockup reference).
    SourceUnavailable,
}

impl StripState {
    /// Maps the status words emitted by `app.rs::runs_row_view` (zh + en)
    /// to a strip state. Unknown words fall back to `Processing`, or to
    /// `Interrupted` when the application flagged the row as attention.
    pub(crate) fn from_status(status: &str, attention: bool) -> Self {
        match status {
            "完成" | "已完成" | "Completed" => Self::Completed,
            "失败" | "Failed" => Self::Failed,
            "已取消" | "Cancelled" | "Canceled" => Self::Cancelled,
            "已中断" | "Interrupted" => Self::Interrupted,
            "排队" | "已排队" | "Queued" => Self::Queued,
            "源不可用" | "源文件不可用" | "Source unavailable" | "Source missing" => {
                Self::SourceUnavailable
            }
            _ if attention => Self::Interrupted,
            _ => Self::Processing,
        }
    }

    /// Fill fraction in `0.0..=1.0`. Completed is always full; queued is
    /// always empty; failed/cancelled/interrupted are clamped below a full
    /// bar even when the stored progress reached 100%.
    pub(crate) fn fill(self, progress: f64) -> f64 {
        let progress = progress.clamp(0.0, 1.0);
        match self {
            Self::Completed | Self::SourceUnavailable => 1.0,
            Self::Queued => 0.0,
            Self::Processing => progress,
            Self::Failed | Self::Cancelled | Self::Interrupted => {
                if progress >= 1.0 {
                    PARTIAL_CAP
                } else {
                    progress
                }
            }
        }
    }

    /// Queued rows render the amber track alone, with no fill child.
    pub(crate) fn shows_fill(self) -> bool {
        self != Self::Queued
    }

    /// Slot track color; 排队 tints `unavailable_wash` with 22% attention
    /// (mockup `color-mix(in srgb, var(--attention) 22%, var(--track))`).
    pub(crate) fn track(self, palette: Palette) -> Hsla {
        if self == Self::Queued {
            palette
                .unavailable_wash
                .blend(palette.attention.opacity(0.22))
        } else {
            palette.unavailable_wash
        }
    }

    /// Fill color for the state (§2.7). Never called for `Queued`.
    pub(crate) fn fill_color(self, palette: Palette) -> Hsla {
        match self {
            Self::Processing => palette.accent,
            Self::Completed | Self::SourceUnavailable => palette.healthy,
            Self::Failed => palette.danger,
            Self::Queued => palette.attention,
            Self::Cancelled | Self::Interrupted => palette.unavailable,
        }
    }

    /// Percentage text for line 1. Only 处理中 shows a percentage (mockup
    /// keeps the other rows' percent slot empty), so a failed run can never
    /// display "100%".
    pub(crate) fn percent_text(self, progress: f64) -> Option<String> {
        match self {
            Self::Processing => Some(format!("{:.0}%", progress.clamp(0.0, 1.0) * 100.0)),
            _ => None,
        }
    }
}

/// Builds the 4px progress strip: full-width `unavailable_wash` track with a
/// state-colored fill sized by [`StripState::fill`]. Geometry is stable for
/// every state; the track never changes size.
pub(crate) fn progress_strip(state: StripState, progress: f64, palette: Palette) -> Div {
    let fill = state.fill(progress);
    div()
        .relative()
        .w_full()
        .h(px(STRIP_HEIGHT))
        .rounded(px(STRIP_RADIUS))
        .bg(state.track(palette))
        .children((state.shows_fill() && fill > 0.0).then(|| {
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(gpui_kit::relative(fill as f32))
                .rounded(px(STRIP_RADIUS))
                .bg(state.fill_color(palette))
        }))
}

/// Middle-ellipsis truncation for run filenames: keeps the first ~10
/// characters, `…`, and the trailing segment so the `RunNN` identifier and
/// the `.iqrun` extension always survive. Character based (UTF-8 safe),
/// never panics, and never returns more than `max_chars` characters for a
/// truncated name.
pub(crate) fn middle_ellipsis(name: &str, max_chars: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max_chars {
        return name.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }

    // Split off the extension (from the final '.', when it is not the first
    // or last character).
    let dot = chars
        .iter()
        .rposition(|&c| c == '.')
        .filter(|&i| i > 0 && i + 1 < chars.len());
    let (base_end, ext_start) = match dot {
        Some(i) => (i, i),
        None => (chars.len(), chars.len()),
    };
    let ext: &[char] = &chars[ext_start..];

    // Tail = the segment after the final '_' (this carries RunNN when the
    // name has one) plus the extension.
    let base = &chars[..base_end];
    let seg = base.iter().rposition(|&c| c == '_').map(|i| i + 1);
    let mut tail: Vec<char> = match seg {
        Some(start) if start < base.len() => base[start..].to_vec(),
        _ => Vec::new(),
    };
    tail.extend_from_slice(ext);

    if tail.is_empty() {
        // No '_' and no extension: keep a short tail of the raw name so the
        // truncation still reads as a middle ellipsis.
        tail.extend_from_slice(&base[base.len().saturating_sub(8)..]);
    }

    // Head: up to 10 leading characters taken before the tail segment,
    // trimmed of trailing path separators.
    let head_limit = match seg {
        Some(0) => 0,
        Some(start) => start - 1, // exclude the '_' itself
        None => base.len(),
    };
    let mut head: Vec<char> = chars[..head_limit.min(ELLIPSIS_HEAD_CHARS)].to_vec();
    while matches!(head.last(), Some('_' | '-' | '.')) {
        head.pop();
    }

    // Shrink the head to fit the budget, then fall back to a hard truncate
    // when even the tail cannot fit.
    let budget = max_chars.saturating_sub(tail.len() + 1);
    if budget < 4 {
        let mut hard: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        hard.push(ELLIPSIS);
        return hard;
    }
    if head.len() > budget {
        head.truncate(budget);
        while matches!(head.last(), Some('_' | '-' | '.')) {
            head.pop();
        }
    }

    let mut out: String = head.into_iter().collect();
    out.push(ELLIPSIS);
    out.extend(tail);
    debug_assert!(out.chars().count() <= max_chars);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(state: StripState, progress: f64) -> f64 {
        state.fill(progress)
    }

    #[test]
    fn processing_fill_follows_deterministic_progress() {
        assert_eq!(strip(StripState::Processing, 0.63), 0.63);
        assert_eq!(strip(StripState::Processing, 0.0), 0.0);
        assert_eq!(strip(StripState::Processing, -0.5), 0.0);
        assert_eq!(strip(StripState::Processing, 1.7), 1.0);
    }

    #[test]
    fn completed_fill_is_always_healthy_full() {
        assert_eq!(strip(StripState::Completed, 0.4), 1.0);
        assert_eq!(strip(StripState::Completed, 1.0), 1.0);
        assert_eq!(
            StripState::Completed.fill_color(Palette::light()),
            Palette::light().healthy
        );
    }

    #[test]
    fn failed_fill_tracks_actual_progress_and_never_fills_the_bar() {
        assert_eq!(strip(StripState::Failed, 0.43), 0.43);
        for progress in [0.99, 1.0, 1.5] {
            let fill = strip(StripState::Failed, progress);
            assert!(fill < 1.0, "failed fill {fill} must never be full");
        }
        assert_eq!(
            StripState::Failed.fill_color(Palette::light()),
            Palette::light().danger
        );
    }

    #[test]
    fn queued_fill_is_empty_with_amber_tinted_track() {
        let palette = Palette::light();
        assert_eq!(strip(StripState::Queued, 0.9), 0.0);
        assert!(!StripState::Queued.shows_fill());
        let queued_track = StripState::Queued.track(palette);
        assert_ne!(queued_track, palette.unavailable_wash);
        assert_ne!(queued_track, palette.accent, "排队 must not share cyan");
        assert_eq!(
            queued_track,
            palette
                .unavailable_wash
                .blend(palette.attention.opacity(0.22))
        );
        // Processing uses the plain track so the fills compare cleanly.
        assert_eq!(
            StripState::Processing.track(palette),
            palette.unavailable_wash
        );
    }

    #[test]
    fn cancelled_and_interrupted_use_grey_and_never_fill_the_bar() {
        let palette = Palette::dark();
        assert_eq!(strip(StripState::Cancelled, 0.12), 0.12);
        assert_eq!(strip(StripState::Interrupted, 0.67), 0.67);
        for state in [StripState::Cancelled, StripState::Interrupted] {
            assert!(strip(state, 1.0) < 1.0, "{state:?} must not be full");
            assert_eq!(state.fill_color(palette), palette.unavailable);
            assert_eq!(state.track(palette), palette.unavailable_wash);
        }
    }

    #[test]
    fn source_unavailable_keeps_results_browsable_with_full_strip() {
        assert_eq!(strip(StripState::SourceUnavailable, 0.0), 1.0);
        assert_eq!(
            StripState::SourceUnavailable.fill_color(Palette::light()),
            Palette::light().healthy
        );
    }

    #[test]
    fn status_words_map_to_strip_states_in_both_languages() {
        use StripState::*;
        assert_eq!(StripState::from_status("处理中", false), Processing);
        assert_eq!(StripState::from_status("Processing", false), Processing);
        assert_eq!(StripState::from_status("完成", false), Completed);
        assert_eq!(StripState::from_status("已完成", false), Completed);
        assert_eq!(StripState::from_status("Completed", false), Completed);
        assert_eq!(StripState::from_status("失败", false), Failed);
        assert_eq!(StripState::from_status("Failed", false), Failed);
        assert_eq!(StripState::from_status("排队", false), Queued);
        assert_eq!(StripState::from_status("Queued", false), Queued);
        assert_eq!(StripState::from_status("已取消", false), Cancelled);
        assert_eq!(StripState::from_status("已中断", false), Interrupted);
        assert_eq!(
            StripState::from_status("源不可用", false),
            SourceUnavailable
        );
        assert_eq!(
            StripState::from_status("Source unavailable", false),
            SourceUnavailable
        );
        // Unknown wording with the attention flag degrades to grey, never
        // to an accent processing row.
        assert_eq!(StripState::from_status("…", true), Interrupted);
        assert_eq!(StripState::from_status("…", false), Processing);
    }

    #[test]
    fn percent_text_only_appears_while_processing() {
        assert_eq!(
            StripState::Processing.percent_text(0.634),
            Some("63%".to_string())
        );
        for state in [
            StripState::Completed,
            StripState::Failed,
            StripState::Queued,
            StripState::Cancelled,
            StripState::Interrupted,
            StripState::SourceUnavailable,
        ] {
            assert_eq!(state.percent_text(1.0), None, "{state:?} shows no pct");
        }
        // A failed run can never render "100%".
        assert_ne!(
            StripState::Failed.percent_text(1.0),
            Some("100%".to_string())
        );
    }

    #[test]
    fn progress_strip_geometry_is_stable_across_states() {
        let palette = Palette::light();
        for state in [
            StripState::Processing,
            StripState::Completed,
            StripState::Failed,
            StripState::Queued,
            StripState::Cancelled,
            StripState::Interrupted,
            StripState::SourceUnavailable,
        ] {
            let mut strip = progress_strip(state, 0.5, palette);
            let style = strip.style();
            match style.size.height {
                Some(gpui_kit::Length::Definite(gpui_kit::DefiniteLength::Absolute(
                    gpui_kit::AbsoluteLength::Pixels(height),
                ))) => assert_eq!(height, px(STRIP_HEIGHT)),
                other => panic!("unexpected strip height {other:?}"),
            }
            // No drop shadows and no pill caps (§7).
            assert!(style.box_shadow.as_ref().is_none_or(Vec::is_empty));
        }
    }

    #[test]
    fn middle_ellipsis_preserves_extension_and_run_identifier() {
        let cases = [
            (
                "2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN_4Msps.iqrun",
                RUN_NAME_MAX_CHARS,
                "2013_04_04…4Msps.iqrun",
            ),
            (
                "2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN_4Msps_Run09.iqrun",
                RUN_NAME_MAX_CHARS,
                "2013_04_04…Run09.iqrun",
            ),
            // Fits the wide budget unchanged; the compact queue budget
            // truncates it exactly like the mockup.
            (
                "LabSat_int8_10p23Msps_Run04.iqrun",
                RUN_NAME_COMPACT_MAX_CHARS,
                "LabSat_int…Run04.iqrun",
            ),
        ];
        for (input, budget, expected) in cases {
            assert_eq!(middle_ellipsis(input, budget), expected);
        }
        assert_eq!(
            middle_ellipsis("LabSat_int8_10p23Msps_Run04.iqrun", RUN_NAME_MAX_CHARS),
            "LabSat_int8_10p23Msps_Run04.iqrun"
        );
    }

    #[test]
    fn middle_ellipsis_keeps_short_names_unchanged() {
        for name in ["Run09.iqrun", "short.iqrun", ""] {
            assert_eq!(middle_ellipsis(name, RUN_NAME_MAX_CHARS), name);
        }
        // Queue rows use the compact budget.
        let long = "2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN_4Msps_Run09.iqrun";
        let compact = middle_ellipsis(long, RUN_NAME_COMPACT_MAX_CHARS);
        assert!(compact.ends_with("Run09.iqrun"));
        assert!(compact.chars().count() <= RUN_NAME_COMPACT_MAX_CHARS);
    }

    #[test]
    fn middle_ellipsis_is_utf8_safe_and_bounded() {
        let wide = "信号文件_测试数据集_很长很长很长的采集记录.iqrun";
        let out = middle_ellipsis(wide, RUN_NAME_COMPACT_MAX_CHARS);
        assert!(out.chars().count() <= RUN_NAME_COMPACT_MAX_CHARS);
        assert!(out.contains(ELLIPSIS));
        assert!(out.ends_with(".iqrun"));

        // Pathological inputs never panic and stay within budget.
        for (name, budget) in [
            ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 34usize),
            ("x.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 8),
            ("_Run09.iqrun_extra_suffix_without_any_break", 6),
            ("名字", 4),
        ] {
            let out = middle_ellipsis(name, budget);
            assert!(
                out.chars().count() <= budget,
                "{name} -> {out} exceeds {budget}"
            );
        }
        assert_eq!(middle_ellipsis("abcdef", 0), "");
    }
}
