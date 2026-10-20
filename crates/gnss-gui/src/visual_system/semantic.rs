//! Product-level visual roles shared by shell, controls, plots, and statuses.

use super::primitives::{color, dark, light, opacity, shared};
use gpui_kit::Hsla;

/// Semantic colors used by the GNSS workbench.
///
/// Light and dark themes are authored independently. The graphite surface
/// ladder reflects the restrained density studied in Comet and Waku, while
/// cyan and receiver health colors remain GNSScope-specific.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    /// Deepest window tone behind every floating panel.
    pub(crate) canvas: Hsla,
    pub(crate) background: Hsla,
    pub(crate) sidebar: Hsla,
    pub(crate) surface: Hsla,
    pub(crate) surface_alt: Hsla,
    pub(crate) surface_raised: Hsla,
    pub(crate) surface_dialog: Hsla,
    /// Ink-alpha hover/press overlays, valid on any surface tier.
    pub(crate) hover: Hsla,
    pub(crate) active: Hsla,
    /// Lit inner edge for floating panels (dark: white sheen, light: none).
    pub(crate) panel_sheen: Hsla,
    pub(crate) disabled: Hsla,
    pub(crate) border: Hsla,
    /// A fainter rule than `border`: table body separators, lane dividers and
    /// in-pane group breaks, which must not compete with the pane edges.
    pub(crate) border_subtle: Hsla,
    pub(crate) divider: Hsla,
    pub(crate) border_strong: Hsla,
    pub(crate) focus_ring: Hsla,
    pub(crate) text: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) faint: Hsla,
    pub(crate) text_disabled: Hsla,
    pub(crate) text_inverse: Hsla,
    pub(crate) accent: Hsla,
    pub(crate) accent_hover: Hsla,
    pub(crate) accent_pressed: Hsla,
    pub(crate) accent_wash: Hsla,
    pub(crate) on_accent: Hsla,
    pub(crate) solid: Hsla,
    pub(crate) solid_hover: Hsla,
    pub(crate) solid_pressed: Hsla,
    pub(crate) on_solid: Hsla,
    pub(crate) overlay: Hsla,
    pub(crate) overlay_strong: Hsla,
    /// Panel drop shadow (2026-10-20 floating chassis): a single soft shadow
    /// that gives every region weight without a second elevation language.
    pub(crate) shadow: Hsla,
    /// Stronger shadow reserved for true floating layers (menus, palette).
    pub(crate) shadow_strong: Hsla,
    pub(crate) healthy: Hsla,
    pub(crate) healthy_wash: Hsla,
    pub(crate) warning: Hsla,
    pub(crate) warning_wash: Hsla,
    /// Attention amber (spec §2.5 `attention(warning)`): queued / capture /
    /// watch states. Distinct from `accent`, which owns active processing
    /// (处理中 = accent, 排队 = attention; the two must never share a color).
    pub(crate) attention: Hsla,
    pub(crate) danger: Hsla,
    pub(crate) danger_wash: Hsla,
    pub(crate) unavailable: Hsla,
    pub(crate) unavailable_wash: Hsla,
    pub(crate) plot_grid: Hsla,
    pub(crate) plot_axis: Hsla,
    pub(crate) plot_cursor: Hsla,
    pub(crate) plot_selection: Hsla,
    pub(crate) chart_series: [Hsla; 10],
    /// Sky-plot elevation bands, ordered 75-90 … 0-15 (spec §3.5).
    pub(crate) elevation_bands: [Hsla; 6],
}

