//! Raw visual values. Product and component code must not import this module.

use gpui_kit::{rgb, Hsla};

pub(super) fn color(value: u32) -> Hsla {
    rgb(value).into()
}

pub(crate) fn interface_font_family() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "SF Pro Text"
    }
    #[cfg(target_os = "windows")]
    {
        "Segoe UI Variable Text"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "Noto Sans"
    }
}

pub(crate) fn data_font_family() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "SF Mono"
    }
    #[cfg(target_os = "windows")]
    {
        "Cascadia Mono"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "monospace"
    }
}

/// Shared shell geometry derived from the concept workbench.
#[allow(dead_code)]
pub(crate) mod metrics {
    pub(crate) const TITLE_BAR_HEIGHT: f32 = 48.0;
    /// Gutter between floating panels and the window canvas.
    pub(crate) const SHELL_GUTTER: f32 = 8.0;
    pub(crate) const PLOT_PANE_HEADER_HEIGHT_V3: f32 = 30.0;
    pub(crate) const RUNS_DOCK_WIDTH: f32 = 316.0;
    pub(crate) const RUNS_DOCK_MIN_WIDTH: f32 = 286.0;
    pub(crate) const CONFIGURATION_RAIL_WIDTH: f32 = 276.0;
    pub(crate) const CONFIGURATION_RAIL_MIN_WIDTH: f32 = 258.0;
    pub(crate) const RUNS_HEADER_HEIGHT: f32 = 38.0;
    pub(crate) const RUNS_SEARCH_HEIGHT: f32 = 42.0;
    // Height recovery (design-refresh spec §3): the workbench header budget
    // drops 96 -> 76 (CONTEXT 48 + TABS 28), freeing 20px for the fixed
    // 148px epoch timeline without covering PVT/C/N0 content at 1003px.
    pub(crate) const WORKSPACE_HEADER_HEIGHT: f32 = 64.0;
    pub(crate) const WORKSPACE_CONTEXT_HEIGHT: f32 = 42.0;
    pub(crate) const WORKSPACE_TABS_HEIGHT: f32 = 22.0;
    pub(crate) const STATUS_BAR_HEIGHT: f32 = 32.0;
    pub(crate) const CONTROL_RADIUS: f32 = super::radius::CONTROL;

    // Design-refresh metrics (spec §2.9); not yet consumed by consumers
    // (height recovery / timeline / runs / plots packages land next).
    pub(crate) const TIMELINE_HEIGHT: f32 = 124.0;
    pub(crate) const TIMELINE_HEADER_HEIGHT: f32 = 28.0;
    pub(crate) const TIMELINE_TRACK_HEIGHT: f32 = 58.0;
    pub(crate) const TIMELINE_LABELS_HEIGHT: f32 = 18.0;
    pub(crate) const TIMELINE_TRANSPORT_WIDTH: f32 = 144.0;
    pub(crate) const RUN_ROW_HEIGHT: f32 = 50.0;
    pub(crate) const RUN_SECTION_HEADER_HEIGHT: f32 = 24.0;
    pub(crate) const PVT_ROW_HEIGHT: f32 = 18.5;
    pub(crate) const PVT_HEADER_ROW_HEIGHT: f32 = 20.0;
    pub(crate) const PLOT_PANE_HEADER_HEIGHT: f32 = 30.0;
    pub(crate) const PLOT_GRID_PADDING: f32 = 4.0;
    pub(crate) const PLOT_GRID_GAP: f32 = 4.0;
    // C/N0 history lane grid (spec §3.7 / gap 17): fixed 28px lane pitch so
    // the nine lanes never reflow when values change.
    pub(crate) const CN0_LANE_PITCH: f32 = 28.0;

