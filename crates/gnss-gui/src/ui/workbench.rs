//! Workbench header metadata formatting (design-refresh spec §3.3).
//!
//! The metadata line is `开始 HH:MM:SS · 已用时 hh:mm:ss · 采样数 N ·
//! 中频 F Hz` (English: `Started/Elapsed/Samples/IF`). Every segment pairs a
//! localized label with a real value, or is omitted entirely — missing data
//! is never replaced with `0` (spec §7.7 data honesty).

/// Separator glyph between metadata segments (text_tertiary when rendered;
/// spacing comes from the flex gap in the header row).
pub(crate) const SEGMENT_SEPARATOR: &str = "·";

/// One labeled metadata segment (spec §3.3): the label renders in the
/// metadata (interface) font, the value in the data font — both 12px.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetadataSegment {
    pub(crate) label: &'static str,
    pub(crate) value: String,
}

impl MetadataSegment {
    /// Pair a localized label with its formatted value.
    pub(crate) fn new(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
        }
    }
}

/// Format a real sample count. Values at or above 1e8 use scientific
/// notation with two decimals (e.g. `3.84e9`); smaller values are plain
/// integers with thousands separators. Callers pair the value with the
/// localized 采样数 / Samples label.
pub(crate) fn format_sample_count(label: &'static str, samples: u64) -> MetadataSegment {
    let value = if samples == 0 {
        "0".to_string()
    } else if (samples as f64) >= 1e8 {
        format!("{:.2e}", samples as f64)
    } else {
        let digits = samples.to_string();
        let (head, tail) = digits.split_at(digits.len() % 3);
        let tail_groups = tail
            .as_bytes()
            .chunks(3)
            .map(|group| std::str::from_utf8(group).unwrap_or_default())
            .collect::<Vec<_>>();
        if tail_groups.is_empty() {
            digits
        } else if head.is_empty() {
            tail_groups.join(",")
        } else {
            format!("{head},{}", tail_groups.join(","))
        }
    };
    MetadataSegment::new(label, value)
}

/// Format an elapsed duration as `hh:mm:ss` from real processed samples and
/// the run's sample rate. `None` when the rate is not usable or no samples
/// have been processed — callers omit the segment rather than show `0`.
pub(crate) fn format_elapsed(
    label: &'static str,
    processed_samples: u64,
    sample_rate_hz: f64,
) -> Option<MetadataSegment> {
    if processed_samples == 0 || !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return None;
    }
    let total_seconds = (processed_samples as f64 / sample_rate_hz) as u64;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    Some(MetadataSegment::new(
        label,
        format!("{hours:02}:{minutes:02}:{seconds:02}"),
    ))
}

/// Format the intermediate frequency in Hz from the real configured value.
/// `None` when it is not usable.
pub(crate) fn format_intermediate_frequency(
    label: &'static str,
    if_hz: f64,
) -> Option<MetadataSegment> {
    if !if_hz.is_finite() {
        return None;
    }
    Some(MetadataSegment::new(label, format!("{if_hz:.0} Hz")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirror of the header line composition: `label value` per segment,
    /// joined with the separator glyph (spec §3.3 frozen format).
    fn join_for_test(segments: &[MetadataSegment]) -> String {
        segments
            .iter()
            .map(|segment| format!("{} {}", segment.label, segment.value))
            .collect::<Vec<_>>()
            .join(&format!(" {SEGMENT_SEPARATOR} "))
    }

    #[test]
    fn large_sample_counts_use_scientific_notation() {
        assert_eq!(format_sample_count("采样数", 3_840_000_000).value, "3.84e9");
        assert_eq!(format_sample_count("采样数", 100_000_000).value, "1.00e8");
    }

    #[test]
    fn small_sample_counts_stay_plain() {
        assert_eq!(
            format_sample_count("采样数", 12_345_678).value,
            "12,345,678"
        );
        assert_eq!(format_sample_count("采样数", 999_999).value, "999,999");
        assert_eq!(format_sample_count("采样数", 1_000).value, "1,000");
        assert_eq!(format_sample_count("采样数", 42).value, "42");
    }

    #[test]
    fn elapsed_comes_from_real_samples_and_rate() {
        // 3.84e9 samples at 4e6 Hz = 960 s = 00:16:00.
        assert_eq!(
            format_elapsed("已用时", 3_840_000_000, 4_000_000.0).map(|segment| segment.value),
            Some("00:16:00".to_string())
        );
        assert_eq!(
            format_elapsed("已用时", 3_661_000_000, 1_000_000.0).map(|segment| segment.value),
            Some("01:01:01".to_string())
        );
    }

    #[test]
    fn unusable_elapsed_is_none_never_zero() {
        // No processed samples, bad rate: omit the segment, never show 00:00:00.
        assert!(format_elapsed("已用时", 0, 4_000_000.0).is_none());
        assert!(format_elapsed("已用时", 1_000, 0.0).is_none());
        assert!(format_elapsed("已用时", 1_000, f64::NAN).is_none());
        assert!(format_elapsed("已用时", 1_000, f64::INFINITY).is_none());
    }

    #[test]
    fn intermediate_frequency_is_real_or_omitted() {
        assert_eq!(
            format_intermediate_frequency("中频", 4_130_400.0).map(|segment| segment.value),
            Some("4130400 Hz".to_string())
        );
        assert_eq!(
            format_intermediate_frequency("中频", 0.0).map(|segment| segment.value),
            Some("0 Hz".to_string())
        );
        assert!(format_intermediate_frequency("中频", f64::NAN).is_none());
    }

    #[test]
    fn frozen_line_pairs_localized_labels_with_real_values() {
        // Spec §3.3 frozen format: `开始 14:32:07 · 已用时 00:10:05 ·
        // 采样数 3.84e9 · 中频 0 Hz`. Labels are supplied by the localized
        // caller; the formatters only pair them with real values.
        let segments = vec![
            MetadataSegment::new("开始", "14:32:07"),
            format_elapsed("已用时", 2_420_000_000, 4_000_000.0).unwrap(),
            format_sample_count("采样数", 3_840_000_000),
            format_intermediate_frequency("中频", 0.0).unwrap(),
        ];
        assert_eq!(
            join_for_test(&segments),
            "开始 14:32:07 · 已用时 00:10:05 · 采样数 3.84e9 · 中频 0 Hz"
        );
        // The English interface uses the same value formatters with the
        // Started/Elapsed/Samples/IF labels.
        let english = vec![
            MetadataSegment::new("Started", "14:32:07"),
            format_sample_count("Samples", 3_840_000_000),
        ];
        assert_eq!(join_for_test(&english), "Started 14:32:07 · Samples 3.84e9");
    }

    #[test]
    fn missing_segments_are_omitted_not_zero_filled() {
        // No usable rate and NaN IF: only the segments carrying real values
        // appear — the line gets shorter, never zero-filled.
        let mut segments = vec![MetadataSegment::new("开始", "08:12:44")];
        segments.extend(format_elapsed("已用时", 0, 4_000_000.0));
        segments.push(format_sample_count("采样数", 3_840_000_000));
        segments.extend(format_intermediate_frequency("中频", f64::NAN));
        assert_eq!(join_for_test(&segments), "开始 08:12:44 · 采样数 3.84e9");
    }
}
