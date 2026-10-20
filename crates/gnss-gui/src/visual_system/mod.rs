//! GNSScope-owned visual system.
//!
//! Raw values live in `primitives`, product meaning lives in `semantic`, and
//! concrete control decisions live in `components`. Feature renderers consume
//! semantic roles only; `gpui-component` is contained behind the component
//! adapter in this module.

mod components;
mod context_menu;
mod controls;
mod primitives;
mod semantic;

pub(crate) use components::apply_component_theme;
pub(crate) use context_menu::ContextMenuExt;
pub(crate) use controls::{
    gnss_badge, gnss_divider, gnss_input, gnss_progress, gnss_scroll_area, gnss_search_input,
    BadgeKind, ButtonKind, ControlSize, ControlState, GnssButton, GnssMenuItem, GnssRow,
    GnssSwitch,
};
use gpui_kit::AnyElement;
pub(crate) use primitives::{data_font_family, interface_font_family, metrics};
pub(crate) use semantic::Palette;

/// Floating panel corner radius, exposed so presentation modules can round a
/// region without importing the private primitive module.
pub(crate) fn radius_panel() -> f32 {
    primitives::radius::PANEL
}

/// Nested-surface radius (plot wells, sub-cards, row pills).
pub(crate) fn radius_surface() -> f32 {
    primitives::radius::SURFACE
}

/// The lit 1px top edge inside a floating panel. A single hairline of ink at
/// low alpha reads as a light source and keeps dark panels from looking flat.
pub(crate) fn panel_sheen(palette: Palette) -> AnyElement {
    use gpui_kit::prelude::*;
    gpui_kit::div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(gpui_kit::px(1.))
        .bg(palette.panel_sheen)
        .into_any_element()
}

/// One soft panel shadow shared by every floating region.
pub(crate) fn panel_shadow(palette: Palette) -> Vec<gpui_kit::BoxShadow> {
    vec![gpui_kit::BoxShadow {
        color: palette.shadow,
        offset: gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(3.)),
        blur_radius: gpui_kit::px(12.),
        spread_radius: gpui_kit::px(-3.),
        inset: false,
    }]
}

/// The stronger shadow reserved for overlays that float above everything.
pub(crate) fn floating_shadow(palette: Palette) -> Vec<gpui_kit::BoxShadow> {
    vec![gpui_kit::BoxShadow {
        color: palette.shadow_strong,
        offset: gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(14.)),
        blur_radius: gpui_kit::px(36.),
        spread_radius: gpui_kit::px(-8.),
        inset: false,
    }]
}

#[cfg(test)]
mod tests {
    use super::Palette;
    use gpui_kit::Hsla;

    fn relative_luminance(color: Hsla) -> f32 {
        let rgb = color.to_rgb();
        let linearize = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linearize(rgb.r) + 0.7152 * linearize(rgb.g) + 0.0722 * linearize(rgb.b)
    }

    fn contrast_ratio(foreground: Hsla, background: Hsla) -> f32 {
        let foreground = relative_luminance(foreground);
        let background = relative_luminance(background);
        let lighter = foreground.max(background);
        let darker = foreground.min(background);
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn themes_meet_text_and_control_contrast_targets() {
        for (name, palette) in [("light", Palette::light()), ("dark", Palette::dark())] {
            for (role, foreground, background, minimum) in [
                ("text/surface", palette.text, palette.surface, 7.0),
                ("muted/surface", palette.muted, palette.surface, 4.5),
                ("faint/surface", palette.faint, palette.surface, 4.5),
                ("accent/on-accent", palette.on_accent, palette.accent, 4.5),
                ("solid/on-solid", palette.on_solid, palette.solid, 7.0),
            ] {
                let ratio = contrast_ratio(foreground, background);
                assert!(
                    ratio >= minimum,
                    "{name} {role} contrast {ratio:.2} is below {minimum:.1}"
                );
            }
        }
    }

    #[test]
    fn themes_define_complete_surface_and_overlay_ladders() {
        let light = Palette::light();
        let dark = Palette::dark();

        assert!(relative_luminance(light.background) < relative_luminance(light.surface));
        assert!(relative_luminance(light.sidebar) < relative_luminance(light.surface));
        assert!(relative_luminance(dark.background) < relative_luminance(dark.surface));
        assert!(relative_luminance(dark.surface) < relative_luminance(dark.surface_raised));
        assert!(relative_luminance(dark.surface_raised) <= relative_luminance(dark.surface_dialog));

        for (name, palette) in [("light", light), ("dark", dark)] {
            assert!(
                palette.overlay.a > 0.0 && palette.overlay.a < 1.0,
                "{name} overlay"
            );
            assert!(
                palette.overlay_strong.a > palette.overlay.a && palette.overlay_strong.a < 1.0,
                "{name} strong overlay"
            );
            assert_ne!(palette.disabled, palette.surface);
            assert_ne!(palette.focus_ring, palette.border_strong);
            assert_ne!(palette.plot_grid, palette.plot_cursor);
            assert_ne!(palette.healthy_wash, palette.warning_wash);
            assert_ne!(palette.warning_wash, palette.danger_wash);
            assert!(palette.chart_series.iter().all(Hsla::is_opaque));
        }
    }

    #[test]
    fn feature_renderer_contains_no_raw_product_colors() {
        let source = include_str!("../app.rs");
        for forbidden in ["color(0x", "rgb(0x", "rgba(0x", "hsla(0x"] {
            assert!(
                !source.contains(forbidden),
                "app.rs must use semantic tokens instead of {forbidden}"
            );
        }
    }
}
