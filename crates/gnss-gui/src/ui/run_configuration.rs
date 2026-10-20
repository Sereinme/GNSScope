//! Read-only run snapshot presentation. The application supplies all display values.
use crate::visual_system::{data_font_family, metrics, Palette};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement, SharedString};

pub(crate) enum ConfigurationEntry {
    Section(&'static str),
    Row(&'static str, String),
}
pub(crate) struct ConfigurationView {
    pub(crate) title: &'static str,
    /// Quiet caption next to the title (spec §3.9: "运行配置 只读快照").
    pub(crate) subtitle: &'static str,
    /// Plain bottom note (no bordered box), e.g. the immutability reminder.
    pub(crate) footer: Option<&'static str>,
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
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .rounded(px(crate::visual_system::radius_panel()))
        .child(
            div()
                .h(px(38.))
                .px(px(12.))
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(palette.text)
                                .child(view.title),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(palette.muted)
                                .child(view.subtitle),
                        ),
                )
                .child(close_action),
        )
        // The header stays put; only the snapshot entries scroll, so the rail
        // identity never scrolls out of view in a tall configuration.
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .px(px(12.))
                .pb(px(16.))
                .children(view.entries.into_iter().map(|entry| match entry {
                    ConfigurationEntry::Section(label) => {
                        inspector_section(label, palette).into_any_element()
                    }
                    ConfigurationEntry::Row(label, value) => {
                        inspector_row(label, value, palette).into_any_element()
                    }
                }))
                .children(view.footer.map(|footer| {
                    div()
                        .mt(px(12.))
                        .pt(px(8.))
                        .border_t_1()
                        .border_color(palette.border)
                        .text_size(px(11.))
                        .text_color(palette.muted)
                        .child(footer)
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
        .text_size(px(11.))
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(palette.muted)
        .child(label)
}

pub(crate) fn inspector_row(
    label: &'static str,
    value: impl Into<SharedString>,
    palette: Palette,
) -> impl IntoElement {
    div()
        .min_h(px(28.))
        .flex()
        .items_center()
        .child(
            div()
                .w(px(108.))
                .flex_shrink_0()
                .text_size(px(11.))
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                // Single-line value slot (middle-ellipsis + tooltip above
                // carries the full value); nowrap stops the 28px row from
                // clipping a wrapped second line mid-glyph.
                .truncate()
                .text_right()
                .text_size(px(11.))
                .font_family(data_font_family())
                .text_color(palette.text)
                .child(value.into()),
        )
}
