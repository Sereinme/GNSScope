//! Native title bar with app-owned search and settings controls.
use crate::visual_system::{data_font_family, metrics, Palette};
use gpui::prelude::*;
use gpui::{div, px, AnyElement, SharedString};
use gpui_component::{IconName, TitleBar};
pub(crate) fn render(
    selected_name: SharedString,
    page_context: &'static str,
    compact: bool,
    search: AnyElement,
    settings_control: AnyElement,
    palette: Palette,
) -> impl IntoElement {
    TitleBar::new()
        .h(px(metrics::TITLE_BAR_HEIGHT))
        .bg(palette.sidebar)
        .border_color(palette.border)
        .text_color(palette.text)
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
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .flex_shrink_0()
                        .child(
                            div()
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(palette.accent)
                                .child(IconName::Map),
                        )
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(palette.text)
                                .child("GNSScope"),
                        ),
                )
                .child(div().h(px(18.)).w(px(1.)).bg(palette.border))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(7.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(9.))
                                .text_color(palette.muted)
                                .child(page_context),
                        )
                        .child(
                            div()
                                .max_w(px(420.))
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_size(px(11.))
                                .font_family(data_font_family())
                                .text_color(palette.text)
                                .child(selected_name),
                        ),
                )
                .child(div().flex_1())
                .children(
                    (!compact).then(|| div().w(px(360.)).h(px(28.)).flex_shrink_0().child(search)),
                )
                .child(div().flex_1())
                .child(settings_control),
        )
}