impl Palette {
    pub(crate) fn light() -> Self {
        Self {
            canvas: color(light::CANVAS),
            background: color(light::BACKGROUND),
            sidebar: color(light::SIDEBAR),
            surface: color(light::SURFACE),
            surface_alt: color(light::SURFACE_ALT),
            surface_raised: color(light::SURFACE_RAISED),
            surface_dialog: color(light::SURFACE_DIALOG),
            hover: color(light::TEXT).opacity(opacity::STATE_HOVER),
            active: color(light::TEXT).opacity(opacity::STATE_PRESSED),
            panel_sheen: color(light::TEXT).opacity(opacity::PANEL_SHEEN * 0.6),
            disabled: color(light::DISABLED),
            border: color(light::TEXT).opacity(0.12),
            border_subtle: color(light::TEXT).opacity(0.07),
            divider: color(light::TEXT).opacity(0.10),
            border_strong: color(light::TEXT).opacity(0.22),
            focus_ring: color(light::ACCENT),
            text: color(light::TEXT),
            muted: color(light::MUTED),
            faint: color(light::FAINT),
            text_disabled: color(light::TEXT_DISABLED),
            text_inverse: color(shared::LIGHT_ON_SOLID),
            accent: color(light::ACCENT),
            accent_hover: color(light::ACCENT_HOVER),
            accent_pressed: color(light::ACCENT_PRESSED),
            accent_wash: color(light::ACCENT_WASH),
            on_accent: color(shared::LIGHT_ON_ACCENT),
            solid: color(shared::LIGHT_SOLID),
            solid_hover: color(shared::LIGHT_SOLID_HOVER),
            solid_pressed: color(shared::LIGHT_SOLID_PRESSED),
            on_solid: color(shared::LIGHT_ON_SOLID),
            overlay: color(shared::OVERLAY).opacity(opacity::OVERLAY),
            overlay_strong: color(shared::OVERLAY).opacity(opacity::OVERLAY_STRONG),
            shadow: color(shared::OVERLAY).opacity(0.10),
            shadow_strong: color(shared::OVERLAY).opacity(0.16),
            healthy: color(light::HEALTHY),
            healthy_wash: color(light::HEALTHY_WASH),
            warning: color(light::WARNING),
            warning_wash: color(light::WARNING_WASH),
            attention: color(light::WARNING),
            danger: color(light::DANGER),
            danger_wash: color(light::DANGER_WASH),
            unavailable: color(light::UNAVAILABLE),
            unavailable_wash: color(light::UNAVAILABLE_WASH),
            plot_grid: color(light::PLOT_GRID),
            plot_axis: color(light::PLOT_AXIS),
            plot_cursor: color(light::ACCENT),
            plot_selection: color(light::ACCENT_WASH),
            chart_series: shared::CHART_LIGHT.map(color),
            elevation_bands: light::ELEVATION.map(color),
        }
    }

    pub(crate) fn dark() -> Self {
        Self {
            canvas: color(dark::CANVAS),
            background: color(dark::BACKGROUND),
            sidebar: color(dark::SIDEBAR),
            surface: color(dark::SURFACE),
            surface_alt: color(dark::SURFACE_ALT),
            surface_raised: color(dark::SURFACE_RAISED),
            surface_dialog: color(dark::SURFACE_DIALOG),
            hover: color(dark::TEXT).opacity(opacity::STATE_HOVER),
            active: color(dark::TEXT).opacity(opacity::STATE_PRESSED),
            panel_sheen: color(dark::TEXT).opacity(opacity::PANEL_SHEEN),
            disabled: color(dark::DISABLED),
            border: color(dark::TEXT).opacity(0.10),
            border_subtle: color(dark::TEXT).opacity(0.06),
            divider: color(dark::TEXT).opacity(0.08),
            border_strong: color(dark::TEXT).opacity(0.20),
            focus_ring: color(dark::ACCENT),
            text: color(dark::TEXT),
            muted: color(dark::MUTED),
            faint: color(dark::FAINT),
            text_disabled: color(dark::TEXT_DISABLED),
            text_inverse: color(shared::DARK_ON_SOLID),
            accent: color(dark::ACCENT),
            accent_hover: color(dark::ACCENT_HOVER),
            accent_pressed: color(dark::ACCENT_PRESSED),
            accent_wash: color(dark::ACCENT_WASH),
            on_accent: color(shared::DARK_ON_ACCENT),
            solid: color(shared::DARK_SOLID),
            solid_hover: color(shared::DARK_SOLID_HOVER),
            solid_pressed: color(shared::DARK_SOLID_PRESSED),
            on_solid: color(shared::DARK_ON_SOLID),
            overlay: color(shared::OVERLAY).opacity(opacity::OVERLAY),
            overlay_strong: color(shared::OVERLAY).opacity(opacity::OVERLAY_STRONG),
            shadow: color(0x000000).opacity(0.45),
            shadow_strong: color(0x000000).opacity(0.62),
            healthy: color(dark::HEALTHY),
            healthy_wash: color(dark::HEALTHY_WASH),
            warning: color(dark::WARNING),
            warning_wash: color(dark::WARNING_WASH),
            attention: color(dark::WARNING),
            danger: color(dark::DANGER),
            danger_wash: color(dark::DANGER_WASH),
            unavailable: color(dark::UNAVAILABLE),
            unavailable_wash: color(dark::UNAVAILABLE_WASH),
            plot_grid: color(dark::PLOT_GRID),
            plot_axis: color(dark::PLOT_AXIS),
            plot_cursor: color(dark::ACCENT),
            plot_selection: color(dark::ACCENT_WASH),
            chart_series: shared::CHART_DARK.map(color),
            elevation_bands: dark::ELEVATION.map(color),
        }
    }
}
