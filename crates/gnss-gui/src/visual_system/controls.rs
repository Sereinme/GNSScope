//! GNSScope-owned compact controls.
//!
//! These primitives keep component states in one place while retaining the
//! behavior-heavy `gpui-component` controls required by ADR-0005. GNSScope
//! owns their composition, tokens, and accessible labels.

use super::primitives::{radius, typography};
use super::semantic::Palette;
use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenu},
    progress::Progress,
    scroll::ScrollableElement as _,
    switch::Switch,
    Disableable as _, IconName, Selectable as _, Sizable as _, Size, StyledExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, Anchor, App, ClickEvent, Context, ElementId, Entity, Hsla, InteractiveElement,
    IntoElement, MouseButton, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window,
};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ControlSize {
    Compact,
    #[default]
    Default,
}

impl ControlSize {
    fn height(self) -> f32 {
        match self {
            Self::Compact => 26.0,
            Self::Default => 32.0,
        }
    }

    fn text_size(self) -> f32 {
        match self {
            Self::Compact => typography::CAPTION,
            Self::Default => typography::METADATA,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ControlState {
    #[default]
    Default,
    Hovered,
    Pressed,
    Focused,
    Selected,
    Disabled,
    Busy,
    Error,
}

impl ControlState {
    pub(crate) fn resolve(
        disabled: bool,
        busy: bool,
        pressed: bool,
        focused: bool,
        hovered: bool,
        selected: bool,
        error: bool,
    ) -> Self {
        if disabled {
            Self::Disabled
        } else if busy {
            Self::Busy
        } else if pressed {
            Self::Pressed
        } else if focused {
            Self::Focused
        } else if hovered {
            Self::Hovered
        } else if selected {
            Self::Selected
        } else if error {
            Self::Error
        } else {
            Self::Default
        }
    }

    fn normalized(self) -> Self {
        Self::resolve(
            self == Self::Disabled,
            self == Self::Busy,
            self == Self::Pressed,
            self == Self::Focused,
            self == Self::Hovered,
            self == Self::Selected,
            self == Self::Error,
        )
    }

    fn blocks_activation(self) -> bool {
        matches!(self, Self::Disabled | Self::Busy)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ButtonKind {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Destructive,
}

#[derive(Clone, Copy)]
struct ControlVisual {
    background: Hsla,
    foreground: Hsla,
    border: Hsla,
}

fn button_visual(kind: ButtonKind, state: ControlState, palette: Palette) -> ControlVisual {
    if state == ControlState::Disabled {
        return ControlVisual {
            background: palette.disabled,
            foreground: palette.text_disabled,
            border: palette.border,
        };
    }

    if state == ControlState::Error {
        return ControlVisual {
            background: palette.danger_wash,
            foreground: palette.danger,
            border: palette.danger,
        };
    }

    if state == ControlState::Selected {
        return ControlVisual {
            background: palette.accent_wash,
            foreground: palette.accent,
            border: palette.accent,
        };
    }

    let (background, foreground, border) = match kind {
        ButtonKind::Primary => (
            match state {
                ControlState::Hovered => palette.accent_hover,
                ControlState::Pressed => palette.accent_pressed,
                _ => palette.accent,
            },
            palette.on_accent,
            palette.accent,
        ),
        ButtonKind::Secondary => (
            match state {
                ControlState::Hovered => palette.hover,
                ControlState::Pressed => palette.active,
                _ => palette.surface_alt,
            },
            palette.text,
            palette.border_strong,
        ),
        ButtonKind::Ghost => (
            match state {
                ControlState::Hovered => palette.hover,
                ControlState::Pressed => palette.active,
                _ => palette.surface.opacity(0.0),
            },
            palette.text,
            if state == ControlState::Focused {
                palette.focus_ring
            } else {
                palette.surface.opacity(0.0)
            },
        ),
        ButtonKind::Destructive => (
            match state {
                ControlState::Hovered => palette.danger.opacity(0.92),
                ControlState::Pressed => palette.danger.opacity(0.84),
                _ => palette.danger,
            },
            palette.on_accent,
            palette.danger,
        ),
    };

    ControlVisual {
        background,
        foreground,
        border: if state == ControlState::Focused {
            palette.focus_ring
        } else {
            border
        },
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

#[derive(IntoElement)]
pub(crate) struct GnssButton {
    id: SharedString,
    palette: Palette,
    style: StyleRefinement,
    label: Option<SharedString>,
    icon: Option<IconName>,
    tooltip: Option<SharedString>,
    kind: ButtonKind,
    size: ControlSize,
    state: ControlState,
    on_click: Option<ClickHandler>,
    menu: Option<MenuBuilder>,
}

impl GnssButton {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        palette: Palette,
    ) -> Self {
        Self {
            id: id.into(),
            palette,
            style: StyleRefinement::default(),
            label: Some(label.into()),
            icon: None,
            tooltip: None,
            kind: ButtonKind::Secondary,
            size: ControlSize::Default,
            state: ControlState::Default,
            on_click: None,
            menu: None,
        }
    }

    pub(crate) fn icon_only(
        id: impl Into<SharedString>,
        icon: IconName,
        accessible_name: impl Into<SharedString>,
        palette: Palette,
    ) -> Self {
        let accessible_name = accessible_name.into();
        Self {
            id: id.into(),
            palette,
            style: StyleRefinement::default(),
            label: None,
            icon: Some(icon),
            tooltip: Some(accessible_name),
            kind: ButtonKind::Ghost,
            size: ControlSize::Default,
            state: ControlState::Default,
            on_click: None,
            menu: None,
        }
    }

    pub(crate) fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub(crate) fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub(crate) fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub(crate) fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub(crate) fn state(mut self, state: ControlState) -> Self {
        self.state = state;
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        if disabled {
            self.state = ControlState::Disabled;
        }
        self
    }

    pub(crate) fn busy(mut self, busy: bool) -> Self {
        if busy {
            self.state = ControlState::Busy;
        }
        self
    }

    pub(crate) fn selected(mut self, selected: bool) -> Self {
        if selected {
            self.state = ControlState::Selected;
        }
        self
    }

    pub(crate) fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub(crate) fn dropdown_menu(
        mut self,
        builder: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(builder));
        self
    }
}

impl Styled for GnssButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for GnssButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.normalized();
        let visual = button_visual(self.kind, state, self.palette);
        let hovered = button_visual(self.kind, ControlState::Hovered, self.palette);
        let pressed = button_visual(self.kind, ControlState::Pressed, self.palette);
        let blocks_activation = state.blocks_activation();
        let height = self.size.height();
        let forced_state = state != ControlState::Default;
        let hover_background = if forced_state {
            visual.background
        } else {
            hovered.background
        };
        let active_background = if forced_state {
            visual.background
        } else {
            pressed.background
        };
        let variant = ButtonCustomVariant::new(cx)
            .color(visual.background)
            .foreground(visual.foreground)
            .hover(hover_background)
            .active(active_background);
        let content = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .text_color(visual.foreground)
            .text_size(px(self.size.text_size()))
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .children((state != ControlState::Busy).then_some(self.icon).flatten())
            .children(self.label);

        let button = Button::new(self.id)
            .custom(variant)
            .border_1()
            .border_color(visual.border)
            .rounded(px(radius::CONTROL))
            .with_size(Size::Size(px(height)))
            .compact()
            .disabled(state == ControlState::Disabled)
            .loading(state == ControlState::Busy)
            .selected(state == ControlState::Selected)
            .refine_style(&self.style)
            .child(content)
            .when_some(self.tooltip, |button, tooltip| button.tooltip(tooltip))
            .when_some(
                self.on_click.filter(|_| !blocks_activation),
                |button, handler| {
                    button.on_click(move |event, window, cx| handler(event, window, cx))
                },
            );

        if let Some(builder) = self.menu.filter(|_| !blocks_activation) {
            button
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                    builder(menu, window, cx)
                })
                .into_any_element()
        } else {
            button.into_any_element()
        }
    }
}

type ToggleHandler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub(crate) struct GnssSwitch {
    id: SharedString,
    palette: Palette,
    checked: bool,
    label: SharedString,
    state: ControlState,
    on_toggle: Option<ToggleHandler>,
}

impl GnssSwitch {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        checked: bool,
        palette: Palette,
    ) -> Self {
        Self {
            id: id.into(),
            palette,
            checked,
            label: label.into(),
            state: ControlState::Default,
            on_toggle: None,
        }
    }

