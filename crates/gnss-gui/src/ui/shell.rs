use crate::visual_system::{interface_font_family, Palette};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement};

const COMPACT_BREAKPOINT: f32 = 1_180.0;
const DRAWER_REPLACES_WORKSPACE_BREAKPOINT: f32 = 900.0;
const RUN_CONFIGURATION_BREAKPOINT: f32 = 1_320.0;

/// Presentation-only decisions derived from the current viewport and requested panes.
/// Receiver, run, and persistence state deliberately stay outside this module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShellLayout {
    pub(crate) compact: bool,
    pub(crate) drawer_replaces_workspace: bool,
    pub(crate) runs_dock_replaces_content: bool,
    pub(crate) show_runs_dock: bool,
    pub(crate) show_content: bool,
    pub(crate) show_workspace: bool,
    pub(crate) show_run_configuration: bool,
}

impl ShellLayout {
    pub(crate) fn resolve(
        viewport_width: f32,
        side_drawer_open: bool,
        run_configuration_requested: bool,
        workspace_page: bool,
        runs_dock_requested: bool,
        narrow_runs_open: bool,
    ) -> Self {
        let compact = viewport_width < COMPACT_BREAKPOINT;
        let drawer_replaces_workspace = viewport_width < DRAWER_REPLACES_WORKSPACE_BREAKPOINT;
        let runs_dock_replaces_content = drawer_replaces_workspace;
        Self {
            compact,
            drawer_replaces_workspace,
            runs_dock_replaces_content,
            show_runs_dock: if runs_dock_replaces_content {
                narrow_runs_open
            } else {
                runs_dock_requested
            },
            show_content: !runs_dock_replaces_content || !narrow_runs_open,
            show_workspace: !drawer_replaces_workspace || !side_drawer_open,
            show_run_configuration: run_configuration_requested
                && viewport_width >= RUN_CONFIGURATION_BREAKPOINT
                && !side_drawer_open
                && workspace_page,
        }
    }
}

pub(crate) fn shell_root(
    palette: Palette,
    title_bar: AnyElement,
    body: AnyElement,
    status_bar: AnyElement,
) -> AnyElement {
    div()
        .size_full()
        .bg(palette.canvas)
        .text_color(palette.text)
        .font_family(interface_font_family())
        .flex()
        .flex_col()
        .child(title_bar)
        .child(body)
        .child(status_bar)
        .into_any_element()
}

/// A document surface (New Analysis, Run Library, Interface Settings): one
/// floating panel holding a scrollable page, so documents share the workbench's
/// rounded/shadowed chassis instead of painting straight on the canvas.
pub(crate) fn document_panel(content: AnyElement, palette: Palette) -> AnyElement {
    use gpui_kit::Styled as _;
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .rounded(px(crate::visual_system::radius_panel()))
        .overflow_hidden()
        .shadow(crate::visual_system::panel_shadow(palette))
        .child(crate::visual_system::panel_sheen(palette))
        .child(div().size_full().overflow_y_scrollbar().child(content))
        .into_any_element()
}

/// The window body: floating regions separated by a single gutter, inset from
/// the window canvas so each region reads as a raised surface rather than a
/// tile in a wireframe (2026-10-20 floating chassis).
pub(crate) fn shell_body(runs_dock: Option<AnyElement>, content: Option<AnyElement>) -> AnyElement {
    let gutter = crate::visual_system::metrics::SHELL_GUTTER;
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .gap(px(gutter))
        .px(px(gutter))
        .pb(px(gutter))
        .children(runs_dock)
        .children(content)
        .into_any_element()
}

/// Layout per `design-refresh-spec.md` §8: the timeline spans only the workbench
/// column (between the Runs dock and the side region); the side region — the
/// run-configuration rail or a drawer — is a full-height column the timeline never
/// truncates.
pub(crate) fn workspace_shell(
    workspace: Option<AnyElement>,
    side_region: Option<AnyElement>,
    timeline: Option<AnyElement>,
) -> AnyElement {
    let gutter = crate::visual_system::metrics::SHELL_GUTTER;
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .gap(px(gutter))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(gutter))
                .children(workspace)
                .children(timeline),
        )
        .children(side_region)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::ShellLayout;

    #[test]
    fn shell_breakpoints_collapse_optional_regions_in_order() {
        let reference = ShellLayout::resolve(1_568.0, false, true, true, true, false);
        assert!(!reference.compact);
        assert!(!reference.drawer_replaces_workspace);
        assert!(reference.show_workspace);
        assert!(reference.show_run_configuration);
        assert!(reference.show_runs_dock);
        assert!(reference.show_content);

        let desktop = ShellLayout::resolve(1_440.0, false, true, true, true, false);
        assert!(desktop.show_run_configuration);

        let compact = ShellLayout::resolve(1_100.0, false, true, true, true, false);
        assert!(compact.compact);
        assert!(compact.show_workspace);
        assert!(!compact.show_run_configuration);

        let narrow_drawer = ShellLayout::resolve(800.0, true, true, true, true, false);
        assert!(narrow_drawer.drawer_replaces_workspace);
        assert!(!narrow_drawer.show_workspace);
        assert!(!narrow_drawer.show_run_configuration);
        assert!(!narrow_drawer.show_runs_dock);
        assert!(narrow_drawer.show_content);

        let narrow_runs = ShellLayout::resolve(800.0, false, true, true, true, true);
        assert!(narrow_runs.runs_dock_replaces_content);
        assert!(narrow_runs.show_runs_dock);
        assert!(!narrow_runs.show_content);
    }

    #[test]
    fn exact_thresholds_preserve_desktop_preferences() {
        for (width, configuration, compact, replaces) in [
            (899.0, false, true, true),
            (900.0, false, true, false),
            (1179.0, false, true, false),
            (1180.0, false, false, false),
            (1319.0, false, false, false),
            (1320.0, true, false, false),
        ] {
            for desktop_requested in [false, true] {
                for narrow_open in [false, true] {
                    let layout = ShellLayout::resolve(
                        width,
                        false,
                        true,
                        true,
                        desktop_requested,
                        narrow_open,
                    );
                    assert_eq!(layout.show_run_configuration, configuration);
                    assert_eq!(layout.compact, compact);
                    assert_eq!(layout.runs_dock_replaces_content, replaces);
                    assert_eq!(
                        layout.show_runs_dock,
                        if replaces {
                            narrow_open
                        } else {
                            desktop_requested
                        }
                    );
                    assert_eq!(layout.show_content, !replaces || !narrow_open);
                }
            }
        }
    }

    #[test]
    fn run_configuration_is_workspace_only_and_yields_to_drawers() {
        assert!(
            !ShellLayout::resolve(1_568.0, false, true, false, true, false).show_run_configuration
        );
        assert!(
            !ShellLayout::resolve(1_568.0, true, true, true, true, false).show_run_configuration
        );
        assert!(
            !ShellLayout::resolve(1_568.0, false, false, true, true, false).show_run_configuration
        );
    }
}
