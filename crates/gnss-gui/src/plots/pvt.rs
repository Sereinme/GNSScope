//! PVT summary table for the final selected solution (design-refresh spec §3.8).
//!
//! This module owns the five-column contract (参数 / 值 / σ (1σ) / 单位 /
//! 状态 — frozen 146/170/72/40/46 at the 474 px reference table width,
//! narrowing proportionally to 388 px of column minima on smaller panes and
//! scrolling horizontally only below that), the language-frozen 15-row key
//! list in exact order, full-precision value formatting (never truncated), the
//! honest "—" presentation for unavailable sigma/value cells (never 0), and the
//! quiet bare-text status mapping (有效 / 不可用 / 无效). It renders display-only
//! data supplied by `app.rs`; state ownership stays in the application.
//!
//! Preview mode (`GNSSCOPE_PREVIEW=1`) tells the frozen §3.0 story on the two
//! story-bearing rows (使用/跟踪卫星数 = `7 / 9`, DOP = `3.12 / 2.41 / 2.08`);
//! every other row keeps showing the deterministic preview fixture values.

use crate::visual_system::{data_font_family, interface_font_family, metrics, Palette};
use crate::GuiPvtViewModel;
use gnss_core::pvt::{PvtEpochSolutionKind, UtcDateTime};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement};

// ---------------------------------------------------------------------------
// Adaptive column geometry (spec §3.8)
// ---------------------------------------------------------------------------

/// Frozen reference widths: 参数 146 / 值 170 / σ 72 / 单位 40 / 状态 46 at
/// the 474 px reference table width (mockup `viewBox 474`).
pub(crate) const TABLE_WIDTH: f32 = 474.0;
/// Column minima sized to the real content (longest key/value strings never
/// truncate): 参数 130 / 值 140 / σ 48 / 单位 30 / 状态 40 = 388 px.
pub(crate) const COL_MIN: [f32; 5] = [130.0, 140.0, 48.0, 30.0, 40.0];
/// Surplus distribution: below 474 px the columns narrow proportionally
/// toward their minima; the grow shares land exactly on the frozen widths at
/// 474 px (16/30/24/10/6 of the 86 px surplus).
pub(crate) const COL_GROW: [f32; 5] = [16.0, 30.0, 24.0, 10.0, 6.0];
/// Smallest table width; below this the pane scrolls horizontally instead of
/// clipping a column (spec §3.8: values are never truncated).
pub(crate) const TABLE_MIN_WIDTH: f32 = 388.0;

/// One table column: basis = minimum, grows into its frozen width share.
fn column_div(index: usize) -> gpui_kit::Div {
    div()
        .flex_basis(px(COL_MIN[index]))
        .flex_grow(COL_GROW[index])
        .flex_shrink_0()
        .min_w(px(COL_MIN[index]))
        .overflow_hidden()
        .whitespace_nowrap()
}

/// Header captions, Chinese, never all-caps English (gap 10).
pub(crate) const HEADER_LABELS: [&str; 5] = ["参数", "值", "σ (1σ)", "单位", "状态"];

/// Placeholder for an unavailable value or sigma. Never substitute 0.
const UNAVAILABLE: &str = "—";

/// Frozen row contract (spec §3.8): key + unit, exact names and order.
const ROW_DEFS: [(&str, Option<&str>); 15] = [
    ("时间 (GPS)", None),
    ("Week / SOW", None),
    ("位置 X (ECEF)", Some("m")),
    ("位置 Y (ECEF)", Some("m")),
    ("位置 Z (ECEF)", Some("m")),
    ("纬度", Some("°")),
    ("经度", Some("°")),
    ("高度", Some("m")),
    ("速度 E", Some("m/s")),
    ("速度 N", Some("m/s")),
    ("速度 U", Some("m/s")),
    ("3D 速度", Some("m/s")),
    ("解类型", None),
    ("使用/跟踪卫星数", Some("颗")),
    ("PDOP / HDOP / VDOP", None),
];

