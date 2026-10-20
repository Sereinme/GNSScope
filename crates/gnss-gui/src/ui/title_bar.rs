//! Native title bar with the quiet command-search field (spec §3.1).
//!
//! The application call site passes a real command `InputState` element (the
//! `command_input` behind the ⌘K palette, wired in issue 17 item 1). That
//! element is rendered inside this module's `command_field` chrome (icon,
//! hover/focus border, right-aligned "⌘K" hint) so the interactive input and
//! the spec's field styling stay separated: presentation here, state in app.

use crate::visual_system::{data_font_family, interface_font_family, metrics, Palette};
use gpui_kit::component::{Icon, IconName, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement, Div, SharedString, Stateful};

// §3.1 command-field geometry. `visual_system::metrics` carries no
// command-field constants and the typography/radius token modules are not
// exported to `ui/`, so these live here as private constants (noted in
// issue 16).
pub(crate) const CMD_FIELD_WIDTH: f32 = 360.0;
pub(crate) const CMD_FIELD_HEIGHT: f32 = 26.0;
pub(crate) const CMD_FIELD_RADIUS: f32 = 0.0;
/// Magnifier icon center inset from the field's left edge (mockup: icon
/// centered at 13px inside the 26px field).
const CMD_ICON_INSET: f32 = 13.0;
/// Icon box: 14px, left edge at inset − half icon = 6px.
const CMD_ICON_SIZE: f32 = 14.0;
const CMD_ICON_LEFT: f32 = CMD_ICON_INSET - CMD_ICON_SIZE / 2.0;
/// Shortcut hint: tick 10px data font, text tertiary, no filled chip (§3.1).
const CMD_HINT: &str = "⌘K / Ctrl+K";
const CMD_HINT_SIZE: f32 = 10.0;
const CMD_HINT_RIGHT_INSET: f32 = 8.0;
/// Placeholder caption 11px text tertiary.
const CMD_PLACEHOLDER_ZH: &str = "搜索命令或解算任务…";
const CMD_PLACEHOLDER_EN: &str = "Search commands or runs…";
const CMD_PLACEHOLDER_SIZE: f32 = 11.0;
const CMD_TEXT_GAP: f32 = 6.0;

fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// Renders the title bar. `command_input` is the real command `InputState`
/// element (issue 17 item 1), hosted inside [`command_field_with`]'s chrome.
/// `run_selector`, when provided (Workspace page), replaces the plain
/// context label + name with the app-built selector field (spec §3.1:
/// ~240px bordered field, middle-ellipsized data-font filename, ▾).
pub(crate) fn render(
    selected_name: SharedString,
    page_context: &'static str,
    run_selector: Option<AnyElement>,
    compact: bool,
    command_input: AnyElement,
    settings_control: AnyElement,
    palette: Palette,
) -> impl IntoElement {
    let zh = has_cjk(page_context);
    TitleBar::new()
        .h(px(metrics::TITLE_BAR_HEIGHT))
        // Carbon UI shell: the header is an inverse bar, not another canvas
        // strip. It is the one place in the app where light text sits on a
        // dark field even in the light theme.
        .bg(palette.header)
        .border_color(palette.header_hairline)
        .text_color(palette.header_text)
        .child(
            div()
                .h_full()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(4.))
                .child(
                    // Typographic wordmark with a single accent hairline: a
                    // small instrument stripe rather than a decorative icon
                    // tile, which read as a second logo (design pass).
                    div()
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .flex_shrink_0()
                        // Mark: two concentric rings (a signal/epoch glyph)
                        // drawn from hairlines — geometric, quiet, and
                        // unmistakably not a second logo tile.
                        .child(
                            div()
                                .relative()
                                .size(px(12.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_1()
                                .border_color(palette.accent)
                                .child(div().size(px(4.)).bg(palette.accent)),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(palette.header_text)
                                .child("GNSScope"),
                        ),
                )
                .child(div().h(px(20.)).w(px(1.)).bg(palette.header_hairline))
                .child(match run_selector {
                    Some(selector) => selector,
                    None => div()
                        .flex()
                        .items_baseline()
                        .gap(px(7.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.))
                                .text_color(palette.header_muted)
                                .child(page_context),
                        )
                        .child(
                            div()
                                .max_w(px(420.))
                                .min_w_0()
                                // Single-line selector name: nowrap keeps the
                                // fixed-height slot from clipping a wrapped
                                // second line mid-glyph.
                                .truncate()
                                .text_size(px(12.))
                                .font_family(data_font_family())
                                .text_color(palette.header_text)
                                .child(selected_name),
                        )
                        .into_any_element(),
                })
                .child(div().flex_1())
                // Command field is centered between two flexible spacers and
                // hidden in the compact layout (title-bar chrome budget). The
                // real command input (issue 17 item 1) is hosted in the field.
                .children((!compact).then(|| command_field_with(zh, palette, Some(command_input))))
                .child(div().flex_1())
                .child(settings_control),
        )
}