    // Page-level geometry for the document surfaces (New Analysis, Run
    // Library, Interface Settings): a bounded measure keeps long forms from
    // stretching into unscannable full-width fields, and a pinned action bar
    // keeps the primary action reachable without scrolling.
    pub(crate) const PAGE_MAX_WIDTH: f32 = 1180.0;
    pub(crate) const PAGE_GUTTER: f32 = 32.0;
    pub(crate) const PAGE_GUTTER_COMPACT: f32 = 16.0;
    pub(crate) const SECTION_GAP: f32 = 22.0;
    pub(crate) const FORM_FIELD_WIDTH: f32 = 232.0;
    pub(crate) const FORM_FIELD_WIDE_WIDTH: f32 = 484.0;
    pub(crate) const FORM_ROW_HEIGHT: f32 = 46.0;
    pub(crate) const TABLE_ROW_HEIGHT: f32 = 26.0;
    pub(crate) const TABLE_HEADER_HEIGHT: f32 = 26.0;
    pub(crate) const ACTION_BAR_HEIGHT: f32 = 52.0;
    pub(crate) const LIBRARY_ROW_HEIGHT: f32 = 46.0;
}

// These named scales are intentionally established before Phase 2 migrates
// every control. Keeping them here prevents each primitive from inventing a
// local density or state duration during that migration.
#[allow(dead_code)]
pub(super) mod spacing {
    pub(crate) const HAIRLINE: f32 = 1.0;
    pub(crate) const XXS: f32 = 2.0;
    pub(crate) const XS: f32 = 4.0;
    pub(crate) const SM: f32 = 8.0;
    pub(crate) const MD: f32 = 12.0;
    /// Single horizontal inset for pane headers, table cells, rows, the
    /// status strip and the title bar, so every left edge in the app shares
    /// one rhythm (Apple's 8px base snapped to 12 for dense surfaces).
    pub(crate) const INSET: f32 = 12.0;
    /// Tighter inset for nested content (pane wells, list bodies).
    pub(crate) const INSET_TIGHT: f32 = 8.0;
    pub(crate) const LG: f32 = 16.0;
    pub(crate) const XL: f32 = 24.0;
}

#[allow(dead_code)]
pub(super) mod typography {
    // MICRO is the tick role (chart/timeline tick labels, spec §2.2 "tick").
    pub(crate) const MICRO: f32 = 10.0;
    // Page headings sit one step below the run title: a document surface
    // needs a quieter voice than the selected-run instrument (design pass).
    pub(crate) const PAGE_TITLE: f32 = 22.0;
    // Section eyebrow: the quiet uppercase-free grouping label used by the
    // configuration rail, settings groups, and page sections.
    pub(crate) const SECTION: f32 = 11.0;
    pub(crate) const CAPTION: f32 = 11.0;
    pub(crate) const MEASUREMENT_SMALL: f32 = 11.0;
    pub(crate) const METADATA: f32 = 12.0;
    pub(crate) const MEASUREMENT: f32 = 12.0;
    pub(crate) const CONTROL: f32 = 13.0;
    pub(crate) const BODY: f32 = 13.0;
    pub(crate) const PANE_TITLE: f32 = 13.0;
    pub(crate) const WORKSPACE_TITLE: f32 = 20.0;
}

#[allow(dead_code)]
pub(super) mod radius {
    // Carbon's radius scale: 0 for everything structural, 2px for tags, and a
    // pill only for status dots. A square is the system's signature — corners
    // are never rounded to look friendly, and depth is never implied by a
    // radius. Every name stays so call sites keep one vocabulary.
    pub(crate) const PANEL: f32 = 0.0;
    pub(crate) const PANE: f32 = PANEL;
    pub(crate) const SURFACE: f32 = 0.0;
    pub(crate) const CONTROL: f32 = 0.0;
    pub(crate) const CONTROL_SOFT: f32 = 0.0;
    pub(crate) const RAISED: f32 = 0.0;
    pub(crate) const DIALOG: f32 = 0.0;
    /// Tags and status chips: Carbon's only non-zero radius besides the pill.
    pub(crate) const TAG: f32 = 2.0;
    pub(crate) const PILL: f32 = 9999.0;
}

