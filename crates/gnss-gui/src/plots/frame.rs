//! Shared plot pane frame (design-refresh spec §3.4).
//!
//! Pane chrome: opaque `surface` body with a 1px border, a 28px pane head
//! carrying a quiet pane_title 13/600 title, an optional caption 11px
//! subtitle, and rect radius-4 20px-high controls at the right end. Grid
//! lines are 1px solid `plot_grid` and axes are 1px solid `plot_axis`; the
//! quad-based drawing model has no dash support, so dashed or dash-dotted
//! axes are structurally impossible here (spec §3.4 / gap 19). Emptiness is
//! rendered as one of the four honest states (暂无数据 / 加载中… / 错误: … /
//! 不可用) as centered caption text; curves are never fabricated (spec §7.7).

use crate::visual_system::{metrics, Palette};
use gpui_kit::prelude::*;
use gpui_kit::{div, point, px, quad, size, AnyElement, Bounds, SharedString, Window};

/// Stroke-width policy: grid and axis lines are exactly 1px, solid.
pub(crate) const LINE_STROKE_WIDTH: f32 = 1.0;

/// Pane-head control height (spec §3.4: 20px controls inside the 28px head).
pub(crate) const HEAD_CONTROL_HEIGHT: f32 = 20.0;

/// The four honest pane states (spec §3.4). Curves are never fabricated.
/// Production paths currently only reach `Empty`; the other three variants
/// are required by the frozen four-state contract and unit-tested, so they
/// are kept even though no live source emits them yet.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlotState {
    Empty,
    Loading,
    Error(String),
    Unavailable,
}

impl PlotState {
    pub(crate) fn caption(&self) -> String {
        match self {
            Self::Empty => "暂无数据".to_string(),
            Self::Loading => "加载中…".to_string(),
            Self::Error(detail) => format!("错误: {detail}"),
            Self::Unavailable => "不可用".to_string(),
        }
    }
}

/// A straight plot line. The quad-based drawing model has no dash support,
/// so a `LineRole` can only ever paint solid (spec §3.4 / gap 19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineRole {
    Grid,
    Axis,
}

impl LineRole {
    pub(crate) fn stroke_width(self) -> f32 {
        LINE_STROKE_WIDTH
    }

    fn color(self, palette: Palette) -> gpui_kit::Hsla {
        match self {
            Self::Grid => palette.plot_grid,
            Self::Axis => palette.plot_axis,
        }
    }
}

/// Paint a vertical 1px solid line.
pub(crate) fn paint_v_line(
    window: &mut Window,
    x: f32,
    y0: f32,
    y1: f32,
    role: LineRole,
    palette: Palette,
) {
    let height = (y1 - y0).max(role.stroke_width());
    window.paint_quad(quad(
        Bounds {
            origin: point(px(x), px(y0)),
            size: size(px(role.stroke_width()), px(height)),
        },
        px(0.),
        role.color(palette),
        px(0.),
        role.color(palette),
        Default::default(),
    ));
}

/// Paint a horizontal 1px solid line.
pub(crate) fn paint_h_line(
    window: &mut Window,
    y: f32,
    x0: f32,
    x1: f32,
    role: LineRole,
    palette: Palette,
) {
    let width = (x1 - x0).max(role.stroke_width());
    window.paint_quad(quad(
        Bounds {
            origin: point(px(x0), px(y)),
            size: size(px(width), px(role.stroke_width())),
        },
        px(0.),
        role.color(palette),
        px(0.),
        role.color(palette),
        Default::default(),
    ));
}

/// Bottom-right grid-spacing note, caption 11px text_tertiary (spec §3.4).
pub(crate) fn grid_note(text: impl Into<SharedString>, palette: Palette) -> AnyElement {
    div()
        .absolute()
        .right(px(8.))
        .bottom(px(6.))
        .text_size(px(11.))
        .text_color(palette.faint)
        .child(text.into())
        .into_any_element()
}