    pub(crate) fn state(mut self, state: ControlState) -> Self {
        self.state = state;
        self
    }

    pub(crate) fn on_toggle(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for GnssSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let state = self.state.normalized();
        let can_activate = !state.blocks_activation() && self.on_toggle.is_some();
        let handler = self.on_toggle.clone();
        let visual_id = SharedString::from(format!("{}-visual", self.id));

        div()
            .id(self.id.clone())
            .h(px(28.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(radius::CONTROL))
            .focus(|style| style.border_2().border_color(self.palette.focus_ring))
            .text_size(px(typography::METADATA))
            .text_color(if state == ControlState::Disabled {
                self.palette.text_disabled
            } else {
                self.palette.text
            })
            .when(can_activate, |this| {
                this.cursor_pointer()
                    .track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if can_activate {
                    // Match the wrapped Button's focus-visible behavior: mouse
                    // activation does not move the keyboard focus indicator.
                    window.prevent_default();
                } else {
                    cx.stop_propagation();
                }
            })
            .when_some(handler, |this, handler| {
                this.on_click(move |_, window, cx| {
                    if can_activate {
                        handler(&!self.checked, window, cx);
                    } else {
                        cx.stop_propagation();
                    }
                })
            })
            .child(
                Switch::new(visual_id)
                    .checked(self.checked)
                    .disabled(state == ControlState::Disabled)
                    .with_size(Size::Small),
            )
            .child(self.label)
    }
}

#[derive(IntoElement)]
pub(crate) struct GnssRow {
    id: SharedString,
    palette: Palette,
    label: SharedString,
    metadata: SharedString,
    state: ControlState,
    badge: Option<(SharedString, BadgeKind)>,
    on_click: Option<ClickHandler>,
}

impl GnssRow {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        metadata: impl Into<SharedString>,
        palette: Palette,
    ) -> Self {
        Self {
            id: id.into(),
            palette,
            label: label.into(),
            metadata: metadata.into(),
            state: ControlState::Default,
            badge: None,
            on_click: None,
        }
    }