/// §3.0 story values applied only in deterministic preview mode.
const PREVIEW_USED_TRACKED: &str = "7 / 9";
const PREVIEW_DOP: &str = "3.12 / 2.41 / 2.08";

// ---------------------------------------------------------------------------
// Table model (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Quiet status cell: bare text, never a filled chip (spec §3.8 / §7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowStatus {
    Healthy,
    Unavailable,
    Invalid,
}

impl RowStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Healthy => "有效",
            Self::Unavailable => "不可用",
            Self::Invalid => "无效",
        }
    }

    fn color(self, palette: Palette) -> gpui_kit::Hsla {
        match self {
            Self::Healthy => palette.healthy,
            Self::Unavailable => palette.unavailable,
            Self::Invalid => palette.danger,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PvtTableRow {
    pub(crate) key: &'static str,
    pub(crate) value: String,
    /// `None` renders as "—" in text_tertiary; a reported 0 is never shown
    /// for an unavailable sigma (spec §3.8: never substitute 0).
    pub(crate) sigma: Option<String>,
    pub(crate) unit: Option<&'static str>,
    pub(crate) status: RowStatus,
}

impl PvtTableRow {
    pub(crate) fn sigma_display(&self) -> &str {
        self.sigma.as_deref().unwrap_or(UNAVAILABLE)
    }
}

/// Display input for the selected-epoch PVT summary.
pub(crate) struct SummaryInput<'a> {
    /// Real selected-epoch PVT output (`current_pvt()`); `None` = no solution.
    pub(crate) pvt: Option<&'a GuiPvtViewModel>,
    /// Whether the position passed `pvt_position_is_displayable` (computed by
    /// `app.rs`, which owns that quality gate).
    pub(crate) position_displayable: bool,
    /// `GNSSCOPE_PREVIEW=1` deterministic fixtures tell the §3.0 story.
    pub(crate) preview: bool,
}

/// Process-global preview flag, mirroring `GnsscopeApp::new`.
pub(crate) fn preview_mode() -> bool {
    std::env::var("GNSSCOPE_PREVIEW")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

/// A physical one-sigma must be finite and positive; covariance defaults of
/// 0.0 mean "not computed" and stay unavailable (never 0).
fn sigma_value(value: Option<f64>) -> Option<String> {
    finite(value.unwrap_or(f64::NAN))
        .filter(|value| *value > 0.0)
        .map(|value| signed_fixed(value, 3))
}

/// Full-precision fixed-point rendering; the ASCII minus is drawn as U+2212
/// to match the mockup's typographic convention.
fn signed_fixed(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value);
    match text.strip_prefix('-') {
        Some(rest) => format!("−{rest}"),
        None => text,
    }
}

fn number(value: f64, decimals: usize) -> String {
    finite(value).map_or_else(
        || UNAVAILABLE.to_string(),
        |value| signed_fixed(value, decimals),
    )
}

fn format_gps_datetime(utc: &UtcDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        utc.year,
        utc.month,
        utc.day,
        utc.hour,
        utc.minute,
        utc.second.trunc() as u32
    )
}

fn dop_cell(value: Option<f64>) -> String {
    value.map_or_else(|| UNAVAILABLE.to_string(), |value| format!("{value:.2}"))
}

