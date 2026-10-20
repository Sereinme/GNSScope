//! Run row presentation, driven by immutable display data and a scoped action callback.
//!
//! Rows follow the confirmed mockup (spec §3.2): 50px tall, exactly two
//! compact lines (name + status/metadata) with a 4px honest progress strip
//! between them, bare colored status words (no chips or dots), a
//! middle-ellipsized filename with a full-name tooltip, and a 2px accent
//! left edge on selected/active rows. Cyan rationing per spec §2.5: accent
//! only appears on the processing status word, the processing fill, the
//! processing percentage, and the selection/active edge.
use crate::ui::primitives::{
    middle_ellipsis, progress_strip, StripState, RUN_NAME_COMPACT_MAX_CHARS, RUN_NAME_MAX_CHARS,
};
use crate::visual_system::ContextMenuExt;
use crate::visual_system::{
    data_font_family, interface_font_family, metrics, ButtonKind, ControlSize, GnssButton, Palette,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{menu::PopupMenuItem, Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, App, ClickEvent, Div, Hsla, SharedString, Stateful, Window};
use std::rc::Rc;
use uuid::Uuid;

use super::Localize;
pub(crate) type RunHandler = Rc<dyn Fn(RunAction, &mut Window, &mut App)>;
#[derive(Clone, Copy)]
pub(crate) enum RunAction {
    Select(Uuid),
    ContextSelect(Uuid),
    MoveUp(Uuid),
    MoveDown(Uuid),
    Cancel(Uuid),
    Reveal(Uuid),
    Delete(Uuid),
}

/// Placeholder timestamp shown on queued rows that have not started yet
/// (mockup §3.2 queue rows).
const NOT_STARTED: &str = "--:--:--";

/// Compact product menu content; PopupMenu keeps keyboard navigation, focus,
/// screen-edge positioning and the existing themed floating surface.
pub(crate) fn history_menu_item(
    label: &'static str,
    icon: IconName,
    destructive: bool,
    palette: Palette,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, _| {
        div()
            .id(if destructive {
                "history-delete-content"
            } else {
                "history-reveal-content"
            })
            .h(px(30.))
            .w_full()
            // PopupMenu already provides 8 px horizontal item padding.
            .rounded(px(4.))
            .cursor_pointer()
            .hover(move |style| {
                style.bg(if destructive {
                    palette.danger_wash
                } else {
                    palette.hover
                })
            })
            .active(move |style| {
                style.bg(if destructive {
                    palette.danger_wash
                } else {
                    palette.active
                })
            })
            .flex()
            .items_center()
            .gap(px(8.))
            .font_family(interface_font_family())
            .text_size(px(13.))
            .text_color(if destructive {
                palette.danger
            } else {
                palette.text
            })
            .child(
                Icon::new(icon.clone())
                    .size(px(14.))
                    .text_color(if destructive {
                        palette.danger
                    } else {
                        palette.muted
                    }),
            )
            .child(label)
    })
}
pub(crate) struct RunRowView {
    pub(crate) id: Uuid,
    pub(crate) selected: bool,
    pub(crate) filename: String,
    pub(crate) created_short: String,
    pub(crate) created_long: String,
    pub(crate) elapsed: String,
    pub(crate) progress: f64,
    pub(crate) pvt_epoch_count: usize,
    /// Real total sample count of the source file (issue 17 item 5 / ticket
    /// 14 follow-up 1); `None` renders no 采样 segment — never fabricated.
    pub(crate) samples_total: Option<u64>,
    /// Real source file size in bytes; `None` renders no size segment.
    pub(crate) size_bytes: Option<u64>,
    /// Full source path for the row tooltip (spec §3.2 "全名与路径");
    /// `None` keeps the two-line name + timestamp tooltip.
    pub(crate) source_path: Option<String>,
    pub(crate) status: &'static str,
    pub(crate) status_color: Hsla,
    pub(crate) attention: bool,
}
fn on_action(
    actions: &RunHandler,
    action: RunAction,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let actions = actions.clone();
    move |_, window, cx| actions(action, window, cx)
}

