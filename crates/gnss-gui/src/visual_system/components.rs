//! Component-level roles and the sole third-party theme adapter.

use super::primitives::{data_font_family, interface_font_family, radius};
use super::semantic::Palette;
use gpui_kit::component::Theme;
use gpui_kit::{px, Hsla};

#[derive(Clone, Copy)]
struct ComponentTokens {
    canvas: Hsla,
    foreground: Hsla,
    border: Hsla,
    input_border: Hsla,
    focus_ring: Hsla,
    caret: Hsla,
    selection: Hsla,
    primary: Hsla,
    primary_hover: Hsla,
    primary_pressed: Hsla,
    on_primary: Hsla,
    secondary: Hsla,
    secondary_hover: Hsla,
    secondary_pressed: Hsla,
    on_secondary: Hsla,
    quiet: Hsla,
    quiet_foreground: Hsla,
    sidebar: Hsla,
    sidebar_border: Hsla,
    row: Hsla,
    row_header: Hsla,
    row_hover: Hsla,
    row_selected: Hsla,
    row_selected_border: Hsla,
    tab: Hsla,
    tab_bar: Hsla,
    tab_segmented: Hsla,
    tab_active: Hsla,
    tab_foreground: Hsla,
    popover: Hsla,
    progress: Hsla,
    slider_bar: Hsla,
    slider_thumb: Hsla,
    switch: Hsla,
    switch_thumb: Hsla,
    scrollbar: Hsla,
    scrollbar_thumb: Hsla,
    scrollbar_thumb_hover: Hsla,
}

impl From<Palette> for ComponentTokens {
    fn from(palette: Palette) -> Self {
        Self {
            canvas: palette.background,
            foreground: palette.text,
            border: palette.border,
            input_border: palette.border_strong,
            focus_ring: palette.focus_ring,
            caret: palette.accent,
            selection: palette.accent_wash,
            primary: palette.accent,
            primary_hover: palette.accent_hover,
            primary_pressed: palette.accent_pressed,
            on_primary: palette.on_accent,
            secondary: palette.surface_alt,
            secondary_hover: palette.hover,
            secondary_pressed: palette.active,
            on_secondary: palette.text,
            quiet: palette.disabled,
            quiet_foreground: palette.text_disabled,
            sidebar: palette.sidebar,
            sidebar_border: palette.divider,
            row: palette.surface,
            row_header: palette.surface_alt,
            row_hover: palette.hover,
            row_selected: palette.accent_wash,
            row_selected_border: palette.accent,
            tab: palette.surface,
            tab_bar: palette.surface,
            tab_segmented: palette.surface_alt,
            tab_active: palette.active,
            tab_foreground: palette.muted,
            popover: palette.surface_dialog,
            progress: palette.accent,
            slider_bar: palette.active,
            slider_thumb: palette.accent,
            switch: palette.active,
            switch_thumb: palette.solid,
            scrollbar: palette.background,
            scrollbar_thumb: palette.border_strong,
            scrollbar_thumb_hover: palette.muted,
        }
    }
}

/// Keeps behavior-heavy third-party controls inside GNSScope's visual language.
pub(crate) fn apply_component_theme(theme: &mut Theme, palette: Palette) {
    let tokens = ComponentTokens::from(palette);

    theme.font_family = interface_font_family().into();
    theme.mono_font_family = data_font_family().into();
    theme.radius = px(radius::CONTROL);
    theme.radius_lg = px(radius::RAISED);
    theme.shadow = false;

    theme.background = tokens.canvas;
    theme.foreground = tokens.foreground;
    theme.border = tokens.border;
    theme.input = tokens.input_border;
    theme.ring = tokens.focus_ring;
    theme.caret = tokens.caret;
    theme.selection = tokens.selection;

    theme.primary = tokens.primary;
    theme.primary_hover = tokens.primary_hover;
    theme.primary_active = tokens.primary_pressed;
    theme.primary_foreground = tokens.on_primary;
    theme.accent = tokens.secondary_hover;
    theme.accent_foreground = tokens.on_secondary;

    theme.secondary = tokens.secondary;
    theme.secondary_hover = tokens.secondary_hover;
    theme.secondary_active = tokens.secondary_pressed;
    theme.secondary_foreground = tokens.on_secondary;
    theme.muted = tokens.quiet;
    theme.muted_foreground = tokens.quiet_foreground;

    theme.sidebar = tokens.sidebar;
    theme.sidebar_border = tokens.sidebar_border;
    theme.sidebar_foreground = tokens.foreground;
    theme.sidebar_accent = tokens.secondary_pressed;
    theme.sidebar_accent_foreground = tokens.foreground;
    theme.sidebar_primary = tokens.primary;
    theme.sidebar_primary_foreground = tokens.on_primary;

    // `theme.list` became a behavior flag (active highlight) in gpui-component
    // 0.7 and no longer carries a row color; the row palette still reaches the
    // remaining list and table color roles below.
    theme.list_head = tokens.row_header;
    theme.list_hover = tokens.row_hover;
    theme.list_active = tokens.row_selected;
    theme.list_active_border = tokens.row_selected_border;
    theme.table = tokens.row;
    theme.table_head = tokens.row_header;
    theme.table_head_foreground = tokens.tab_foreground;
    theme.table_hover = tokens.row_hover;
    theme.table_active = tokens.row_selected;
    theme.table_active_border = tokens.row_selected_border;
    theme.table_row_border = tokens.border;

    theme.tab = tokens.tab;
    theme.tab_bar = tokens.tab_bar;
    theme.tab_bar_segmented = tokens.tab_segmented;
    theme.tab_active = tokens.tab_active;
    theme.tab_active_foreground = tokens.foreground;
    theme.tab_foreground = tokens.tab_foreground;
    theme.title_bar = tokens.sidebar;
    theme.title_bar_border = tokens.sidebar_border;
    theme.popover = tokens.popover;
    theme.popover_foreground = tokens.foreground;
    theme.progress_bar = tokens.progress;
    theme.slider_bar = tokens.slider_bar;
    theme.slider_thumb = tokens.slider_thumb;
    theme.switch = tokens.switch;
    theme.switch_thumb = tokens.switch_thumb;
    theme.scrollbar = tokens.scrollbar;
    theme.scrollbar_thumb = tokens.scrollbar_thumb;
    theme.scrollbar_thumb_hover = tokens.scrollbar_thumb_hover;
}
