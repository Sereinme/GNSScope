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
    pub(crate) const TITLE_BAR_HEIGHT: f32 = 40.0;
    pub(crate) const RUNS_DOCK_WIDTH: f32 = 316.0;
    pub(crate) const RUNS_DOCK_MIN_WIDTH: f32 = 286.0;
    pub(crate) const CONFIGURATION_RAIL_WIDTH: f32 = 276.0;
    pub(crate) const CONFIGURATION_RAIL_MIN_WIDTH: f32 = 258.0;
    pub(crate) const RUNS_HEADER_HEIGHT: f32 = 38.0;
    pub(crate) const RUNS_SEARCH_HEIGHT: f32 = 42.0;
    // Height recovery (design-refresh spec §3): the workbench header budget
    // drops 96 -> 76 (CONTEXT 48 + TABS 28), freeing 20px for the fixed
    // 148px epoch timeline without covering PVT/C/N0 content at 1003px.
    pub(crate) const WORKSPACE_HEADER_HEIGHT: f32 = 76.0;
    pub(crate) const WORKSPACE_CONTEXT_HEIGHT: f32 = 48.0;
    pub(crate) const WORKSPACE_TABS_HEIGHT: f32 = 28.0;
    pub(crate) const STATUS_BAR_HEIGHT: f32 = 28.0;
    pub(crate) const CONTROL_RADIUS: f32 = super::radius::CONTROL;

    // Design-refresh metrics (spec §2.9); not yet consumed by consumers
    // (height recovery / timeline / runs / plots packages land next).
    pub(crate) const TIMELINE_HEIGHT: f32 = 148.0;
    pub(crate) const TIMELINE_HEADER_HEIGHT: f32 = 28.0;
    pub(crate) const TIMELINE_TRACK_HEIGHT: f32 = 85.0;
    pub(crate) const TIMELINE_LABELS_HEIGHT: f32 = 18.0;
    pub(crate) const TIMELINE_TRANSPORT_WIDTH: f32 = 144.0;
    pub(crate) const RUN_ROW_HEIGHT: f32 = 50.0;
    pub(crate) const RUN_SECTION_HEADER_HEIGHT: f32 = 24.0;
    pub(crate) const PVT_ROW_HEIGHT: f32 = 20.0;
    pub(crate) const PVT_HEADER_ROW_HEIGHT: f32 = 22.0;
    pub(crate) const PLOT_PANE_HEADER_HEIGHT: f32 = 28.0;
    pub(crate) const PLOT_GRID_PADDING: f32 = 4.0;
    pub(crate) const PLOT_GRID_GAP: f32 = 4.0;
    // C/N0 history lane grid (spec §3.7 / gap 17): fixed 28px lane pitch so
    // the nine lanes never reflow when values change.
    pub(crate) const CN0_LANE_PITCH: f32 = 28.0;
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
    pub(crate) const LG: f32 = 16.0;
    pub(crate) const XL: f32 = 24.0;
}

#[allow(dead_code)]
pub(super) mod typography {
    // MICRO is the tick role (chart/timeline tick labels, spec §2.2 "tick").
    pub(crate) const MICRO: f32 = 10.0;
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
    pub(crate) const PANE: f32 = 2.0;
    pub(crate) const CONTROL: f32 = 4.0;
    pub(crate) const CONTROL_SOFT: f32 = 6.0;
    pub(crate) const RAISED: f32 = 8.0;
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
}

#[allow(dead_code)]
pub(super) mod duration {
    pub(crate) const HOVER_MS: u64 = 120;
    pub(crate) const PRESS_MS: u64 = 80;
    pub(crate) const MENU_MS: u64 = 140;
    pub(crate) const PANE_MS: u64 = 180;
}