/// Shared row chrome: 50px box, 12px content inset (2px reserved left edge
/// and 10px padding), selection surface, hover surface, keyboard highlight
/// and a tooltip carrying the full untruncated filename, the full creation
/// timestamp (`created_long`), the complete metadata readout and — when the
/// app resolved it — the full source path (spec §3.2 "全名与路径", issue 17
/// item 5).
///
/// The keyboard highlight (spec §2.8) is a 2px inner stroke drawn by
/// promoting the border to all four sides while shrinking the padding by
/// the same amount, so focused rows keep identical content geometry.
fn row_tooltip(run: &RunRowView, localize: Localize) -> String {
    let mut lines = vec![
        run.filename.clone(),
        run.created_long.clone(),
        detail_readout(run, localize),
    ];
    if let Some(path) = run.source_path.as_ref().filter(|path| !path.is_empty()) {
        lines.push(path.clone());
    }
    lines.join("\n")
}

fn row_shell(
    id: SharedString,
    run: &RunRowView,
    edge: bool,
    focusable: bool,
    localize: Localize,
    palette: Palette,
) -> Stateful<Div> {
    let selected = run.selected;
    let tooltip_text = row_tooltip(run, localize);
    div()
        .id(id)
        .h(px(metrics::RUN_ROW_HEIGHT))
        .mx(px(6.))
        .rounded(px(crate::visual_system::radius_surface()))
        .cursor_pointer()
        .border_l_2()
        .border_color(if edge {
            palette.accent
        } else {
            palette.sidebar.opacity(0.0)
        })
        .pl(px(9.))
        .pr(px(10.))
        .py(px(4.))
        // A selection pill inset from the dock edges reads as a chosen row;
        // a full-bleed wash reads as a filled table cell (2026-10-20 chassis).
        .bg(if selected {
            palette.accent_wash
        } else {
            palette.sidebar.opacity(0.0)
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(palette.hover))
                .active(|style| style.bg(palette.active))
        })
        .when(focusable, |this| {
            this.tab_index(0).focus(move |style| {
                let style = style
                    .border_2()
                    .border_color(palette.focus_ring)
                    .py(px(2.))
                    .pr(px(10.));
                if selected {
                    style
                } else {
                    style.bg(palette.hover)
                }
            })
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
}

/// Line 1 (18px): middle-ellipsized filename + processing percentage.
/// The percent slot stays empty for every non-processing state, so a failed
/// run can never render "100%" (spec §2.7/§3.2).
fn name_line(run: &RunRowView, state: StripState, max_chars: usize, palette: Palette) -> Div {
    div()
        .h(px(18.))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                // Single-line slot: without the explicit nowrap the name
                // wraps and the fixed 18px row hard-clips the second line
                // mid-glyph (the stuck/half-rendered text in the dock).
                .truncate()
                .text_size(px(12.))
                .font_family(data_font_family())
                .text_color(palette.text)
                .child(middle_ellipsis(&run.filename, max_chars)),
        )
        .children(state.percent_text(run.progress).map(|percent| {
            div()
                .flex_shrink_0()
                .text_size(px(12.))
                .font_family(data_font_family())
                .text_color(palette.accent)
                .child(percent)
        }))
}

/// The 4px strip sits between the two lines with a 2px gap on each side
/// (18 + 8 + 16 = 42px of content inside the 50px row).
fn strip_line(run: &RunRowView, state: StripState, palette: Palette) -> Div {
    // 3px breathing room above and below, and the strip stops short of the
    // row edges so it reads as a measure attached to the name rather than a
    // full-bleed rule under it.
    div()
        .my(px(3.))
        .pr(px(2.))
        .child(progress_strip(state, run.progress, palette))
}