/// The §3.1 command-search field: 360×26, radius 6, `surface_alt` fill, 1px
/// border, magnifier at the 13px inset, right-aligned "⌘K / Ctrl+K" tick
/// hint, caption placeholder. Hover strengthens the border; focus draws the
/// 2px ring.
///
/// `input`, when provided (the real command `InputState`, issue 17 item 1),
/// is hosted in place of the static placeholder so the field is genuinely
/// editable while keeping all chrome in this module.
pub(crate) fn command_field_with(
    zh: bool,
    palette: Palette,
    input: Option<AnyElement>,
) -> Stateful<Div> {
    div()
        .id("title-bar-command-field")
        .w(px(CMD_FIELD_WIDTH))
        .h(px(CMD_FIELD_HEIGHT))
        .flex_shrink_0()
        .rounded(px(CMD_FIELD_RADIUS))
        .bg(palette.header_field)
        .border_1()
        .border_color(palette.header_hairline)
        .flex()
        .items_center()
        .pl(px(CMD_ICON_LEFT))
        .pr(px(CMD_HINT_RIGHT_INSET))
        .text_color(palette.header_text)
        .font_family(interface_font_family())
        .hover(move |style| style.border_color(palette.header_muted))
        .tab_index(0)
        .focus(move |style| style.border_2().border_color(palette.focus_ring))
        .child(
            Icon::new(IconName::Search)
                .size(px(CMD_ICON_SIZE))
                .text_color(palette.header_muted),
        )
        // The interactive input fills the remaining width; the static
        // placeholder is only the decoration-only fallback (tests / previews).
        .child(match input {
            Some(input) => div().flex_1().min_w_0().child(input),
            None => placeholder_element(zh, palette),
        })
        .child(div().flex_1())
        .child(hint_element(palette))
}

/// Decoration-only command field (no real input) — used by the tests and any
/// preview path that renders the chrome without application-side state.
#[cfg(test)]
pub(crate) fn command_field(zh: bool, palette: Palette) -> Stateful<Div> {
    command_field_with(zh, palette, None)
}

/// Placeholder: caption 11px text tertiary (spec §3.1).
fn placeholder_element(zh: bool, palette: Palette) -> Div {
    let text = if zh {
        CMD_PLACEHOLDER_ZH
    } else {
        CMD_PLACEHOLDER_EN
    };
    div()
        .ml(px(CMD_TEXT_GAP))
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(CMD_PLACEHOLDER_SIZE))
        .text_color(palette.faint)
        .child(text)
}

/// Shortcut hint: tick 10px data font text tertiary, right aligned, with no
/// filled chip behind it (spec §3.1 / §7).
fn hint_element(palette: Palette) -> Div {
    div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .text_size(px(CMD_HINT_SIZE))
        .font_family(data_font_family())
        .text_color(palette.faint)
        .child(CMD_HINT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette::dark()
    }

    fn style_of(mut element: impl Styled) -> gpui_kit::StyleRefinement {
        element.style().clone()
    }

    #[test]
    fn command_field_geometry_matches_spec() {
        // §3.1 geometry under the Carbon chassis: 360×26, square corners.
        assert_eq!(CMD_FIELD_WIDTH, 360.0);
        assert_eq!(CMD_FIELD_HEIGHT, 26.0);
        assert_eq!(CMD_FIELD_RADIUS, 0.0);
        // Capsule guards (spec §7): height < 28 and radius < 14.
        const { assert!(CMD_FIELD_HEIGHT < 28.0) };
        const { assert!(CMD_FIELD_RADIUS < 14.0) };
        assert_eq!(CMD_ICON_INSET, 13.0);
        assert_eq!(CMD_HINT, "⌘K / Ctrl+K");
        assert_eq!(CMD_PLACEHOLDER_ZH, "搜索命令或解算任务…");
    }

    #[test]
    fn command_field_chrome_header_field_border_no_shadow() {
        let palette = palette();
        let style = style_of(command_field(true, palette));
        match style.size.height {
            Some(gpui_kit::Length::Definite(gpui_kit::DefiniteLength::Absolute(
                gpui_kit::AbsoluteLength::Pixels(height),
            ))) => assert_eq!(height, px(CMD_FIELD_HEIGHT)),
            other => panic!("unexpected field height {other:?}"),
        }
        match style.size.width {
            Some(gpui_kit::Length::Definite(gpui_kit::DefiniteLength::Absolute(
                gpui_kit::AbsoluteLength::Pixels(width),
            ))) => assert_eq!(width, px(CMD_FIELD_WIDTH)),
            other => panic!("unexpected field width {other:?}"),
        }
        // 1px border on all sides, square, header field fill, no shadow.
        for side in [
            style.border_widths.top,
            style.border_widths.right,
            style.border_widths.bottom,
            style.border_widths.left,
        ] {
            assert_eq!(side, Some(gpui_kit::AbsoluteLength::Pixels(px(1.0))));
        }
        assert_eq!(
            style.corner_radii.top_left,
            Some(gpui_kit::AbsoluteLength::Pixels(px(CMD_FIELD_RADIUS)))
        );
        assert_eq!(style.background, Some(palette.header_field.into()));
        assert!(style.box_shadow.as_ref().is_none_or(Vec::is_empty));
    }

    #[test]
    fn hint_and_placeholder_follow_text_roles() {
        let palette = palette();
        let hint = style_of(hint_element(palette));
        assert_eq!(
            hint.text.font_size,
            Some(gpui_kit::AbsoluteLength::Pixels(px(CMD_HINT_SIZE)))
        );
        assert_eq!(hint.text.color, Some(palette.faint));

        let placeholder = style_of(placeholder_element(true, palette));
        assert_eq!(
            placeholder.text.font_size,
            Some(gpui_kit::AbsoluteLength::Pixels(px(CMD_PLACEHOLDER_SIZE)))
        );
        assert_eq!(placeholder.text.color, Some(palette.faint));
        // Hint and placeholder share text tertiary — no colored decoration.
        assert_eq!(hint.text.color, placeholder.text.color);
    }
}