pub(super) mod light {
    pub(crate) const BACKGROUND: u32 = 0xf3f4f6;
    pub(crate) const SIDEBAR: u32 = 0xedeff2;
    pub(crate) const SURFACE: u32 = 0xffffff;
    pub(crate) const SURFACE_ALT: u32 = 0xf6f7f8;
    pub(crate) const SURFACE_RAISED: u32 = 0xffffff;
    pub(crate) const SURFACE_DIALOG: u32 = 0xffffff;
    pub(crate) const HOVER: u32 = 0xe9ecef;
    pub(crate) const ACTIVE: u32 = 0xdfe5ea;
    pub(crate) const DISABLED: u32 = 0xe8ebee;
    pub(crate) const BORDER: u32 = 0xd6dae0;
    pub(crate) const BORDER_STRONG: u32 = 0xb8c0ca;
    pub(crate) const TEXT: u32 = 0x242930;
    pub(crate) const MUTED: u32 = 0x56616d;
    pub(crate) const FAINT: u32 = 0x68737f;
    pub(crate) const TEXT_DISABLED: u32 = 0x8b949f;
    pub(crate) const ACCENT: u32 = 0x167a94;
    pub(crate) const ACCENT_HOVER: u32 = 0x126d85;
    pub(crate) const ACCENT_PRESSED: u32 = 0x0f5f75;
    pub(crate) const ACCENT_WASH: u32 = 0xe0f1f4;
    pub(crate) const HEALTHY: u32 = 0x147958;
    pub(crate) const HEALTHY_WASH: u32 = 0xe1f3eb;
    pub(crate) const WARNING: u32 = 0x985700;
    pub(crate) const WARNING_WASH: u32 = 0xf8ecd8;
    pub(crate) const DANGER: u32 = 0xb63542;
    pub(crate) const DANGER_WASH: u32 = 0xf8e4e6;
    pub(crate) const UNAVAILABLE: u32 = 0x66717d;
    pub(crate) const UNAVAILABLE_WASH: u32 = 0xe9ecef;
    // Weakened for the 1px-solid grid policy (spec §2.9 / gap 19).
    pub(crate) const PLOT_GRID: u32 = 0xe3e8ec;
    pub(crate) const PLOT_AXIS: u32 = 0x6b7580;
}

pub(super) mod dark {
    pub(crate) const BACKGROUND: u32 = 0x090a0c;
    pub(crate) const SIDEBAR: u32 = 0x0e1013;
    pub(crate) const SURFACE: u32 = 0x111419;
    pub(crate) const SURFACE_ALT: u32 = 0x171b21;
    pub(crate) const SURFACE_RAISED: u32 = 0x1c2128;
    pub(crate) const SURFACE_DIALOG: u32 = 0x20252c;
    pub(crate) const HOVER: u32 = 0x1a1f26;
    pub(crate) const ACTIVE: u32 = 0x222932;
    pub(crate) const DISABLED: u32 = 0x181c22;
    pub(crate) const BORDER: u32 = 0x282f38;
    pub(crate) const BORDER_STRONG: u32 = 0x3b4551;
    pub(crate) const TEXT: u32 = 0xe7ebef;
    pub(crate) const MUTED: u32 = 0xaab2bc;
    pub(crate) const FAINT: u32 = 0x89939f;
    pub(crate) const TEXT_DISABLED: u32 = 0x626d79;
    pub(crate) const ACCENT: u32 = 0x58c7db;
    pub(crate) const ACCENT_HOVER: u32 = 0x6fd0e1;
    pub(crate) const ACCENT_PRESSED: u32 = 0x3fb8ce;
    pub(crate) const ACCENT_WASH: u32 = 0x142c32;
    pub(crate) const HEALTHY: u32 = 0x65c991;
    pub(crate) const HEALTHY_WASH: u32 = 0x14281f;
    pub(crate) const WARNING: u32 = 0xe0b45b;
    pub(crate) const WARNING_WASH: u32 = 0x302715;
    pub(crate) const DANGER: u32 = 0xeb7077;
    pub(crate) const DANGER_WASH: u32 = 0x32191d;
    pub(crate) const UNAVAILABLE: u32 = 0x89939f;
    // Progress-strip groove target (spec §2.7 / theme.css --track); light unchanged.
    pub(crate) const UNAVAILABLE_WASH: u32 = 0x262c35;
    // Weakened for the 1px-solid grid policy (spec §2.9 / gap 19).
    pub(crate) const PLOT_GRID: u32 = 0x242b33;
    pub(crate) const PLOT_AXIS: u32 = 0x89939f;
}

pub(super) mod shared {
    pub(crate) const LIGHT_ON_ACCENT: u32 = 0xffffff;
    pub(crate) const LIGHT_SOLID: u32 = 0x242930;
    pub(crate) const LIGHT_ON_SOLID: u32 = 0xffffff;
    pub(crate) const DARK_ON_ACCENT: u32 = 0x071316;
    pub(crate) const DARK_SOLID: u32 = 0xe7ebef;
    pub(crate) const DARK_ON_SOLID: u32 = 0x111419;
    pub(crate) const OVERLAY: u32 = 0x020305;

    // 10 series colors; theme.css `--chart-1..10`. C/N0 needs 9 adjacent-distinct
    // lanes (spec §3.7 / gap 17); entries 9/10 are the design-refresh additions.
    pub(crate) const CHART_LIGHT: [u32; 10] = [
        0x147958, 0x3578c8, 0x985700, 0x7446a6, 0xc65f17, 0x167a94, 0x66717d, 0xb63542, 0x8a6a2f,
        0xa84a82,
    ];
    pub(crate) const CHART_DARK: [u32; 10] = [
        0x65c991, 0x78aef7, 0xe0b45b, 0xb792f4, 0xed8a4b, 0x58c7db, 0xa7b0bd, 0xeb7077, 0xc2a06f,
        0xd48fb4,
    ];
}
