//! Presentation-only global status strip. Values and actions are supplied by the app.
use crate::visual_system::{data_font_family, metrics, Palette};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement, Hsla};

pub(crate) fn render(
    palette: Palette,
    toggle: AnyElement,
    items: impl IntoIterator<Item = (&'static str, String, Hsla)>,
    message: String,
) -> impl IntoElement {
    div()
        .h(px(metrics::STATUS_BAR_HEIGHT))
        .flex_shrink_0()
        .bg(palette.sidebar)
        .border_t_1()
        .border_color(palette.border)
        .flex()
        .items_center()
        .text_size(px(10.))
        .text_color(palette.faint)
        .child(toggle)
        .children(
            items
                .into_iter()
                .map(|(label, value, color)| status_bar_item(label, value, color, palette)),
        )
        .child(div().flex_1())
        .child(
            div()
                .px(px(10.))
                .overflow_hidden()
                .text_ellipsis()
                .font_family(data_font_family())
                .child(message),
        )
}

fn status_bar_item(
    label: &'static str,
    value: String,
    value_color: Hsla,
    palette: Palette,
) -> impl IntoElement {
    div()
        .h_full()
        .px(px(10.))
        .border_l_1()
        .border_color(palette.border)
        .flex()
        .items_center()
        .gap(px(5.))
        .child(
            div()
                .text_size(px(9.))
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(10.))
                .font_family(data_font_family())
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(value_color)
                .child(value),
        )
}