/// "网格: 10 m" label for a real grid step in meters.
pub(crate) fn grid_spacing_label(step_m: f64) -> String {
    if step_m.abs() >= 10.0 || step_m.fract().abs() < f64::EPSILON {
        format!("网格: {step_m:.0} m")
    } else {
        format!("网格: {step_m:.1} m")
    }
}

/// Quiet rect control for a pane head (radius 4, 20px high, caption text).
pub(crate) fn quiet_control(label: impl Into<SharedString>, palette: Palette) -> AnyElement {
    div()
        .h(px(HEAD_CONTROL_HEIGHT))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(palette.border)
        .text_size(px(11.))
        .text_color(palette.muted)
        .child(label.into())
        .into_any_element()
}

/// Centered four-state caption (spec §3.4); never a fabricated curve.
pub(crate) fn state_caption(state: &PlotState, palette: Palette) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .text_color(palette.faint)
        .child(state.caption())
        .into_any_element()
}

/// Pane chrome shared by every plot pane (spec §3.4): 28px head with a quiet
/// title, optional caption subtitle, right-end 20px controls, opaque
/// `surface` well with a 4px gutter.
pub(crate) fn render(
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    plot: AnyElement,
    controls: Vec<AnyElement>,
    expanded: bool,
    palette: Palette,
) -> AnyElement {
    div()
        .min_h(if expanded { px(620.) } else { px(310.) })
        .h_full()
        .flex()
        .flex_col()
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .child(
            div()
                .h(px(metrics::PLOT_PANE_HEADER_HEIGHT))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_between()
                .bg(palette.surface_alt)
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_shrink_0()
                                // pane_title = 13px semibold (typography scale
                                // is private to visual_system; value mirrored).
                                .text_size(px(13.))
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(palette.muted)
                                .child(title.into()),
                        )
                        .children(subtitle.map(|subtitle| {
                            div()
                                // caption = 11px.
                                .text_size(px(11.))
                                .text_color(palette.faint)
                                .child(subtitle)
                        })),
                )
                .child(div().flex().items_center().gap(px(4.)).children(controls)),
        )
        .child(
            div()
                .mt(px(metrics::PLOT_GRID_PADDING))
                .mx(px(metrics::PLOT_GRID_PADDING))
                .mb(px(metrics::PLOT_GRID_PADDING))
                .flex_1()
                .min_h(if expanded { px(550.) } else { px(260.) })
                .child(plot),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_policy_is_one_pixel_solid() {
        // The quad model has no dash pattern: both roles are 1px solid and
        // carry distinct grid/axis palette roles (gap 19: no dashed axes).
        assert_eq!(LineRole::Grid.stroke_width(), 1.0);
        assert_eq!(LineRole::Axis.stroke_width(), 1.0);
        assert_eq!(LINE_STROKE_WIDTH, 1.0);
        assert_ne!(
            LineRole::Grid.color(Palette::dark()),
            LineRole::Axis.color(Palette::dark())
        );
    }

    #[test]
    fn four_states_render_the_frozen_captions() {
        assert_eq!(PlotState::Empty.caption(), "暂无数据");
        assert_eq!(PlotState::Loading.caption(), "加载中…");
        assert_eq!(
            PlotState::Error("文件损坏".to_string()).caption(),
            "错误: 文件损坏"
        );
        assert_eq!(PlotState::Unavailable.caption(), "不可用");
    }

    #[test]
    fn grid_note_carries_the_real_step() {
        assert_eq!(grid_spacing_label(10.0), "网格: 10 m");
        assert_eq!(grid_spacing_label(2.5), "网格: 2.5 m");
    }

    #[test]
    fn pane_head_is_twenty_eight_pixels() {
        assert_eq!(metrics::PLOT_PANE_HEADER_HEIGHT, 28.0);
        assert_eq!(HEAD_CONTROL_HEIGHT, 20.0);
    }
}