/// Build the frozen 15-row model for the selected epoch.
pub(crate) fn rows(input: SummaryInput<'_>) -> Vec<PvtTableRow> {
    let Some(pvt) = input.pvt else {
        // No selected-epoch solution: every row is honestly unavailable.
        return ROW_DEFS
            .into_iter()
            .map(|(key, unit)| PvtTableRow {
                key,
                value: UNAVAILABLE.to_string(),
                sigma: None,
                unit,
                status: RowStatus::Unavailable,
            })
            .collect();
    };

    let position_status = if input.position_displayable {
        RowStatus::Healthy
    } else {
        RowStatus::Invalid
    };
    let week = pvt.time.gps_week.map(|week| week.to_string());
    let sow = pvt
        .time
        .receiver_time_sow
        .and_then(finite)
        .map(|sow| format!("{sow:.3}"));
    let time = pvt.time.utc.as_ref().map(format_gps_datetime);
    let velocity = pvt.velocity.as_ref();
    let used = pvt.quality.used_prns.len();
    let tracked = if pvt.satellites.is_empty() {
        used
    } else {
        pvt.satellites.len()
    }
    .max(used);
    let hdop = finite(pvt.quality.hdop).filter(|value| *value > 0.0);
    // PDOP/VDOP are now carried by `GuiPvtQuality` (issue 19) and persisted
    // through the run-library seam; pre-existing stored runs map them to
    // `None`, which renders as "—" per component — e.g. `3.12 / 2.41 / —`
    // — never fabricated (spec §3.8 / §7).
    let pdop = pvt
        .quality
        .pdop
        .and_then(finite)
        .filter(|value| *value > 0.0);
    let vdop = pvt
        .quality
        .vdop
        .and_then(finite)
        .filter(|value| *value > 0.0);

    let mut built = vec![
        PvtTableRow {
            key: ROW_DEFS[0].0,
            value: time.clone().unwrap_or_else(|| UNAVAILABLE.to_string()),
            sigma: None,
            unit: ROW_DEFS[0].1,
            status: if time.is_some() {
                RowStatus::Healthy
            } else {
                RowStatus::Unavailable
            },
        },
        PvtTableRow {
            key: ROW_DEFS[1].0,
            value: format!(
                "{} / {}",
                week.as_deref().unwrap_or(UNAVAILABLE),
                sow.as_deref().unwrap_or(UNAVAILABLE)
            ),
            sigma: None,
            unit: ROW_DEFS[1].1,
            status: if week.is_some() && sow.is_some() {
                RowStatus::Healthy
            } else {
                RowStatus::Unavailable
            },
        },
    ];
    for (offset, component) in pvt.ecef_m.iter().enumerate() {
        built.push(PvtTableRow {
            key: ROW_DEFS[2 + offset].0,
            value: number(*component, 3),
            sigma: None,
            unit: ROW_DEFS[2 + offset].1,
            status: position_status,
        });
    }
    built.extend([
        PvtTableRow {
            key: ROW_DEFS[5].0,
            value: number(pvt.latitude_deg, 7),
            sigma: None,
            unit: ROW_DEFS[5].1,
            status: position_status,
        },
        PvtTableRow {
            key: ROW_DEFS[6].0,
            value: number(pvt.longitude_deg, 7),
            sigma: None,
            unit: ROW_DEFS[6].1,
            status: position_status,
        },
        PvtTableRow {
            key: ROW_DEFS[7].0,
            value: number(pvt.altitude_m, 3),
            sigma: sigma_value(Some(pvt.quality.vertical_sigma_m)),
            unit: ROW_DEFS[7].1,
            status: position_status,
        },
    ]);
    for (offset, component) in [
        velocity.map(|velocity| velocity.east_mps),
        velocity.map(|velocity| velocity.north_mps),
        velocity.map(|velocity| velocity.up_mps),
    ]
    .into_iter()
    .enumerate()
    {
        let (value, status) = match component.and_then(finite) {
            Some(value) => (signed_fixed(value, 4), RowStatus::Healthy),
            None if velocity.is_some() => (UNAVAILABLE.to_string(), RowStatus::Invalid),
            None => (UNAVAILABLE.to_string(), RowStatus::Unavailable),
        };
        built.push(PvtTableRow {
            key: ROW_DEFS[8 + offset].0,
            value,
            sigma: None,
            unit: ROW_DEFS[8 + offset].1,
            status,
        });
    }
    let (speed, speed_status) = match velocity.and_then(|velocity| finite(velocity.speed_mps)) {
        Some(value) => (signed_fixed(value, 3), RowStatus::Healthy),
        None if velocity.is_some() => (UNAVAILABLE.to_string(), RowStatus::Invalid),
        None => (UNAVAILABLE.to_string(), RowStatus::Unavailable),
    };
    built.push(PvtTableRow {
        key: ROW_DEFS[11].0,
        value: speed,
        sigma: sigma_value(pvt.quality.velocity_sigma_mps),
        unit: ROW_DEFS[11].1,
        status: speed_status,
    });
    // 解类型 shows the real positioning fix type derived by the view model
    // (issue 17 / §3.8); the candidate kind Debug string (Raw/Smoothed/
    // Kalman) is never presented as a fix type. When no valid fix exists the
    // row honestly reads "—".
    let solution_kind = pvt.solution_kind;
    let fix_type = pvt.fix_type();
    built.push(PvtTableRow {
        key: ROW_DEFS[12].0,
        value: fix_type.map_or_else(|| UNAVAILABLE.to_string(), |fix| fix.label().to_string()),
        sigma: None,
        unit: ROW_DEFS[12].1,
        status: match fix_type {
            Some(_) => RowStatus::Healthy,
            None if solution_kind == PvtEpochSolutionKind::Unconverged
                || pvt.quality.n_sats == 0 =>
            {
                RowStatus::Invalid
            }
            None => RowStatus::Unavailable,
        },
    });
    built.push(PvtTableRow {
        key: ROW_DEFS[13].0,
        value: format!("{used} / {tracked}"),
        sigma: None,
        unit: ROW_DEFS[13].1,
        status: if used > 0 {
            RowStatus::Healthy
        } else {
            RowStatus::Invalid
        },
    });
    let dop_values = [pdop, hdop, vdop];
    let dop_status = if dop_values.iter().any(Option::is_some) {
        RowStatus::Healthy
    } else {
        RowStatus::Unavailable
    };
    let dop_value = if dop_values.iter().any(Option::is_some) {
        dop_values
            .iter()
            .map(|value| dop_cell(*value))
            .collect::<Vec<_>>()
            .join(" / ")
    } else {
        UNAVAILABLE.to_string()
    };
    built.push(PvtTableRow {
        key: ROW_DEFS[14].0,
        value: dop_value,
        sigma: None,
        unit: ROW_DEFS[14].1,
        status: dop_status,
    });

    debug_assert_eq!(built.len(), ROW_DEFS.len());
    if input.preview {
        // §3.0 story rows only; other rows keep the fixture values.
        if let Some(row) = built.get_mut(13) {
            row.value = PREVIEW_USED_TRACKED.to_string();
            row.status = RowStatus::Healthy;
        }
        if let Some(row) = built.get_mut(14) {
            row.value = PREVIEW_DOP.to_string();
            row.status = RowStatus::Healthy;
        }
    }
    built
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn header_cell(index: usize, label: &'static str) -> impl IntoElement {
    let cell = column_div(index);
    // Headers follow their column's alignment: the key column is left-aligned
    // and inset like the rows; the value and sigma headers right-align over
    // right-aligned numbers.
    match index {
        0 => cell.px(px(12.)).child(label),
        1 => cell.pr(px(8.)).text_right().child(label),
        2 => cell.pr(px(12.)).text_right().child(label),
        3 => cell.pr(px(6.)).child(label),
        _ => cell.child(label),
    }
}

fn header_row(palette: Palette) -> impl IntoElement {
    div()
        .h(px(metrics::PVT_HEADER_ROW_HEIGHT))
        .flex()
        .items_center()
        .bg(palette.surface_alt)
        .border_b_1()
        .border_color(palette.border)
        .text_size(px(11.))
        .font_family(interface_font_family())
        .text_color(palette.muted)
        .children(
            HEADER_LABELS
                .into_iter()
                .enumerate()
                .map(|(index, label)| header_cell(index, label).into_any_element()),
        )
}

fn row_element(row: &PvtTableRow, is_last: bool, palette: Palette) -> AnyElement {
    div()
        .h(px(metrics::PVT_ROW_HEIGHT))
        .flex()
        .items_center()
        // The mockup draws a hairline under every row except the last one.
        .when(!is_last, |this| {
            this.border_b_1().border_color(palette.border)
        })
        .child(
            column_div(0)
                .px(px(12.))
                .text_size(px(11.))
                .font_family(interface_font_family())
                .text_color(palette.muted)
                .child(row.key),
        )
        .child(
            // Measurements right-align on the decimal side and end with the
            // unit column, so the eye can compare magnitudes down the stack.
            column_div(1)
                .text_right()
                .pr(px(8.))
                .text_size(px(12.))
                .font_family(data_font_family())
                .text_color(palette.text)
                .child(row.value.clone()),
        )
        .child(
            column_div(2)
                .text_right()
                .pr(px(12.))
                .text_size(px(12.))
                .font_family(data_font_family())
                .text_color(if row.sigma.is_some() {
                    palette.muted
                } else {
                    palette.faint
                })
                .child(row.sigma_display().to_string()),
        )
        .child(
            column_div(3)
                .pr(px(6.))
                .text_size(px(11.))
                .font_family(interface_font_family())
                .text_color(palette.faint)
                .child(row.unit.unwrap_or(UNAVAILABLE)),
        )
        .child(
            column_div(4)
                .text_size(px(11.))
                .font_family(interface_font_family())
                .text_color(row.status.color(palette))
                .child(row.status.label()),
        )
        .into_any_element()
}

/// The table fills the pane well up to the 474 px reference width, narrows
/// proportionally down to the 388 px column minima, and scrolls
/// horizontally only below that — the 状态 column is never clipped
/// unreachable behind the pane edge (spec §3.8 precision rules).
pub(crate) fn summary_table(input: SummaryInput<'_>, palette: Palette) -> AnyElement {
    let rows = rows(input);
    div()
        .w_full()
        .overflow_x_scrollbar()
        .child(
            div()
                .w_full()
                .min_w(px(TABLE_MIN_WIDTH))
                .max_w(px(TABLE_WIDTH))
                .flex()
                .flex_col()
                .child(header_row(palette))
                .children(
                    rows.iter()
                        .enumerate()
                        .map(|(index, row)| row_element(row, index + 1 == ROW_DEFS.len(), palette)),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_pvt_view_model;
    use crate::GuiSatelliteGeometry;

    const FROZEN_KEYS: [&str; 15] = [
        "时间 (GPS)",
        "Week / SOW",
        "位置 X (ECEF)",
        "位置 Y (ECEF)",
        "位置 Z (ECEF)",
        "纬度",
        "经度",
        "高度",
        "速度 E",
        "速度 N",
        "速度 U",
        "3D 速度",
        "解类型",
        "使用/跟踪卫星数",
        "PDOP / HDOP / VDOP",
    ];

    fn sample_input(preview: bool) -> SummaryInput<'static> {
        // `sample_pvt_view_model` returns an owned value, so tests build the
        // fixture inside a leaked box to keep the input borrow simple.
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new(sample_pvt_view_model()));
        SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview,
        }
    }

    fn build(input: SummaryInput<'_>) -> Vec<PvtTableRow> {
        rows(input)
    }

    fn row_by_key<'a>(rows: &'a [PvtTableRow], key: &str) -> &'a PvtTableRow {
        rows.iter()
            .find(|row| row.key == key)
            .unwrap_or_else(|| panic!("row {key} must exist"))
    }

    #[test]
    fn frozen_row_keys_and_order_match_the_spec() {
        let rows = build(sample_input(false));
        assert_eq!(rows.len(), 15);
        let keys = rows.iter().map(|row| row.key).collect::<Vec<_>>();
        assert_eq!(keys, FROZEN_KEYS, "frozen key list and order");
        // The row definitions are the single production source of the list.
        let def_keys = ROW_DEFS.iter().map(|(key, _)| *key).collect::<Vec<_>>();
        assert_eq!(def_keys, FROZEN_KEYS);
    }

    #[test]
    fn column_model_reproduces_frozen_widths_at_the_reference_table() {
        assert_eq!(HEADER_LABELS, ["参数", "值", "σ (1σ)", "单位", "状态"]);
        // 18.5/20 (down from 20/22) keeps all fifteen rows visible inside the
        // 30px pane head after the floating chassis took 8px of gutters.
        assert_eq!(metrics::PVT_ROW_HEIGHT, 18.5);
        assert_eq!(metrics::PVT_HEADER_ROW_HEIGHT, 20.0);
        assert_eq!(COL_MIN.iter().sum::<f32>(), TABLE_MIN_WIDTH);
        let surplus = TABLE_WIDTH - TABLE_MIN_WIDTH;
        assert_eq!(COL_GROW.iter().sum::<f32>(), surplus);
        // Each column's min + its grow share of the 86px surplus lands
        // exactly on the frozen 146/170/72/40/46 at the 474px reference.
        let frozen = [146.0, 170.0, 72.0, 40.0, 46.0];
        for (index, frozen_width) in frozen.into_iter().enumerate() {
            let width = COL_MIN[index] + surplus * COL_GROW[index] / surplus;
            assert_eq!(width, frozen_width, "column {index} at reference width");
        }
    }

    #[test]
    fn values_render_full_precision_without_truncation() {
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new({
            let mut pvt = sample_pvt_view_model();
            pvt.latitude_deg = 41.276_019_2;
            pvt.longitude_deg = 2.170_043_1;
            pvt.altitude_m = 137.842;
            pvt.ecef_m = [-2_702_668.142_4, -4_292_642.816_4, 3_869_482.517_4];
            pvt
        }));
        let rows = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview: false,
        });
        // Seven-decimal latitude, never truncated or concatenated.
        assert_eq!(row_by_key(&rows, "纬度").value, "41.2760192");
        assert_eq!(row_by_key(&rows, "经度").value, "2.1700431");
        // Three-decimal ECEF with the U+2212 minus.
        assert_eq!(row_by_key(&rows, "位置 X (ECEF)").value, "−2702668.142");
        assert_eq!(row_by_key(&rows, "位置 Y (ECEF)").value, "−4292642.816");
        assert_eq!(row_by_key(&rows, "位置 Z (ECEF)").value, "3869482.517");
        assert_eq!(row_by_key(&rows, "高度").value, "137.842");
        // Week / SOW is a full pair, never a "W17"-style fragment.
        assert_eq!(row_by_key(&rows, "Week / SOW").value, "1734 / 368658.000");
        for row in &rows {
            assert!(
                !row.value.contains('…') && !row.value.ends_with('.'),
                "{} must not be truncated: {}",
                row.key,
                row.value
            );
        }
    }

    #[test]
    fn unavailable_sigma_is_dash_and_never_zero() {
        let rows = build(sample_input(false));
        // Sample fixture exposes vertical sigma 28 m and velocity sigma 0.10 m/s.
        assert_eq!(row_by_key(&rows, "高度").sigma_display(), "28.000");
        assert_eq!(row_by_key(&rows, "3D 速度").sigma_display(), "0.100");
        // Rows without a matching measured sigma show "—", never 0.
        for key in ["时间 (GPS)", "位置 X (ECEF)", "纬度", "速度 E", "解类型"] {
            let row = row_by_key(&rows, key);
            assert_eq!(row.sigma, None, "{key} has no exposed sigma");
            assert_eq!(row.sigma_display(), "—", "{key} sigma display");
            assert_ne!(row.sigma_display(), "0");
        }
        // A covariance default of 0.0 means "not computed" -> "—", never "0.000".
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new({
            let mut pvt = sample_pvt_view_model();
            pvt.quality.vertical_sigma_m = 0.0;
            pvt.quality.velocity_sigma_mps = Some(0.0);
            pvt
        }));
        let rows = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview: false,
        });
        assert_eq!(row_by_key(&rows, "高度").sigma_display(), "—");
        assert_eq!(row_by_key(&rows, "3D 速度").sigma_display(), "—");
        assert!(rows
            .iter()
            .all(|row| row.sigma_display() != "0" && row.sigma_display() != "0.000"));
    }

    #[test]
    fn dop_values_combine_into_a_single_row() {
        let rows = build(sample_input(false));
        assert_eq!(
            rows.iter().filter(|row| row.key.contains("DOP")).count(),
            1,
            "PDOP/HDOP/VDOP share one row"
        );
        let dop = row_by_key(&rows, "PDOP / HDOP / VDOP");
        assert_eq!(dop.key, "PDOP / HDOP / VDOP");
        // The view model now carries the receiver's real PDOP/VDOP (issue 19).
        assert_eq!(dop.value, "5.40 / 3.90 / 4.50");
        assert_eq!(dop.status, RowStatus::Healthy);
        // Preview tells the §3.0 story on exactly this row.
        let preview = build(sample_input(true));
        let dop = row_by_key(&preview, "PDOP / HDOP / VDOP");
        assert_eq!(dop.value, "3.12 / 2.41 / 2.08");
        assert_eq!(dop.status, RowStatus::Healthy);
    }

    #[test]
    fn dop_row_renders_position_wise_dash_for_missing_components() {
        // Mixed presence (e.g. a pre-issue-19 stored run): each missing
        // component renders "—" in its own position — never 0, never a
        // fabricated neighbor value.
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new({
            let mut pvt = sample_pvt_view_model();
            pvt.quality.pdop = Some(3.12);
            pvt.quality.hdop = 2.41;
            pvt.quality.vdop = None;
            pvt
        }));
        let rows = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview: false,
        });
        let dop = row_by_key(&rows, "PDOP / HDOP / VDOP");
        assert_eq!(dop.value, "3.12 / 2.41 / —");
        assert_eq!(dop.status, RowStatus::Healthy);

        // A stored run without any DOP data renders the row-wide "—".
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new({
            let mut pvt = sample_pvt_view_model();
            pvt.quality.pdop = None;
            pvt.quality.vdop = None;
            pvt.quality.hdop = 0.0;
            pvt
        }));
        let rows = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview: false,
        });
        let dop = row_by_key(&rows, "PDOP / HDOP / VDOP");
        assert_eq!(dop.value, "—");
        assert_eq!(dop.status, RowStatus::Unavailable);
        assert!(
            !dop.value.contains('0'),
            "never substitute 0: {}",
            dop.value
        );
    }

    #[test]
    fn used_tracked_row_presentation_is_invariant() {
        // Preview story: 使用/跟踪 = 7 / 9 (spec §3.0 / §3.8).
        let preview = build(sample_input(true));
        assert_eq!(row_by_key(&preview, "使用/跟踪卫星数").value, "7 / 9");
        // Fixture: 5 used PRNs, no satellite list -> tracked collapses to used.
        let fixture = build(sample_input(false));
        assert_eq!(row_by_key(&fixture, "使用/跟踪卫星数").value, "5 / 5");
        // With a satellite list the tracked count follows it.
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new({
            let mut pvt = sample_pvt_view_model();
            pvt.satellites = (0..9)
                .map(|index| GuiSatelliteGeometry {
                    prn: index,
                    azimuth_deg: 0.0,
                    elevation_deg: 45.0,
                    residual_m: 0.1,
                    standardized_residual: 0.1,
                    pseudorange_sigma_m: 1.0,
                    used_in_solution: index < 5,
                    raim_excluded: false,
                })
                .collect();
            pvt
        }));
        let rows = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview: false,
        });
        let row = row_by_key(&rows, "使用/跟踪卫星数");
        assert_eq!(row.value, "5 / 9");
        let (used, tracked) = row.value.split_once(" / ").unwrap();
        assert!(
            used.parse::<usize>().unwrap() <= tracked.parse::<usize>().unwrap(),
            "使用 <= 跟踪 invariant"
        );
    }

    #[test]
    fn status_text_maps_to_the_three_quiet_labels() {
        assert_eq!(RowStatus::Healthy.label(), "有效");
        assert_eq!(RowStatus::Unavailable.label(), "不可用");
        assert_eq!(RowStatus::Invalid.label(), "无效");

        // Healthy fixture: every row reports 有效.
        let healthy = build(sample_input(false));
        assert!(healthy
            .iter()
            .all(|row| row.status == RowStatus::Healthy && row.status.label() == "有效"));

        // No selected-epoch solution: every row is 不可用 with an honest "—".
        let missing = build(SummaryInput {
            pvt: None,
            position_displayable: false,
            preview: false,
        });
        assert_eq!(missing.len(), 15);
        for row in &missing {
            assert_eq!(row.status, RowStatus::Unavailable);
            assert_eq!(row.status.label(), "不可用");
            assert_eq!(row.value, "—");
            assert_eq!(row.sigma_display(), "—");
        }

        // Position failing the quality gate: rows show 无效 (value stays real).
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new(sample_pvt_view_model()));
        let invalid = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: false,
            preview: false,
        });
        assert_eq!(row_by_key(&invalid, "纬度").status, RowStatus::Invalid);
        assert_eq!(row_by_key(&invalid, "纬度").status.label(), "无效");
        assert_ne!(row_by_key(&invalid, "纬度").value, "—");

        // Unconverged solution kind is 无效 and the fix-type value stays an
        // honest "—" (the candidate Debug string is never the fix type);
        // missing velocity rows are 不可用.
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new({
            let mut pvt = sample_pvt_view_model();
            pvt.solution_kind = PvtEpochSolutionKind::Unconverged;
            pvt.velocity = None;
            pvt
        }));
        let rows = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview: false,
        });
        assert_eq!(row_by_key(&rows, "解类型").status, RowStatus::Invalid);
        assert_eq!(row_by_key(&rows, "解类型").value, "—");
        assert_eq!(row_by_key(&rows, "速度 E").status, RowStatus::Unavailable);
        assert_eq!(row_by_key(&rows, "速度 E").value, "—");
        assert_eq!(row_by_key(&rows, "3D 速度").value, "—");
    }

    #[test]
    fn fix_type_row_consumes_the_real_derived_fix_type() {
        // Healthy fixture derives "3D Fix" — the real fix type, not the
        // candidate kind Debug string.
        let healthy = build(sample_input(false));
        let fix = row_by_key(&healthy, "解类型");
        assert_eq!(fix.value, "3D Fix");
        assert_eq!(fix.status, RowStatus::Healthy);

        // A candidate kind string never appears in the fix-type cell.
        for candidate_debug in ["Raw", "Smoothed", "Kalman"] {
            assert_ne!(fix.value, candidate_debug);
        }

        // A converged-looking Raw selection with too few used satellites has
        // no derivable fix type: honest "—" / 不可用.
        let pvt: &'static GuiPvtViewModel = Box::leak(Box::new({
            let mut pvt = sample_pvt_view_model();
            pvt.quality.n_sats = 3;
            pvt
        }));
        let undersampled = build(SummaryInput {
            pvt: Some(pvt),
            position_displayable: true,
            preview: false,
        });
        let fix = row_by_key(&undersampled, "解类型");
        assert_eq!(fix.value, "—");
        assert_eq!(fix.status, RowStatus::Unavailable);

        // No selected-epoch solution: every row, including 解类型, stays "—".
        let missing = build(SummaryInput {
            pvt: None,
            position_displayable: false,
            preview: false,
        });
        assert_eq!(row_by_key(&missing, "解类型").value, "—");
    }
}
