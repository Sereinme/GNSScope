//! Shared plot frame; domain content and focus commands are supplied by the app.
use crate::visual_system::{data_font_family, Palette};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement};
pub(crate) fn render(
    title: AnyElement,
    plot: AnyElement,
    focus_control: AnyElement,
    expanded: bool,
    show_enu: bool,
    palette: Palette,
) -> impl IntoElement {
    div()
        .min_h(if expanded { px(620.) } else { px(310.) })
        .h_full()
        .flex()
        .flex_col()
        .rounded(px(0.))
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .px(px(12.))
        .pt(px(8.))
        .pb(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(title)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .children((show_enu).then(|| {
                            div()
                                .h(px(24.))
                                .px(px(7.))
                                .flex()
                                .items_center()
                                .rounded(px(2.))
                                .border_1()
                                .border_color(palette.border)
                                .font_family(data_font_family())
                                .text_size(px(11.))
                                .text_color(palette.muted)
                                .child("ENU")
                        }))
                        .child(focus_control),
                ),
        )
        .child(
            div()
                .mt(px(6.))
                .flex_1()
                .min_h(if expanded { px(554.) } else { px(264.) })
                .child(plot),
        )
}
