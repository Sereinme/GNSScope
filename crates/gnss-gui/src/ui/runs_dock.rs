//! Runs dock frame. Row controls are supplied by the application with their callbacks.
use crate::visual_system::{data_font_family, metrics, Palette};
use gpui::prelude::*;
use gpui::{div, px, AnyElement};
use gpui_component::scroll::ScrollableElement;

pub(crate) struct RunSection {
    pub(crate) title: &'static str,
    pub(crate) count: usize,
    pub(crate) rows: Vec<AnyElement>,
    pub(crate) empty: Option<&'static str>,
}
pub(crate) struct RunsView {
    pub(crate) title: &'static str,
    pub(crate) full_width: bool,
    pub(crate) sections: [RunSection; 3],
    pub(crate) footer: String,
}
pub(crate) fn render(
    view: RunsView,
    actions: AnyElement,
    search: AnyElement,
    palette: Palette,
) -> AnyElement {
    div()
        .when(view.full_width, |this| this.w_full().min_w_0())
        .when(!view.full_width, |this| {
            this.w(px(metrics::RUNS_DOCK_WIDTH))
                .min_w(px(metrics::RUNS_DOCK_MIN_WIDTH))
        })
        .h_full()
        .flex_shrink_0()
        .bg(palette.sidebar)
        .border_r_1()
        .border_color(palette.border)
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(metrics::RUNS_HEADER_HEIGHT))
                .flex_shrink_0()
                .px(px(12.))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child(view.title),
                )
                .child(actions),
        )
        .child(
            div()
                .h(px(metrics::RUNS_SEARCH_HEIGHT))
                .flex_shrink_0()
                .px(px(9.))
                .py(px(7.))
                .border_b_1()
                .border_color(palette.border)
                .child(search),
        )
        .child(div().flex_1().min_h_0().overflow_y_scrollbar().children(
            view.sections.into_iter().flat_map(|section| {
                let header =
                    section_header(section.title, section.count, palette).into_any_element();
                let empty = section.empty.map(|text| {
                    div()
                        .px(px(12.))
                        .py(px(10.))
                        .text_size(px(9.))
                        .text_color(palette.muted)
                        .child(text)
                        .into_any_element()
                });
                std::iter::once(header).chain(section.rows).chain(empty)
            }),
        ))
        .child(
            div()
                .h(px(28.))
                .flex_shrink_0()
                .px(px(12.))
                .border_t_1()
                .border_color(palette.border)
                .flex()
                .items_center()
                .text_size(px(9.))
                .text_color(palette.faint)
                .child(view.footer),
        )
        .into_any_element()
}
fn section_header(label: &'static str, count: usize, palette: Palette) -> impl IntoElement {
    div()
        .h(px(30.))
        .px(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .bg(palette.surface)
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(palette.muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(8.))
                .font_family(data_font_family())
                .text_color(palette.muted)
                .child(count.to_string()),
        )
}
