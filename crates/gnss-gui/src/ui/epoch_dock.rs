//! Epoch dock drawing from display-only data and app-owned interaction slots.
#[derive(Clone, Copy)]
pub(crate) enum EpochAction {
    First,
    Previous,
    Next,
    Last,
    ZoomOut,
    ZoomIn,
    Follow,
}
pub(crate) type EpochHandler = std::rc::Rc<dyn Fn(EpochAction, &mut gpui::Window, &mut gpui::App)>;
pub(crate) struct EpochControls {
    pub(crate) epoch_count: usize,
    pub(crate) zoom: usize,
    pub(crate) follow_live: bool,
    pub(crate) can_follow: bool,
}
fn epoch_handler(
    actions: &EpochHandler,
    action: EpochAction,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    let actions = actions.clone();
    move |_, window, cx| actions(action, window, cx)
}
pub(crate) fn controls(
    view: EpochControls,
    actions: EpochHandler,
    localize: super::Localize,
    palette: Palette,
) -> impl IntoElement {
    use crate::visual_system::{ButtonKind, ControlSize, GnssButton};
    use gpui_component::IconName;
    div()
        .w(px(204.))
        .h_full()
        .flex_shrink_0()
        .border_l_1()
        .border_color(palette.border)
        .px(px(10.))
        .py(px(8.))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .h(px(32.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    GnssButton::icon_only(
                        "epoch-first",
                        IconName::ArrowLeft,
                        localize("跳到第一个 epoch", "Jump to first epoch"),
                        palette,
                    )
                    .size(ControlSize::Compact)
                    .disabled(view.epoch_count == 0)
                    .on_click(epoch_handler(&actions, EpochAction::First)),
                )
                .child(
                    GnssButton::icon_only(
                        "epoch-previous",
                        IconName::ChevronLeft,
                        localize("上一个 epoch", "Previous epoch"),
                        palette,
                    )
                    .size(ControlSize::Compact)
                    .disabled(view.epoch_count == 0)
                    .on_click(epoch_handler(&actions, EpochAction::Previous)),
                )
                .child(
                    GnssButton::icon_only(
                        "epoch-next",
                        IconName::ChevronRight,
                        localize("下一个 epoch", "Next epoch"),
                        palette,
                    )
                    .size(ControlSize::Compact)
                    .disabled(view.epoch_count == 0)
                    .on_click(epoch_handler(&actions, EpochAction::Next)),
                )
                .child(
                    GnssButton::icon_only(
                        "epoch-last",
                        IconName::ArrowRight,
                        localize("跳到最后一个 epoch", "Jump to last epoch"),
                        palette,
                    )
                    .size(ControlSize::Compact)
                    .disabled(view.epoch_count == 0)
                    .on_click(epoch_handler(&actions, EpochAction::Last)),
                ),
        )
        .child(div().h(px(1.)).w_full().bg(palette.border))
        .child(
            div()
                .h(px(31.))
                .flex()
                .gap(px(5.))
                .child(
                    div()
                        .w(px(82.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded(px(2.))
                        .border_1()
                        .border_color(palette.border)
                        .child(
                            GnssButton::new("timeline-zoom-out", "−", palette)
                                .h_full()
                                .w(px(26.))
                                .flex_shrink_0()
                                .kind(ButtonKind::Ghost)
                                .size(ControlSize::Compact)
                                .rounded(px(0.))
                                .tooltip(localize("缩小时间轴", "Zoom timeline out"))
                                .disabled(view.zoom == 1)
                                .on_click(epoch_handler(&actions, EpochAction::ZoomOut)),
                        )
                        .child(
                            div()
                                .w(px(28.))
                                .text_center()
                                .font_family(data_font_family())
                                .text_size(px(8.))
                                .text_color(palette.text)
                                .child(format!("{}×", view.zoom)),
                        )
                        .child(
                            GnssButton::new("timeline-zoom-in", "+", palette)
                                .h_full()
                                .w(px(26.))
                                .flex_shrink_0()
                                .kind(ButtonKind::Ghost)
                                .size(ControlSize::Compact)
                                .rounded(px(0.))
                                .tooltip(localize("放大时间轴", "Zoom timeline in"))
                                .disabled(view.zoom == 4)
                                .on_click(epoch_handler(&actions, EpochAction::ZoomIn)),
                        ),
                )
                .child(
                    GnssButton::new("epoch-follow-live", localize("跟随", "Follow"), palette)
                        .h_full()
                        .flex_1()
                        .kind(ButtonKind::Ghost)
                        .size(ControlSize::Compact)
                        .selected(view.follow_live)
                        .icon(IconName::Redo2)
                        .disabled(!view.can_follow)
                        .on_click(epoch_handler(&actions, EpochAction::Follow)),
                ),
        )
}
use crate::visual_system::data_font_family;
use crate::visual_system::Palette;
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Hsla, SharedString};

pub(crate) struct EpochView {
    pub(crate) title: &'static str,
    pub(crate) epoch_number: usize,
    pub(crate) epoch_count: usize,
    pub(crate) cursor_percent: f32,
    pub(crate) markers: Vec<(usize, f32, &'static str, Hsla)>,
    pub(crate) labels: [String; 6],
}

pub(crate) fn render(
    view: EpochView,
    slider: AnyElement,
    controls: AnyElement,
    palette: Palette,
) -> impl IntoElement {
    let EpochView {
        title,
        epoch_number,
        epoch_count,
        cursor_percent,
        markers,
        labels,
    } = view;
    div()
        .flex_shrink_0()
        .h(px(154.))
        .bg(palette.surface)
        .border_t_1()
        .border_color(palette.border)
        .child(
            div()
                .h(px(34.))
                .px(px(14.))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_family(data_font_family())
                        .text_color(palette.accent)
                        .child(format!("{epoch_number} / {epoch_count}")),
                ),
        )
        .child(
            div().h(px(120.)).px(px(12.)).pt(px(7.)).pb(px(9.)).child(
                div()
                    .h_full()
                    .flex()
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.background.opacity(0.16))
                    .child(
                        div().flex_1().min_w_0().px(px(12.)).py(px(8.)).child(
                            div()
                                .relative()
                                .h_full()
                                .bg(palette.surface_alt.opacity(0.18))
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(42.))
                                        .h(px(1.))
                                        .bg(palette.muted.opacity(0.82)),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(48.))
                                        .h(px(1.))
                                        .bg(palette.muted.opacity(0.42)),
                                )
                                .children((0..=50).map(|tick| {
                                    let major = tick % 10 == 0;
                                    div()
                                        .absolute()
                                        .left(gpui::relative(tick as f32 / 50.0))
                                        .top(if major { px(40.) } else { px(45.) })
                                        .w(px(1.))
                                        .h(if major { px(11.) } else { px(4.) })
                                        .bg(if major {
                                            palette.text.opacity(0.58)
                                        } else {
                                            palette.muted.opacity(0.42)
                                        })
                                }))
                                .children(markers.into_iter().map(
                                    |(index, percent, label, color)| {
                                        div()
                                            .id(SharedString::from(format!(
                                                "timeline-marker-{index}"
                                            )))
                                            .absolute()
                                            .left(gpui::relative(percent))
                                            .top(px(4.))
                                            .ml(px(-16.))
                                            .w(px(32.))
                                            .h(px(34.))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_start()
                                            .text_size(px(8.5))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .font_family(data_font_family())
                                            .text_color(color)
                                            .child(label)
                                            .child(
                                                div()
                                                    .mt(px(2.))
                                                    .size(px(6.))
                                                    .rounded_full()
                                                    .bg(color),
                                            )
                                            .child(
                                                div().w(px(1.)).h(px(11.)).bg(color.opacity(0.88)),
                                            )
                                    },
                                ))
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(58.))
                                        .h(px(15.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .font_family(data_font_family())
                                        .text_size(px(9.))
                                        .text_color(palette.muted)
                                        .children(
                                            labels.into_iter().map(|label| div().child(label)),
                                        ),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .left(gpui::relative(cursor_percent))
                                        .top(px(8.))
                                        .bottom_0()
                                        .ml(px(-1.))
                                        .w(px(2.))
                                        .bg(palette.accent)
                                        .child(
                                            div()
                                                .absolute()
                                                .top(px(-10.))
                                                .left(px(-5.))
                                                .text_size(px(11.))
                                                .text_color(palette.accent)
                                                .child("▼"),
                                        ),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top_0()
                                        .h(px(24.))
                                        .flex()
                                        .items_center()
                                        .cursor_ew_resize()
                                        .child(slider),
                                ),
                        ),
                    )
                    .child(controls),
            ),
        )
}
