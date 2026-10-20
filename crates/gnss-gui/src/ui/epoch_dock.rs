//! Epoch timeline dock: frozen 148px instrument chrome + right transport cluster.
//!
//! The track, axis labels, header legend and minimap drawing live in
//! [`crate::plots::timeline`]; this module only composes the fifth instrument
//! pane (header 28 + well 119) around them from display-only data supplied by
//! `app.rs`. Interaction callbacks and state ownership stay in `app.rs`, which
//! also owns the transparent seek slider element passed in through
//! [`EpochView::seek`].
//!
//! Geometry follows the frozen contract in
//! `.scratch/ui-visual-refactor/issues/13-timeline-rebuild.md` and design
//! spec §3.10:
//!
//! * well = left area (pad 8/12/8/10 + track 85 + axis labels 18)
//!   | 1px divider | right cluster 144 (rows 15/22/22/18/22 + 4x3 gaps + 8px
//!   bottom clearance).
//! * every right-cluster row is flush to the full 144px width.

use crate::plots::timeline::{self, TrackView};
use crate::ui::Localize;
use crate::visual_system::{
    data_font_family, metrics, ButtonKind, ControlSize, GnssButton, Palette,
};
use gpui_kit::component::IconName;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement, FontWeight};

/// Right-cluster row heights (spec §3.10): counter, stepping, zoom, minimap,
/// follow. Sum 99 + 4x3 gaps + 8px bottom clearance = well 119.
const CLUSTER_COUNTER_HEIGHT: f32 = 15.0;
const CLUSTER_CONTROL_HEIGHT: f32 = 22.0;
const CLUSTER_MINIMAP_HEIGHT: f32 = 18.0;
const CLUSTER_ROW_GAP: f32 = 3.0;
/// Precise-jump popover covers counter + gap + stepping grid (15+3+22).
const JUMP_POPOVER_HEIGHT: f32 = 40.0;
/// Zoom -/+ cell width (36/36/1fr across the 144px cluster).
const ZOOM_BUTTON_WIDTH: f32 = 36.0;

#[derive(Clone, Copy)]
pub(crate) enum EpochAction {
    First,
    Previous,
    Next,
    Last,
    ZoomOut,
    ZoomIn,
    /// Reset the timeline zoom back to 1x (the "1x" chip).
    ZoomReset,
    Follow,
    /// Open the precise epoch jump input over the epoch counter.
    PreciseJump,
}

pub(crate) type EpochHandler =
    std::rc::Rc<dyn Fn(EpochAction, &mut gpui_kit::Window, &mut gpui_kit::App)>;

/// Display-only state of the right transport cluster.
pub(crate) struct EpochControls {
    pub(crate) epoch_count: usize,
    pub(crate) zoom: usize,
    pub(crate) follow_live: bool,
    pub(crate) can_follow: bool,
}

/// Everything the dock needs to render one frame. `seek` is the app-owned
/// transparent slider overlay; `jump_input` is the app-owned precise-jump
/// field shown while `jump_editing` is true.
pub(crate) struct EpochView {
    pub(crate) title: &'static str,
    pub(crate) subtitle: String,
    /// 1-based selected epoch number for the counter and the playhead callout.
    /// The epoch total lives in [`EpochControls::epoch_count`].
    pub(crate) epoch_number: usize,
    pub(crate) track: TrackView,
    pub(crate) minimap_envelope: Option<Vec<Option<f32>>>,
    pub(crate) jump_editing: bool,
    pub(crate) jump_input: Option<AnyElement>,
    pub(crate) seek: AnyElement,
    pub(crate) controls: EpochControls,
    pub(crate) actions: EpochHandler,
    pub(crate) localize: Localize,
}

fn epoch_handler(
    actions: &EpochHandler,
    action: EpochAction,
) -> impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static {
    let actions = actions.clone();
    move |_, window, cx| actions(action, window, cx)
}

/// Icon-only 22px transport button filling one grid cell.
fn step_button(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    action: EpochAction,
    disabled: bool,
    actions: &EpochHandler,
    palette: Palette,
) -> GnssButton {
    GnssButton::icon_only(id, icon, tooltip, palette)
        .size(ControlSize::Compact)
        .w_full()
        .h(px(CLUSTER_CONTROL_HEIGHT))
        .disabled(disabled)
        .on_click(epoch_handler(actions, action))
}