#[allow(dead_code)]
pub(super) mod border {
    pub(crate) const HAIRLINE: f32 = 1.0;
    pub(crate) const FOCUS: f32 = 2.0;
}

#[allow(dead_code)]
pub(super) mod opacity {
    pub(crate) const DISABLED: f32 = 0.52;
    pub(crate) const QUIET: f32 = 0.72;
    pub(crate) const OVERLAY: f32 = 0.42;
    pub(crate) const OVERLAY_STRONG: f32 = 0.62;
    /// State overlays are painted in ink alpha so a hover reads correctly on
    /// any surface (canvas, panel, nested card) instead of assuming one grey.
    pub(crate) const STATE_HOVER: f32 = 0.055;
    pub(crate) const STATE_PRESSED: f32 = 0.10;
    pub(crate) const STATE_SELECTED: f32 = 0.09;
    /// Inner top highlight that gives a panel a lit edge in dark themes.
    pub(crate) const PANEL_SHEEN: f32 = 0.05;
}

#[allow(dead_code)]
pub(super) mod duration {
    pub(crate) const HOVER_MS: u64 = 120;
    pub(crate) const PRESS_MS: u64 = 80;
    pub(crate) const MENU_MS: u64 = 140;
    pub(crate) const PANE_MS: u64 = 180;
}

pub(super) mod light {
    // IBM Carbon "White" theme: white canvas, three grey steps that only ever
    // move in 10-15 point increments, one decisive blue, and hairlines drawn
    // in the same grey as surface-2. Nothing is tinted; depth comes from tone.
    pub(crate) const BACKGROUND: u32 = 0xffffff;
    pub(crate) const SIDEBAR: u32 = 0xf4f4f4;
    pub(crate) const SURFACE: u32 = 0xffffff;
    pub(crate) const SURFACE_ALT: u32 = 0xf4f4f4;
    pub(crate) const SURFACE_RAISED: u32 = 0xf4f4f4;
    pub(crate) const SURFACE_DIALOG: u32 = 0xffffff;
    pub(crate) const DISABLED: u32 = 0xf4f4f4;
    pub(crate) const TEXT: u32 = 0x161616;
    pub(crate) const MUTED: u32 = 0x525252;
    pub(crate) const FAINT: u32 = 0x6f6f6f;
    pub(crate) const TEXT_DISABLED: u32 = 0xa8a8a8;
    pub(crate) const ACCENT: u32 = 0x0f62fe;
    pub(crate) const ACCENT_HOVER: u32 = 0x0050e6;
    pub(crate) const ACCENT_PRESSED: u32 = 0x002d9c;
    pub(crate) const ACCENT_WASH: u32 = 0xedf5ff;
    pub(crate) const HEALTHY: u32 = 0x198038;
    pub(crate) const HEALTHY_WASH: u32 = 0xdefbe6;
    pub(crate) const WARNING: u32 = 0x8e6a00;
    pub(crate) const WARNING_WASH: u32 = 0xfcf4d2;
    pub(crate) const DANGER: u32 = 0xda1e28;
    pub(crate) const DANGER_WASH: u32 = 0xfff1f1;
    pub(crate) const UNAVAILABLE: u32 = 0x6f6f6f;
    pub(crate) const UNAVAILABLE_WASH: u32 = 0xe0e0e0;
    /// The page is the canvas; regions separate by tone and hairline, never by
    /// a gutter, so the canvas is the same white as a panel.
    pub(crate) const CANVAS: u32 = 0xffffff;
    pub(crate) const PLOT_GRID: u32 = 0xe8e8e8;
    pub(crate) const PLOT_AXIS: u32 = 0x161616;
    pub(crate) const ELEVATION: [u32; 6] =
        [0x022b0f, 0x0e6027, 0x198038, 0x24a148, 0x42be65, 0x8ce4a9];
}

