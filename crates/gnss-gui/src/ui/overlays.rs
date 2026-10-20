//! Command-palette overlay (spec §3.1 / issue 16, wired by issue 17).
//!
//! The palette is a floating `surface_raised` layer (radius 8, 1px border)
//! with a search row that keeps focus and result rows whose *keyboard
//! highlight* is drawn independently of the focus ring: a highlighted row
//! shows the hover surface plus a 2px stroke with padding shrunk by the same
//! 2px so content geometry never changes (the `ui/run_rows.rs` technique).
//! Because gpui's per-element interactivity state (`.hover()`/`.focus()`
//! closures) is not inspectable after construction, the row chrome is chosen
//! by static flags (`search_focused`, `highlighted`) so the separation is
//! unit-testable through `.style()` introspection.
//!
//! The ⌘K keybinding, palette open/close state, and command execution live in
//! `app.rs` (app-owned wiring, issue 17 item 1); this module owns the
//! presentation contract and forwards result-row clicks to the app-provided
//! select callback.

use crate::visual_system::{data_font_family, interface_font_family, Palette};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, App, Div, Stateful, Window};
use std::rc::Rc;

// Overlay geometry. `visual_system::metrics` has no command-palette tokens,
// so they stay module-private (noted in issue 16). `OVERLAY_WIDTH` is exposed
// so app-side overlay positioning can center the palette on the same column.
pub(crate) const OVERLAY_WIDTH: f32 = 420.0;
const OVERLAY_RADIUS: f32 = 8.0;
const SEARCH_ROW_HEIGHT: f32 = 34.0;
const RESULT_ROW_HEIGHT: f32 = 32.0;
/// Base row horizontal padding; shrinks by FOCUS_STROKE when the 2px stroke
/// is drawn so highlighted and normal rows keep identical content geometry.
const ROW_PAD_X: f32 = 10.0;
const FOCUS_STROKE: f32 = 2.0;
const RADIUS_SMALL: f32 = 6.0;

const PLACEHOLDER_ZH: &str = "搜索命令或解算任务…";
const PLACEHOLDER_EN: &str = "Search commands or runs…";
const EMPTY_ZH: &str = "无匹配命令";
const EMPTY_EN: &str = "No matching commands";

/// Click/selection callback: receives the index of the clicked row within the
/// *filtered* result list (same order as [`CommandPaletteView::filtered`]).
pub(crate) type CommandPaletteSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// One palette result row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandResult {
    /// Short title shown on the row (command or run name).
    pub(crate) title: String,
    /// Optional right-aligned hint (shortcut or run state).
    pub(crate) hint: Option<&'static str>,
}

/// Immutable palette state for one render. The app owns the underlying
/// inputs/actions; this is a snapshot for presentation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommandPaletteView {
    pub(crate) query: String,
    /// Whether the search row currently owns keyboard focus.
    pub(crate) search_focused: bool,
    /// Keyboard-highlighted result index, if any.
    pub(crate) highlighted: Option<usize>,
    pub(crate) results: Vec<CommandResult>,
    /// Chinese presentation when true (locale of the surrounding page).
    pub(crate) zh: bool,
}

impl CommandPaletteView {
    /// Case-insensitive substring filter over titles. An empty query shows
    /// every result (palette just opened).
    pub(crate) fn filtered(&self) -> Vec<&CommandResult> {
        self.results
            .iter()
            .filter(|result| matches_query(&self.query, &result.title))
            .collect()
    }
}

/// The one query-matching rule shared by the overlay's `filtered()` and the
/// app-side highlighted/clicked index mapping (issue 17 item 1), so keyboard
/// and pointer selection always resolve to the same row.
pub(crate) fn matches_query(query: &str, title: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || title.to_lowercase().contains(&query)
}