/// "1x" zoom reset chip: a square cell of the same divided group as the zoom
/// buttons, data 12, hover tint. It reads as a value, not a second button.
fn zoom_reset_chip(zoom: usize, actions: &EpochHandler, palette: Palette) -> AnyElement {
    div()
        .id("timeline-zoom-reset")
        .h(px(CLUSTER_CONTROL_HEIGHT))
        .flex_1()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(palette.surface_alt)
        .font_family(data_font_family())
        .text_size(px(12.))
        .text_color(if zoom == 1 {
            palette.faint
        } else {
            palette.muted
        })
        .cursor_pointer()
        .hover(|style| style.bg(palette.hover))
        .child(format!("{zoom}\u{d7}"))
        .on_click(epoch_handler(actions, EpochAction::ZoomReset))
        .into_any_element()
}

/// Precise jump entry: covers counter + stepping rows while editing so the
/// app-owned single-line field owns the row instead of the counter.
fn jump_popover(input: Option<AnyElement>, palette: Palette) -> AnyElement {
    div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w_full()
        .h(px(JUMP_POPOVER_HEIGHT))
        .px(px(4.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(palette.accent)
        .bg(palette.surface_raised)
        .occlude()
        .child(
            div()
                .w_full()
                .h(px(30.))
                .flex()
                .items_center()
                .child(input.unwrap_or_else(|| div().into_any_element())),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn cluster(
    track: &TrackView,
    envelope: Option<&[Option<f32>]>,
    epoch_number: usize,
    controls: &EpochControls,
    jump_editing: bool,
    jump_input: Option<AnyElement>,
    actions: &EpochHandler,
    localize: Localize,
    palette: Palette,
) -> AnyElement {
    let no_epochs = controls.epoch_count == 0;
    div()
        .relative()
        .w(px(metrics::TIMELINE_TRANSPORT_WIDTH))
        .flex_none()
        .mr(px(14.))
        .mb(px(8.))
        .ml(px(8.))
        .flex()
        .flex_col()
        .gap(px(CLUSTER_ROW_GAP))
        // Row 1: accent epoch counter, centered across the full cluster;
        // clicking it opens the precise jump input.
        .child(
            div()
                .id("epoch-precise-jump")
                .h(px(CLUSTER_COUNTER_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .font_family(data_font_family())
                .text_size(px(12.))
                .text_color(if no_epochs {
                    palette.faint
                } else {
                    palette.accent
                })
                .cursor_pointer()
                .hover(|style| style.bg(palette.hover))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(if no_epochs {
                                    palette.faint
                                } else {
                                    palette.accent
                                })
                                .child(format!("{epoch_number}")),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.faint)
                                .child(format!("/ {}", controls.epoch_count)),
                        ),
                )
                .on_click(epoch_handler(actions, EpochAction::PreciseJump)),
        )
        // Row 2: 4-way stepping grid, 4 equal cells with 4px gaps (4x33px at
        // the reference width), icons centered.
        .child(
            // A segmented transport group: one hairline box divided into four
            // equal cells, matching the zoom row below it so the right cluster
            // reads as two aligned instrument rows.
            div()
                .h(px(CLUSTER_CONTROL_HEIGHT))
                .flex_shrink_0()
                .grid()
                .grid_cols(4)
                .gap(px(1.))
                .bg(palette.border)
                .child(step_button(
                    "epoch-first",
                    IconName::ArrowLeft,
                    localize("跳至起点", "Jump to start"),
                    EpochAction::First,
                    no_epochs,
                    actions,
                    palette,
                ))
                .child(step_button(
                    "epoch-previous",
                    IconName::ChevronLeft,
                    localize("上一历元", "Previous epoch"),
                    EpochAction::Previous,
                    no_epochs,
                    actions,
                    palette,
                ))
                .child(step_button(
                    "epoch-next",
                    IconName::ChevronRight,
                    localize("下一历元", "Next epoch"),
                    EpochAction::Next,
                    no_epochs,
                    actions,
                    palette,
                ))
                .child(step_button(
                    "epoch-last",
                    IconName::ArrowRight,
                    localize("跳至末尾", "Jump to end"),
                    EpochAction::Last,
                    no_epochs,
                    actions,
                    palette,
                )),
        )
        // Row 3: zoom -/+ (36px cells aligned with the stepping grid) and the
        // full remainder as the 1x reset chip.
        .child(
            div()
                .h(px(CLUSTER_CONTROL_HEIGHT))
                .flex_shrink_0()
                .flex()
                .gap(px(1.))
                .bg(palette.border)
                .child(
                    GnssButton::new("timeline-zoom-out", "\u{2212}", palette)
                        .size(ControlSize::Compact)
                        .w(px(ZOOM_BUTTON_WIDTH))
                        .h(px(CLUSTER_CONTROL_HEIGHT))
                        .flex_shrink_0()
                        .tooltip(localize("缩小时间轴", "Zoom timeline out"))
                        .disabled(controls.zoom == 1)
                        .on_click(epoch_handler(actions, EpochAction::ZoomOut)),
                )
                .child(
                    GnssButton::new("timeline-zoom-in", "+", palette)
                        .size(ControlSize::Compact)
                        .w(px(ZOOM_BUTTON_WIDTH))
                        .h(px(CLUSTER_CONTROL_HEIGHT))
                        .flex_shrink_0()
                        .tooltip(localize("放大时间轴", "Zoom timeline in"))
                        .disabled(controls.zoom == 4)
                        .on_click(epoch_handler(actions, EpochAction::ZoomIn)),
                )
                .child(zoom_reset_chip(controls.zoom, actions, palette)),
        )
        // Row 4: real amplitude minimap (or an honest 不可用 hatch).
        .child(
            div()
                .h(px(CLUSTER_MINIMAP_HEIGHT))
                .flex_shrink_0()
                .child(timeline::minimap(track, envelope, localize, palette)),
        )
        // Row 5: follow-live toggle. Active cue is not color-only: the redo
        // icon is present exactly while following.
        .child(
            div().h(px(CLUSTER_CONTROL_HEIGHT)).flex_shrink_0().child(
                GnssButton::new(
                    "epoch-follow-live",
                    localize("跟随实时", "Follow live"),
                    palette,
                )
                .kind(ButtonKind::Secondary)
                .size(ControlSize::Compact)
                .w_full()
                .h(px(CLUSTER_CONTROL_HEIGHT))
                // Following is shown by the redo glyph plus accent text, not by
                // a filled block: the transport stays a neutral instrument.
                .when(controls.follow_live, |button| {
                    button.icon(IconName::Redo2).text_color(palette.accent)
                })
                .disabled(!controls.can_follow)
                .on_click(epoch_handler(actions, EpochAction::Follow)),
            ),
        )
        // Precise jump popover (rendered last so it paints above rows 1-2).
        .children(jump_editing.then(|| jump_popover(jump_input, palette)))
        .into_any_element()
}

/// Compose the timeline instrument. Pure view: no state mutation happens here.
pub(crate) fn render(view: EpochView, palette: Palette) -> impl IntoElement {
    let EpochView {
        title,
        subtitle,
        epoch_number,
        track,
        minimap_envelope,
        jump_editing,
        jump_input,
        seek,
        controls,
        actions,
        localize,
    } = view;
    let envelope = minimap_envelope.as_deref();
    div()
        .flex_shrink_0()
        .h(px(metrics::TIMELINE_HEIGHT))
        .rounded(px(crate::visual_system::radius_panel()))
        .overflow_hidden()
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .flex()
        .flex_col()
        // Header 28 (incl. 1px bottom line): pane title + real summary
        // subtitle on the left, event legend flush right.
        .child(
            div()
                .h(px(metrics::TIMELINE_HEADER_HEIGHT))
                .pl(px(10.))
                .pr(px(14.))
                .flex()
                .items_center()
                .gap(px(6.))
                .bg(palette.surface_alt)
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(13.))
                                .text_color(palette.muted)
                                .child(title),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(palette.faint)
                                .child(subtitle),
                        ),
                )
                .child(div().flex_shrink_0().child(timeline::legend(palette))),
        )
        // Well 119: left area | 1px divider | right cluster 144.
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .pt(px(8.))
                        .pr(px(12.))
                        .pb(px(8.))
                        .pl(px(10.))
                        .child(timeline::track(&track, seek, localize, palette))
                        .child(timeline::axis_labels(&track, palette)),
                )
                .child(div().w(px(1.)).h_full().flex_shrink_0().bg(palette.border))
                .child(cluster(
                    &track,
                    envelope,
                    epoch_number,
                    &controls,
                    jump_editing,
                    jump_input,
                    &actions,
                    localize,
                    palette,
                )),
        )
}