    pub(crate) fn state(mut self, state: ControlState) -> Self {
        self.state = state;
        self
    }

    pub(crate) fn badge(mut self, label: impl Into<SharedString>, kind: BadgeKind) -> Self {
        self.badge = Some((label.into(), kind));
        self
    }

    pub(crate) fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for GnssRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let state = self.state.normalized();
        let can_activate = !state.blocks_activation() && self.on_click.is_some();
        let handler = self.on_click.clone();
        let background = match state {
            ControlState::Selected => self.palette.accent_wash,
            ControlState::Pressed => self.palette.active,
            ControlState::Disabled => self.palette.disabled,
            ControlState::Error => self.palette.danger_wash,
            _ => self.palette.surface,
        };
        let foreground = if state == ControlState::Disabled {
            self.palette.text_disabled
        } else {
            self.palette.text
        };
        let border = match state {
            ControlState::Focused => self.palette.focus_ring,
            ControlState::Selected => self.palette.accent,
            ControlState::Error => self.palette.danger,
            _ => self.palette.border,
        };

        div()
            .id(self.id)
            .min_h(px(44.0))
            .w_full()
            .px(px(10.0))
            .py(px(7.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(radius::CONTROL))
            .bg(background)
            .border_1()
            .border_color(border)
            .focus(|style| style.border_2().border_color(self.palette.focus_ring))
            .text_color(foreground)
            .when(can_activate, |this| {
                this.cursor_pointer()
                    .track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    .hover(|style| style.bg(self.palette.hover))
                    .active(|style| style.bg(self.palette.active))
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if can_activate {
                    window.prevent_default();
                } else {
                    cx.stop_propagation();
                }
            })
            .when_some(handler, |this, handler| {
                this.on_click(move |event, window, cx| {
                    if can_activate {
                        handler(event, window, cx);
                    } else {
                        cx.stop_propagation();
                    }
                })
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(typography::METADATA))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(self.label),
                    )
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_size(px(typography::CAPTION))
                            .text_color(if state == ControlState::Disabled {
                                self.palette.text_disabled
                            } else {
                                self.palette.muted
                            })
                            .child(self.metadata),
                    ),
            )
            .children(
                self.badge
                    .map(|(label, kind)| gnss_badge(label, kind, self.palette)),
            )
    }
}

