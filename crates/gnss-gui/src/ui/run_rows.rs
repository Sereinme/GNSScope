//! Run row presentation, driven by immutable display data and a scoped action callback.
use crate::visual_system::ContextMenuExt;
use crate::visual_system::{
    data_font_family, gnss_progress, interface_font_family, ButtonKind, ControlSize, GnssButton,
    Palette,
};
use gpui_kit::component::{menu::PopupMenuItem, Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, App, ClickEvent, Hsla, SharedString, Window};
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
pub(crate) fn active_row(
    run: RunRowView,
    actions: RunHandler,
    localize: Localize,
    palette: Palette,
) -> impl IntoElement {
    let id = run.id;
    let selected = run.selected;
    let elapsed = &run.elapsed;
    div()
        .id(SharedString::from(format!("active-run-{id}")))
        .tab_index(0)
        .pl(px(12.))
        .pr(px(10.))
        .py(px(8.))
        .cursor_pointer()
        .bg(if selected {
            palette.accent_wash
        } else {
            palette.sidebar
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(palette.hover))
        })
        .border_l_2()
        .border_color(if selected {
            palette.accent
        } else {
            palette.sidebar
        })
        .focus(|style| style.border_color(palette.focus_ring))
        .on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
            window.prevent_default();
        })
        .on_click(on_action(&actions, RunAction::Select(id)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(
                    div()
                        .w(px(8.))
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(palette.healthy)
                        .child("▶"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(11.))
                        .font_family(data_font_family())
                        .text_color(palette.text)
                        .child(run.filename.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_family(data_font_family())
                        .text_color(palette.healthy)
                        .child(format!("{:.0}%", run.progress * 100.0)),
                ),
        )
        .child(
            div()
                .mt(px(6.))
                .flex()
                .items_center()
                .gap(px(7.))
                .child(
                    div()
                        .w(px(48.))
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(palette.healthy)
                        .child(localize("处理中", "Running")),
                )
                .child(div().flex_1().child(gnss_progress(
                    (run.progress * 100.0) as f32,
                    palette,
                    false,
                ))),
        )
        .child(
            div()
                .mt(px(5.))
                .ml(px(13.))
                .flex()
                .items_center()
                .justify_between()
                .font_family(data_font_family())
                .text_size(px(10.))
                .text_color(palette.muted)
                .child(run.created_short.clone())
                .child(format!("{elapsed} elapsed")),
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
    let id = run.id;
    div()
        .min_h(px(42.))
        .px(px(12.))
        .py(px(7.))
        .border_b_1()
        .border_color(palette.border.opacity(0.55))
        .flex()
        .items_center()
        .gap(px(7.))
        .child(div().size(px(6.)).rounded_full().bg(palette.warning))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(11.))
                        .font_family(data_font_family())
                        .text_color(palette.text)
                        .child(run.filename.clone()),
                )
                .child(
                    div()
                        .mt(px(2.))
                        .text_size(px(10.))
                        .text_color(palette.warning)
                        .child(localize("已排队", "Queued")),
                ),
        )
        .child(
            div()
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
                    .kind(ButtonKind::Destructive)
                    .size(ControlSize::Compact)
                    .on_click(on_action(&actions, RunAction::Cancel(id))),
                ),
        )
}
pub(crate) fn history_row(
    run: RunRowView,
    actions: RunHandler,
    localize: Localize,
    palette: Palette,
) -> impl IntoElement {
    let id = run.id;
    let selected = run.selected;
    let (status, status_color) = (run.status, run.status_color);
    let elapsed = &run.elapsed;
    div()
        .id(SharedString::from(format!("dock-run-{id}")))
        .tab_index(0)
        .min_h(px(64.))
        .pl(px(12.))
        .pr(px(10.))
        .py(px(5.5))
        .cursor_pointer()
        .bg(if selected {
            palette.accent_wash
        } else {
            palette.sidebar
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(palette.hover))
        })
        .border_l_2()
        .border_color(if selected {
            palette.accent
        } else {
            palette.sidebar
        })
        .focus(|style| style.border_color(palette.focus_ring))
        .on_mouse_down(gpui_kit::MouseButton::Left, |_, window, _| {
            window.prevent_default();
        })
        .on_click(on_action(&actions, RunAction::Select(id)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(div().size(px(6.)).rounded_full().bg(status_color))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(12.))
                        .font_family(data_font_family())
                        .text_color(palette.text)
                        .child(run.filename.clone()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(status_color)
                        .child(status),
                ),
        )
        .child(
            div()
                .mt(px(5.))
                .ml(px(13.))
                .flex()
                .items_center()
                .gap(px(7.))
                .child(
                    div()
                        .w(px(50.))
                        .flex_shrink_0()
                        .text_size(px(10.))
                        .text_color(status_color)
                        .child(status),
                )
                .child(div().flex_1().child(gnss_progress(
                    (run.progress * 100.0) as f32,
                    palette,
                    run.attention,
                )))
                .child(
                    div()
                        .w(px(30.))
                        .text_right()
                        .font_family(data_font_family())
                        .text_size(px(10.))
                        .text_color(status_color)
                        .child(format!("{:.0}%", run.progress * 100.0)),
                ),
        )
        .child(
            div()
                .mt(px(4.))
                .ml(px(13.))
                .flex()
                .items_center()
                .justify_between()
                .font_family(data_font_family())
                .text_size(px(10.))
                .text_color(palette.muted)
                .child(run.created_long.clone())
                .child(format!("{elapsed} · {} epochs", run.pvt_epoch_count)),
        )
        .context_menu(move |menu, window, cx| {
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