/// The floating layer: `surface_raised`, radius 8, 1px border (§3.1).
/// `on_select` receives the clicked row's index within the filtered list.
pub(crate) fn command_palette(
    view: &CommandPaletteView,
    palette: Palette,
    on_select: CommandPaletteSelect,
) -> Stateful<Div> {
    let results = view.filtered();
    div()
        .id("command-palette")
        .w(px(OVERLAY_WIDTH))
        .flex_col()
        .rounded(px(OVERLAY_RADIUS))
        .bg(palette.surface_raised)
        .border_1()
        .border_color(palette.border_strong)
        // The stronger floating shadow belongs to the one layer that truly
        // floats above everything; panels use the softer shared shadow.
        .shadow(crate::visual_system::floating_shadow(palette))
        .text_color(palette.text)
        .font_family(interface_font_family())
        .child(search_row(view, palette))
        .child(div().h(px(1.)).bg(palette.border))
        .child(if results.is_empty() {
            empty_state(view, palette)
        } else {
            div().flex_col().children(
                results.iter().enumerate().map(|(index, result)| {
                    result_row(index, result, view, palette, on_select.clone())
                }),
            )
        })
}

/// Search row: keeps the focus ring while typing (`search_focused`), which
/// is separate from any result-row keyboard highlight.
fn search_row(view: &CommandPaletteView, palette: Palette) -> Div {
    let text = if view.query.is_empty() {
        if view.zh {
            PLACEHOLDER_ZH
        } else {
            PLACEHOLDER_EN
        }
    } else {
        &view.query
    };
    let color = if view.query.is_empty() {
        palette.faint
    } else {
        palette.text
    };
    div()
        .h(px(SEARCH_ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(ROW_PAD_X))
        .text_size(px(11.))
        .text_color(color)
        .when(view.search_focused, |this| {
            this.border_2().border_color(palette.focus_ring)
        })
        .child(text.to_string())
}

/// One result. `highlighted` (keyboard) draws the hover surface + 2px
/// stroke; non-highlighted rows are chrome-free until pointer hover. The
/// stroke is paired with a 2px padding reduction so text never shifts.
/// Clicking a row forwards its filtered index to the app's select callback.
fn result_row(
    index: usize,
    result: &CommandResult,
    view: &CommandPaletteView,
    palette: Palette,
    on_select: CommandPaletteSelect,
) -> Stateful<Div> {
    let highlighted = view.highlighted == Some(index);
    let row = div()
        .id(format!("command-result-{index}"))
        .h(px(RESULT_ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(RADIUS_SMALL))
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(palette.text)
        .font_family(data_font_family())
        .on_click(move |_, window, cx| on_select(index, window, cx));
    let row = if highlighted {
        row.bg(palette.hover)
            .border_2()
            .border_color(palette.focus_ring)
            .px(px(ROW_PAD_X - FOCUS_STROKE))
    } else {
        row.px(px(ROW_PAD_X))
            .hover(move |style| style.bg(palette.hover))
    };
    row.child(result.title.clone())
        .children(result.hint.map(|hint| {
            div()
                .ml_auto()
                .text_size(px(10.))
                .text_color(palette.faint)
                .child(hint)
        }))
}

/// Honest empty state (never fabricated results).
fn empty_state(view: &CommandPaletteView, palette: Palette) -> Div {
    div()
        .h(px(RESULT_ROW_HEIGHT))
        .flex()
        .items_center()
        .px(px(ROW_PAD_X))
        .text_size(px(11.))
        .text_color(palette.faint)
        .child(if view.zh { EMPTY_ZH } else { EMPTY_EN })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette::dark()
    }

    fn view(highlighted: Option<usize>) -> CommandPaletteView {
        CommandPaletteView {
            query: String::new(),
            search_focused: true,
            highlighted,
            results: vec![
                CommandResult {
                    title: "打开历史解算任务".to_string(),
                    hint: Some("↵"),
                },
                CommandResult {
                    title: "重新采集 Run09".to_string(),
                    hint: None,
                },
            ],
            zh: true,
        }
    }

    fn style_of(mut element: impl Styled) -> gpui_kit::StyleRefinement {
        element.style().clone()
    }

    fn no_op_select() -> CommandPaletteSelect {
        Rc::new(|_, _, _| {})
    }

    #[test]
    fn overlay_shell_surface_raised_radius_8_single_soft_shadow() {
        let palette = palette();
        let view = view(Some(0));
        let mut overlay = command_palette(&view, palette, no_op_select());
        let style = overlay.style();
        assert_eq!(
            style.corner_radii.top_left,
            Some(gpui_kit::AbsoluteLength::Pixels(px(OVERLAY_RADIUS)))
        );
        assert_eq!(OVERLAY_RADIUS, 8.0);
        assert_eq!(style.background, Some(palette.surface_raised.into()));
        // The palette is the only floating layer in the app, so it owns the
        // single soft shadow; panes, rows and controls stay flat.
        let shadows = style.box_shadow.clone().unwrap_or_default();
        assert_eq!(shadows.len(), 1, "exactly one shadow in the system");
        assert!(shadows[0].blur_radius > px(16.));
        assert!(!shadows[0].inset);
        assert!(view.filtered().len() == 2);
    }

    #[test]
    fn keyboard_highlight_and_focus_are_separate_styles() {
        let palette = palette();
        // Keyboard-highlighted row: hover surface + 2px stroke, padding
        // shrunk by the stroke so content geometry is unchanged.
        let mut highlighted = result_row(
            0,
            &view(Some(0)).results[0],
            &view(Some(0)),
            palette,
            no_op_select(),
        );
        let style = highlighted.style();
        assert_eq!(style.background, Some(palette.hover.into()));
        assert_eq!(
            style.border_widths.left,
            Some(gpui_kit::AbsoluteLength::Pixels(px(FOCUS_STROKE)))
        );
        assert_eq!(style.border_color, Some(palette.focus_ring));
        // Padding inset: ROW_PAD_X - FOCUS_STROKE.
        match &style.padding.left {
            Some(gpui_kit::DefiniteLength::Absolute(gpui_kit::AbsoluteLength::Pixels(left))) => {
                assert_eq!(*left, px(ROW_PAD_X - FOCUS_STROKE));
            }
            other => panic!("unexpected padding {other:?}"),
        }

        // Non-highlighted row: no stroke, no background of its own (hover
        // only), full padding. This is the visible separation between the
        // keyboard highlight and the focus ring.
        let plain = result_row(
            0,
            &view(None).results[0],
            &view(None),
            palette,
            no_op_select(),
        );
        let style = style_of(plain);
        assert_eq!(style.background, None);
        assert_eq!(style.border_color, None);
        assert_eq!(style.border_widths.left, None);

        // The focused search row carries the focus ring on its own; a result
        // highlight never appears there.
        let mut search = search_row(&view(Some(0)), palette);
        let style = search.style();
        assert_eq!(style.border_color, Some(palette.focus_ring));
        assert_eq!(
            style.border_widths.top,
            Some(gpui_kit::AbsoluteLength::Pixels(px(FOCUS_STROKE)))
        );
    }

    #[test]
    fn unfocused_search_row_has_no_ring() {
        let palette = palette();
        let mut unfocused = view(Some(0));
        unfocused.search_focused = false;
        let mut search = search_row(&unfocused, palette);
        let style = search.style();
        assert_eq!(style.border_color, None);
        assert_eq!(style.border_widths.top, None);
    }

    #[test]
    fn filtering_and_empty_state_are_honest() {
        let palette = palette();
        let mut view = view(None);
        assert_eq!(view.filtered().len(), 2);

        view.query = "Run09".to_string();
        let filtered = view.filtered();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "重新采集 Run09");

        view.query = "不存在的命令".to_string();
        assert!(view.filtered().is_empty());
        // Empty results render an honest empty message, never placeholder rows.
        let empty = empty_state(&view, palette);
        assert_eq!(style_of(empty).background, None);

        // English locale.
        view.zh = false;
        let mut empty_en = empty_state(&view, palette);
        assert_eq!(empty_en.style().text.color, Some(palette.faint));
    }
}