#[derive(IntoElement)]
pub(crate) struct GnssMenuItem {
    id: SharedString,
    palette: Palette,
    label: SharedString,
    shortcut: Option<SharedString>,
    icon: Option<IconName>,
    checked: bool,
    destructive: bool,
    state: ControlState,
    on_click: Option<ClickHandler>,
}

impl GnssMenuItem {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        palette: Palette,
    ) -> Self {
        Self {
            id: id.into(),
            palette,
            label: label.into(),
            shortcut: None,
            icon: None,
            checked: false,
            destructive: false,
            state: ControlState::Default,
            on_click: None,
        }
    }

    pub(crate) fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub(crate) fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub(crate) fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub(crate) fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    pub(crate) fn state(mut self, state: ControlState) -> Self {
        self.state = state;
        self
    }

    pub(crate) fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for GnssMenuItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let state = self.state.normalized();
        let can_activate = !state.blocks_activation() && self.on_click.is_some();
        let handler = self.on_click.clone();
        let foreground = if state == ControlState::Disabled {
            self.palette.text_disabled
        } else if self.destructive {
            self.palette.danger
        } else {
            self.palette.text
        };
        let background = match state {
            ControlState::Selected => self.palette.accent_wash,
            ControlState::Pressed => self.palette.active,
            ControlState::Error => self.palette.danger_wash,
            _ => self.palette.surface_dialog,
        };

        div()
            .id(self.id)
            .h(px(30.0))
            .w_full()
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(7.0))
            .rounded(px(radius::CONTROL))
            .bg(background)
            .text_color(foreground)
            .text_size(px(typography::METADATA))
            .focus(|style| style.border_2().border_color(self.palette.focus_ring))
            .when(can_activate, |this| {
                this.cursor_pointer()
                    .track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    .hover(|style| style.bg(self.palette.hover))
                    .active(|style| style.bg(self.palette.active))
            })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if can_activate {
                    window.prevent_default();
                } else {
                    cx.stop_propagation();
                }
            })
            .when_some(handler, |this, handler| {
                this.on_click(move |event, window, cx| {
                    if can_activate {
                        handler(event, window, cx);
                    } else {
                        cx.stop_propagation();
                    }
                })
            })
            .child(
                div()
                    .w(px(14.0))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .children(self.checked.then_some(IconName::Check))
                    .children((!self.checked).then_some(self.icon).flatten()),
            )
            .child(div().flex_1().min_w_0().child(self.label))
            .children(self.shortcut.map(|shortcut| {
                div()
                    .font_family(super::primitives::data_font_family())
                    .text_size(px(typography::CAPTION))
                    .text_color(self.palette.muted)
                    .child(shortcut)
            }))
    }
}

pub(crate) fn gnss_scroll_area(
    id: impl Into<ElementId>,
    height: f32,
    content: impl IntoElement,
    palette: Palette,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(height))
        .w_full()
        .overflow_y_scrollbar()
        .rounded(px(radius::CONTROL))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface_dialog)
        .child(content)
}