pub(super) mod dark {
    // IBM Carbon "Gray-100" theme: a flat near-black field, grey steps at
    // 161616 / 262626 / 393939, and blue-50 because blue-60 disappears on
    // dark. Surfaces never glow; hairlines do the separating.
    pub(crate) const BACKGROUND: u32 = 0x161616;
    pub(crate) const SIDEBAR: u32 = 0x1c1c1c;
    pub(crate) const SURFACE: u32 = 0x161616;
    pub(crate) const SURFACE_ALT: u32 = 0x262626;
    pub(crate) const SURFACE_RAISED: u32 = 0x262626;
    pub(crate) const SURFACE_DIALOG: u32 = 0x262626;
    pub(crate) const DISABLED: u32 = 0x262626;
    pub(crate) const TEXT: u32 = 0xf4f4f4;
    pub(crate) const MUTED: u32 = 0xc6c6c6;
    pub(crate) const FAINT: u32 = 0xa8a8a8;
    pub(crate) const TEXT_DISABLED: u32 = 0x6f6f6f;
    pub(crate) const ACCENT: u32 = 0x4589ff;
    pub(crate) const ACCENT_HOVER: u32 = 0x78a9ff;
    pub(crate) const ACCENT_PRESSED: u32 = 0xa6c8ff;
    pub(crate) const ACCENT_WASH: u32 = 0x0f2c55;
    pub(crate) const HEALTHY: u32 = 0x42be65;
    pub(crate) const HEALTHY_WASH: u32 = 0x0f2f1c;
    pub(crate) const WARNING: u32 = 0xf1c21b;
    pub(crate) const WARNING_WASH: u32 = 0x3d2f0b;
    pub(crate) const DANGER: u32 = 0xff8389;
    pub(crate) const DANGER_WASH: u32 = 0x3d1618;
    pub(crate) const UNAVAILABLE: u32 = 0x8d8d8d;
    pub(crate) const UNAVAILABLE_WASH: u32 = 0x393939;
    /// Same field as a panel: separations are hairlines, not gaps.
    pub(crate) const CANVAS: u32 = 0x161616;
    pub(crate) const PLOT_GRID: u32 = 0x333333;
    pub(crate) const PLOT_AXIS: u32 = 0x8d8d8d;
    pub(crate) const ELEVATION: [u32; 6] =
        [0xa7f0bd, 0x74e792, 0x42be65, 0x24a148, 0x198038, 0x0e6027];
}

pub(super) mod shared {
    pub(crate) const LIGHT_ON_ACCENT: u32 = 0xffffff;
    pub(crate) const LIGHT_SOLID: u32 = 0x1f252c;
    pub(crate) const LIGHT_SOLID_HOVER: u32 = 0x2b333c;
    pub(crate) const LIGHT_SOLID_PRESSED: u32 = 0x141a20;
    pub(crate) const LIGHT_ON_SOLID: u32 = 0xffffff;
    pub(crate) const DARK_ON_ACCENT: u32 = 0x061417;
    pub(crate) const DARK_SOLID: u32 = 0xe9eef3;
    pub(crate) const DARK_SOLID_HOVER: u32 = 0xffffff;
    pub(crate) const DARK_SOLID_PRESSED: u32 = 0xd2dae2;
    pub(crate) const DARK_ON_SOLID: u32 = 0x10161c;
    pub(crate) const OVERLAY: u32 = 0x020305;

    // 10 series colors; C/N0 needs 9 adjacent-distinct lanes (spec §3.7 /
    // gap 17). Light values are darkened until every lane label clears 4.5:1
    // on a white pane (spec §3.7 acceptance requirement).
    pub(crate) const CHART_LIGHT: [u32; 10] = [
        0x0f7a58, 0x2f6cc0, 0x8f5300, 0x6b3fa0, 0xb0521a, 0x137a94, 0x5c6673, 0xb13543, 0x7d5c22,
        0x9c3f78,
    ];
    pub(crate) const CHART_DARK: [u32; 10] = [
        0x67c996, 0x7fb2f7, 0xdfb45f, 0xb492f2, 0xec8a4c, 0x5cc8dd, 0xa9b3c0, 0xea7078, 0xc4a273,
        0xd692b6,
    ];
}
