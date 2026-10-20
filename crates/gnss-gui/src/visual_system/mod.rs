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
pub(crate) use primitives::{data_font_family, interface_font_family, metrics};
pub(crate) use semantic::Palette;

/// Floating panel corner radius, exposed so presentation modules can round a
/// region without importing the private primitive module.
pub(crate) fn radius_panel() -> f32 {
    primitives::radius::PANEL
}

/// The one elevation in the system: a floating overlay that genuinely covers
/// content (the command palette, menus, dialogs). Carbon documents an elevated
/// token for exactly this and nothing else.
pub(crate) fn floating_shadow(palette: Palette) -> Vec<gpui_kit::BoxShadow> {
    vec![gpui_kit::BoxShadow {
        color: palette.shadow_strong,
        offset: gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(6.)),
        blur_radius: gpui_kit::px(18.),
        spread_radius: gpui_kit::px(0.),
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

        // Carbon's field and panel are the same tone — regions are separated
        // by hairlines, not by a surface step — so the ladder asserts the
        // nested steps only: alt is one step away from the field, raised is at
        // or above alt, and the dialog never sits below raised.
        assert_eq!(
            relative_luminance(light.background),
            relative_luminance(light.surface)
        );
        assert_eq!(
            relative_luminance(dark.background),
            relative_luminance(dark.surface)
        );
        assert!(relative_luminance(light.surface_alt) < relative_luminance(light.surface));
        assert!(relative_luminance(dark.surface) < relative_luminance(dark.surface_alt));
        assert!(relative_luminance(dark.surface_alt) <= relative_luminance(dark.surface_raised));

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