pub(crate) fn gnss_input(
    input: &Entity<InputState>,
    palette: Palette,
    state: ControlState,
) -> impl IntoElement {
    let border = match state {
        ControlState::Error => palette.danger,
        ControlState::Focused => palette.focus_ring,
        ControlState::Disabled => palette.border,
        _ => palette.border_strong,
    };
    div()
        .h(px(32.0))
        .rounded(px(radius::CONTROL))
        .border_1()
        .border_color(border)
        .bg(if state == ControlState::Disabled {
            palette.disabled
        } else {
            palette.surface_raised
        })
        .child(
            Input::new(input)
                .bg(palette.surface.opacity(0.0))
                .border_color(palette.surface.opacity(0.0)),
        )
}

pub(crate) fn gnss_search_input(input: &Entity<InputState>, palette: Palette) -> impl IntoElement {
    div()
        .h(px(32.0))
        .rounded(px(radius::CONTROL))
        .border_1()
        .border_color(palette.border_strong)
        .bg(palette.surface_raised)
        .child(
            Input::new(input)
                .prefix(IconName::Search)
                .bg(palette.surface.opacity(0.0))
                .border_color(palette.surface.opacity(0.0)),
        )
}

pub(crate) fn gnss_progress(value: f32, palette: Palette, busy: bool) -> impl IntoElement {
    Progress::new("gnss-progress")
        .h(px(5.0))
        .rounded(px(2.5))
        .bg(if busy {
            palette.warning
        } else {
            palette.accent
        })
        .value(value.clamp(0.0, 100.0))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BadgeKind {
    Neutral,
    Accent,
    Healthy,
    Warning,
    Danger,
}

pub(crate) fn gnss_badge(
    label: impl Into<SharedString>,
    kind: BadgeKind,
    palette: Palette,
) -> impl IntoElement {
    let (background, foreground) = match kind {
        BadgeKind::Neutral => (palette.unavailable_wash, palette.unavailable),
        BadgeKind::Accent => (palette.accent_wash, palette.accent),
        BadgeKind::Healthy => (palette.healthy_wash, palette.healthy),
        BadgeKind::Warning => (palette.warning_wash, palette.warning),
        BadgeKind::Danger => (palette.danger_wash, palette.danger),
    };
    div()
        .h(px(20.0))
        .px(px(7.0))
        .flex()
        .items_center()
        .rounded(px(radius::CONTROL))
        .bg(background)
        .border_1()
        .border_color(foreground.opacity(0.45))
        .text_size(px(typography::CAPTION))
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .text_color(foreground)
        .child(label.into())
}

pub(crate) fn gnss_divider(palette: Palette) -> impl IntoElement {
    div().w_full().h(px(1.0)).bg(palette.divider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_priority_blocks_disabled_and_busy_controls() {
        assert_eq!(
            ControlState::resolve(true, true, true, true, true, true, true),
            ControlState::Disabled
        );
        assert_eq!(
            ControlState::resolve(false, true, true, true, true, true, true),
            ControlState::Busy
        );
        assert!(ControlState::Disabled.blocks_activation());
        assert!(ControlState::Busy.blocks_activation());
        assert!(!ControlState::Selected.blocks_activation());
    }

    #[test]
    fn button_states_use_distinct_semantic_roles() {
        for palette in [Palette::light(), Palette::dark()] {
            let base = button_visual(ButtonKind::Primary, ControlState::Default, palette);
            let hover = button_visual(ButtonKind::Primary, ControlState::Hovered, palette);
            let pressed = button_visual(ButtonKind::Primary, ControlState::Pressed, palette);
            let disabled = button_visual(ButtonKind::Primary, ControlState::Disabled, palette);
            assert_ne!(base.background, hover.background);
            assert_ne!(hover.background, pressed.background);
            assert_ne!(base.foreground, disabled.foreground);
        }
    }
}