/// Line 2 (16px): bare status word (11/500, semantic color) at the left,
/// the start time right after it, and the elapsed readout pinned right; all
/// faint data 11px. Status and start keep their full width (`flex_shrink_0`)
/// so the start time can never be crushed into a clipped fragment; the
/// readout takes the remaining space and degrades to an ellipsis, with the
/// complete readout on the tooltip (spec §3.2).
/// Status wording is the non-color second cue that pairs with the strip
/// geometry (spec §3.2, §7.5: never a filled chip).
fn status_line(
    status: &'static str,
    status_color: Hsla,
    start: &str,
    end: &str,
    palette: Palette,
) -> Div {
    div()
        .h(px(16.))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(11.))
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(status_color)
                .child(status),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(data_font_family())
                .text_size(px(11.))
                .text_color(palette.faint)
                .child(start.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                // Right-aligned single-line slot: if the readout cannot fit
                // it ends with an ellipsis instead of crushing the start
                // time; the tooltip carries the complete readout.
                .truncate()
                .text_right()
                .font_family(data_font_family())
                .text_size(px(11.))
                .text_color(palette.faint)
                .child(end.to_string()),
        )
}

/// Right-hand readout on the row itself: the elapsed duration only. The
/// counters — PVT epoch count (CONTEXT.md 历元), sample count and source file
/// size — belong to [`detail_readout`] and ride in the hover tooltip, because
/// the 316px dock cannot show them on the row without truncating mid-value
/// (spec §3.2 fixes line 2 to status · start · elapsed).
fn elapsed_readout(run: &RunRowView) -> String {
    run.elapsed.clone()
}

/// Full counter readout used by the tooltip: elapsed, PVT epochs, samples and
/// source size in the Chinese key/value register (gap 10). Missing fields drop
/// their segment; values are never fabricated.
fn detail_readout(run: &RunRowView, localize: Localize) -> String {
    let mut segments = vec![run.elapsed.clone()];
    if run.pvt_epoch_count > 0 {
        segments.push(format!(
            "{} {}",
            localize("历元", "Epochs"),
            run.pvt_epoch_count
        ));
    }
    if let Some(samples) = run.samples_total {
        segments.push(format!(
            "{} {}",
            localize("采样", "Samples"),
            grouped_count(samples)
        ));
    }
    if let Some(size) = run.size_bytes {
        segments.push(file_size_label(size));
    }
    segments.join(" · ")
}

/// Thousands-separated integer for the sample-count readout.
fn grouped_count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Human-readable decimal file size, one fraction digit above KB.
fn file_size_label(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn active_row_element(
    run: RunRowView,
    actions: RunHandler,
    localize: Localize,
    palette: Palette,
) -> Stateful<Div> {
    let id = run.id;
    let state = StripState::from_status(run.status, run.attention);
    // The active run always carries the persistent 2px accent edge.
    row_shell(
        SharedString::from(format!("active-run-{id}")),
        &run,
        true,
        true,
        localize,
        palette,
    )
    .on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
        window.prevent_default();
    })
    .on_click(on_action(&actions, RunAction::Select(id)))
    .child(name_line(&run, state, RUN_NAME_MAX_CHARS, palette))
    .child(strip_line(&run, state, palette))
    .child(status_line(
        run.status,
        run.status_color,
        &run.created_short,
        &elapsed_readout(&run),
        palette,
    ))
}
pub(crate) fn active_row(
    run: RunRowView,
    actions: RunHandler,
    localize: Localize,
    palette: Palette,
) -> impl IntoElement {
    active_row_element(run, actions, localize, palette)
}

