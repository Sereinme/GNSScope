//! Read-only run snapshot presentation. The application supplies all display values.
use crate::visual_system::{data_font_family, metrics, Palette};
use gpui::prelude::*;
use gpui::{div, px, AnyElement, SharedString};
use gpui_component::scroll::ScrollableElement;

pub(crate) enum ConfigurationEntry {
    Section(&'static str),
    Row(&'static str, String),
}
pub(crate) struct ConfigurationView {
    pub(crate) title: &'static str,
    pub(crate) snapshot: String,
    pub(crate) notice: &'static str,
    pub(crate) entries: Vec<ConfigurationEntry>,
}
pub(crate) fn render(
    view: ConfigurationView,
    close_action: AnyElement,
    palette: Palette,
) -> AnyElement {
    div()
        .w(px(metrics::CONFIGURATION_RAIL_WIDTH))
        .min_w(px(metrics::CONFIGURATION_RAIL_MIN_WIDTH))
        .h_full()
        .flex_shrink_0()
        .overflow_y_scrollbar()
        .bg(palette.surface)
        .border_l_1()
        .border_color(palette.border)
        .child(
            div()
                .h(px(54.))
                .px(px(14.))
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(palette.text)
                                .child(view.title),
                        )
                        .child(
                            div()
                                .mt(px(3.))
                                .font_family(data_font_family())
                                .text_size(px(9.))
                                .text_color(palette.muted)
                                .child(view.snapshot),
                        ),
                )
                .child(close_action),
        )
        .child(
            div()
                .px(px(14.))
                .pb(px(16.))
                .child(
                    div()
                        .mt(px(10.))
                        .px(px(8.))
                        .py(px(6.))
                        .bg(palette.healthy.opacity(0.07))
                        .border_1()
                        .border_color(palette.healthy.opacity(0.28))
                        .text_size(px(9.))
                        .text_color(palette.healthy)
                        .child(view.notice),
                )
                .children(view.entries.into_iter().map(|entry| match entry {
                    ConfigurationEntry::Section(label) => {
                        inspector_section(label, palette).into_any_element()
                    }
                    ConfigurationEntry::Row(label, value) => {
                        inspector_row(label, value, palette).into_any_element()
                    }
                })),
        )
        .into_any_element()
}
pub(crate) fn inspector_section(label: &'static str, palette: Palette) -> impl IntoElement {
    div()
        .mt(px(12.))
        .pt(px(8.))
        .border_t_1()
        .border_color(palette.border)
        .text_size(px(10.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(palette.healthy)
        .child(label)
}

pub(crate) fn inspector_row(
    label: &'static str,
    value: impl Into<SharedString>,
    palette: Palette,
) -> impl IntoElement {
    div()
        .min_h(px(26.))
        .flex()
        .items_center()
        .child(
            div()
                .w(px(118.))
                .flex_shrink_0()
                .text_size(px(10.))
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_right()
                .text_size(px(10.))
                .font_family(data_font_family())
                .text_color(palette.text)
                .child(value.into()),
        )
}