fn queue_row_element(
    run: RunRowView,
    index: usize,
    queue_len: usize,
    actions: RunHandler,
    localize: Localize,
    palette: Palette,
) -> Stateful<Div> {
    let id = run.id;
    let selected = run.selected;
    // Queued rows are their own state: 排队 status word in attention amber,
    // an amber-tinted empty track and `--:--:--` placeholders (§2.7).
    let tooltip_text = row_tooltip(&run, localize);
    let content = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(name_line(
            &run,
            StripState::Queued,
            RUN_NAME_COMPACT_MAX_CHARS,
            palette,
        ))
        .child(strip_line(&run, StripState::Queued, palette))
        .child(status_line(
            localize("排队", "Queued"),
            palette.attention,
            NOT_STARTED,
            NOT_STARTED,
            palette,
        ));
    div()
        .id(SharedString::from(format!("queue-run-{id}")))
        .h(px(metrics::RUN_ROW_HEIGHT))
        .px(px(12.))
        .pl(px(10.))
        .cursor_pointer()
        .border_l_2()
        .border_color(if selected {
            palette.accent
        } else {
            palette.sidebar.opacity(0.0)
        })
        .bg(if selected {
            palette.accent_wash
        } else {
            palette.sidebar
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(palette.hover))
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(content)
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .child(
                    GnssButton::icon_only(
                        SharedString::from(format!("dock-up-{id}")),
                        IconName::ChevronUp,
                        localize("上移", "Move up"),
                        palette,
                    )
                    .size(ControlSize::Compact)
                    .disabled(index == 0)
                    .on_click(on_action(&actions, RunAction::MoveUp(id))),
                )
                .child(
                    GnssButton::icon_only(
                        SharedString::from(format!("dock-down-{id}")),
                        IconName::ChevronDown,
                        localize("下移", "Move down"),
                        palette,
                    )
                    .size(ControlSize::Compact)
                    .disabled(index + 1 == queue_len)
                    .on_click(on_action(&actions, RunAction::MoveDown(id))),
                )
                .child(
                    GnssButton::icon_only(
                        SharedString::from(format!("dock-cancel-{id}")),
                        IconName::CircleX,
                        localize("取消排队", "Remove from queue"),
                        palette,
                    )
                    .kind(ButtonKind::QuietDestructive)
                    .size(ControlSize::Compact)
                    .on_click(on_action(&actions, RunAction::Cancel(id))),
                ),
        )
}
pub(crate) fn queue_row(
    run: RunRowView,
    index: usize,
    queue_len: usize,
    actions: RunHandler,
    localize: Localize,
    palette: Palette,
) -> impl IntoElement {
    queue_row_element(run, index, queue_len, actions, localize, palette)
}

fn history_row_element(
    run: &RunRowView,
    actions: &RunHandler,
    localize: Localize,
    palette: Palette,
) -> Stateful<Div> {
    let id = run.id;
    let state = StripState::from_status(run.status, run.attention);
    // History rows only grow the left edge when they are the selected run.
    row_shell(
        SharedString::from(format!("dock-run-{id}")),
        run,
        run.selected,
        true,
        localize,
        palette,
    )
    .on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
        window.prevent_default();
    })
    .on_click(on_action(actions, RunAction::Select(id)))
    .child(name_line(run, state, RUN_NAME_MAX_CHARS, palette))
    .child(strip_line(run, state, palette))
    .child(status_line(
        run.status,
        run.status_color,
        &run.created_short,
        &elapsed_readout(run),
        palette,
    ))
}
pub(crate) fn history_row(
    run: RunRowView,
    actions: RunHandler,
    localize: Localize,
    palette: Palette,
) -> impl IntoElement {
    let id = run.id;
    history_row_element(&run, &actions, localize, palette).context_menu(move |menu, window, cx| {
        actions(RunAction::ContextSelect(id), window, cx);
        menu.min_w(px(208.))
            .max_w(px(240.))
            .item(
                history_menu_item(
                    localize("在 Finder 中显示", "Reveal source file"),
                    IconName::FolderOpen,
                    false,
                    palette,
                )
                .on_click(on_action(&actions, RunAction::Reveal(id))),
            )
            .separator()
            .item(
                history_menu_item(
                    localize("删除运行记录…", "Delete run…"),
                    IconName::Delete,
                    true,
                    palette,
                )
                .on_click(on_action(&actions, RunAction::Delete(id))),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::primitives::STRIP_HEIGHT;

    fn test_actions() -> RunHandler {
        Rc::new(|_, _, _| {})
    }

    fn localize_zh(zh: &'static str, _: &'static str) -> &'static str {
        zh
    }

    fn view(status: &'static str, selected: bool, attention: bool) -> RunRowView {
        let palette = Palette::light();
        RunRowView {
            id: Uuid::nil(),
            selected,
            filename: "2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN_4Msps_Run09.iqrun".to_string(),
            created_short: "14:32:07".to_string(),
            created_long: "2013-04-04  14:32:07".to_string(),
            elapsed: "00:10:05".to_string(),
            progress: 0.63,
            pvt_epoch_count: 1248,
            samples_total: Some(4_000_000),
            size_bytes: Some(16_000_000),
            source_path: None,
            status,
            status_color: palette.text,
            attention,
        }
    }

    fn row_height(style: &gpui_kit::StyleRefinement) -> Option<gpui_kit::Pixels> {
        match style.size.height? {
            gpui_kit::Length::Definite(gpui_kit::DefiniteLength::Absolute(
                gpui_kit::AbsoluteLength::Pixels(height),
            )) => Some(height),
            _ => None,
        }
    }

    fn background_color(style: &gpui_kit::StyleRefinement) -> Option<Hsla> {
        style
            .background
            .as_ref()
            .and_then(gpui_kit::Fill::color)
            .and_then(|background| background.as_solid())
    }

    const ALL_STATES: [(&str, bool); 7] = [
        ("处理中", false),
        ("已完成", false),
        ("失败", false),
        ("排队", false),
        ("已取消", true),
        ("已中断", true),
        ("源不可用", false),
    ];

    #[test]
    fn every_run_state_renders_a_50px_shadowless_row() {
        let palette = Palette::light();
        let actions = test_actions();
        for (status, attention) in ALL_STATES {
            let mut row = history_row_element(
                &view(status, false, attention),
                &actions,
                localize_zh,
                palette,
            );
            let style = row.style();
            assert_eq!(
                row_height(style),
                Some(px(metrics::RUN_ROW_HEIGHT)),
                "{status} row must stay 50px"
            );
            assert!(
                style.box_shadow.as_ref().is_none_or(Vec::is_empty),
                "{status} row must not add shadows"
            );
            assert_eq!(style.border_color, Some(palette.sidebar.opacity(0.0)));
        }
    }

    #[test]
    fn selected_history_row_uses_accent_wash_surface_and_accent_edge() {
        let palette = Palette::light();
        let actions = test_actions();
        let mut row =
            history_row_element(&view("失败", true, false), &actions, localize_zh, palette);
        let style = row.style();
        assert_eq!(background_color(style), Some(palette.accent_wash));
        assert_eq!(style.border_color, Some(palette.accent));
    }

    #[test]
    fn active_row_keeps_accent_edge_even_when_not_selected() {
        let palette = Palette::dark();
        let mut row = active_row_element(
            view("处理中", false, false),
            test_actions(),
            localize_zh,
            palette,
        );
        let style = row.style();
        assert_eq!(style.border_color, Some(palette.accent));
        // Unselected rows are transparent so the dock panel shows through and
        // the inset selection pill is the only painted row surface.
        assert_eq!(background_color(style), Some(palette.sidebar.opacity(0.0)));
        assert_eq!(row_height(style), Some(px(metrics::RUN_ROW_HEIGHT)));
    }

    #[test]
    fn queue_row_is_stable_and_carries_no_focus_or_click_surface() {
        let palette = Palette::light();
        let mut row = queue_row_element(
            view("排队", false, false),
            0,
            2,
            test_actions(),
            localize_zh,
            palette,
        );
        let style = row.style();
        assert_eq!(row_height(style), Some(px(metrics::RUN_ROW_HEIGHT)));
        assert_eq!(style.border_color, Some(palette.sidebar.opacity(0.0)));
        assert_eq!(background_color(style), Some(palette.sidebar));
        assert!(style.box_shadow.as_ref().is_none_or(Vec::is_empty));
        // Strip height token stays at the spec's 4px.
        assert_eq!(STRIP_HEIGHT, 4.0);
    }

    #[test]
    fn filename_line_is_18px_and_uses_the_mockup_ellipsis() {
        let palette = Palette::light();
        let run = view("处理中", false, false);
        let mut line = name_line(
            &run,
            StripState::from_status(run.status, run.attention),
            RUN_NAME_MAX_CHARS,
            palette,
        );
        let style = line.style();
        match style.size.height {
            Some(gpui_kit::Length::Definite(gpui_kit::DefiniteLength::Absolute(
                gpui_kit::AbsoluteLength::Pixels(height),
            ))) => assert_eq!(height, px(18.)),
            other => panic!("unexpected name line height {other:?}"),
        }
        // Mockup contract: head + … + RunNN.iqrun.
        assert_eq!(
            middle_ellipsis(&run.filename, RUN_NAME_MAX_CHARS),
            "2013_04_04…Run09.iqrun"
        );
        // The processing row shows a percentage; a failed row never does.
        assert_eq!(
            StripState::Processing.percent_text(0.63),
            Some("63%".to_string())
        );
        assert_eq!(StripState::Failed.percent_text(1.0), None);
    }

    #[test]
    fn metadata_readout_renders_real_samples_and_size_segments() {
        let run = view("已完成", false, false);
        // Real fields render as key/value segments (issue 17 item 5).
        assert_eq!(
            detail_readout(&run, localize_zh),
            "00:10:05 · 历元 1248 · 采样 4,000,000 · 16.0 MB"
        );

        // Missing fields drop their segment instead of showing 0 or "—".
        let mut sparse = view("已完成", false, false);
        sparse.samples_total = None;
        sparse.size_bytes = None;
        sparse.pvt_epoch_count = 0;
        assert_eq!(detail_readout(&sparse, localize_zh), "00:10:05");

        // English labels localize.
        let english = |_, en: &'static str| en;
        assert_eq!(
            detail_readout(&run, english),
            "00:10:05 · Epochs 1248 · Samples 4,000,000 · 16.0 MB"
        );
    }

    #[test]
    fn sample_and_size_labels_format_honestly() {
        assert_eq!(grouped_count(0), "0");
        assert_eq!(grouped_count(999), "999");
        assert_eq!(grouped_count(1_023), "1,023");
        assert_eq!(grouped_count(4_000_000), "4,000,000");
        assert_eq!(file_size_label(512), "512 B");
        assert_eq!(file_size_label(1_023), "1.0 KB");
        assert_eq!(file_size_label(16_000_000), "16.0 MB");
        assert_eq!(file_size_label(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn tooltip_carries_full_filename_readout_and_path() {
        // Without a resolved path the tooltip keeps name + timestamp + the
        // complete metadata readout (which the fixed 16px line may ellipsize).
        let mut run = view("源不可用", false, false);
        assert_eq!(
            row_tooltip(&run, localize_zh),
            format!(
                "{}\n{}\n{}",
                run.filename, run.created_long, "00:10:05 · 历元 1248 · 采样 4,000,000 · 16.0 MB"
            )
        );

        // With a path it gains the full source path line (spec §3.2).
        run.source_path = Some(
            "/Users/demo/data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN_4Msps_Run09.iqrun".to_string(),
        );
        assert_eq!(
            row_tooltip(&run, localize_zh),
            format!(
                "{}\n{}\n00:10:05 · 历元 1248 · 采样 4,000,000 · 16.0 MB\n/Users/demo/data/2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN_4Msps_Run09.iqrun",
                run.filename, run.created_long
            )
        );

        // An empty path string degrades to the three-line tooltip.
        run.source_path = Some(String::new());
        assert_eq!(
            row_tooltip(&run, localize_zh),
            format!(
                "{}\n{}\n00:10:05 · 历元 1248 · 采样 4,000,000 · 16.0 MB",
                run.filename, run.created_long
            )
        );
    }
}
