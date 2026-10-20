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
    /// Workbench tab: no chrome at rest, `text_secondary` label; the selected
    /// tab takes `text_primary` and the caller paints the 2px accent
    /// underline (spec §3.3). A tab must never read as a filled chip.
    Tab,
    /// Quiet on/off toggle (跟随实时, dock toggles): transparent body, and an
    /// active state carried by accent text plus a 1px accent border — no
    /// filled wash, so a persistent toggle never looks like a CTA.
    Toggle,
    Destructive,
    /// Quiet danger action: transparent body, danger foreground, danger-wash
    /// hover. Used for inline cancel/remove icon buttons so destructive
    /// affordances stay calm instead of rendering as solid red blocks
    /// (spec §3.2/§7: no loud pill chrome in dense rows).
    QuietDestructive,
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

    // A tab carries no chrome in either state; selection only raises the
    // label and the caller's underline (handled below, before the shared
    // selected branch).
    if kind == ButtonKind::Tab {
        return ControlVisual {
            background: if state == ControlState::Hovered {
                palette.hover
            } else {
                palette.surface.opacity(0.0)
            },
            foreground: if state == ControlState::Selected {
                palette.text
            } else if state == ControlState::Disabled {
                palette.text_disabled
            } else {
                palette.muted
            },
            border: palette.surface.opacity(0.0),
        };
    }

    if kind == ButtonKind::Toggle {
        let active = state == ControlState::Selected;
        return ControlVisual {
            background: if state == ControlState::Hovered {
                palette.hover
            } else {
                palette.surface.opacity(0.0)
            },
            foreground: if active {
                palette.accent
            } else {
                palette.muted
            },
            border: if active {
                palette.accent.opacity(0.55)
            } else {
                palette.border
            },
        };
    }

    if state == ControlState::Selected {
        // A selected toggle keeps the accent as a hairline identity cue rather
        // than a filled cyan block: cyan is rationed to interaction/selection
        // state, not to control chrome (spec §2.5 / gap 11).
        return ControlVisual {
            background: palette.accent_wash,
            foreground: palette.accent,
            border: palette.accent.opacity(0.45),
        };
    }

    let (background, foreground, border) = match kind {
        // The primary action is a neutral solid, not a saturated accent fill:
        // the interface stays an instrument and cyan keeps its selection
        // meaning (spec §2.5 `solid`/`on_solid`).
        ButtonKind::Primary => (
            match state {
                ControlState::Hovered => palette.solid_hover,
                ControlState::Pressed => palette.solid_pressed,
                _ => palette.solid,
            },
            palette.on_solid,
            palette.solid,
        ),
        ButtonKind::Tab => (
            palette.surface.opacity(0.0),
            palette.muted,
            palette.surface.opacity(0.0),
        ),
        ButtonKind::Toggle => (palette.surface.opacity(0.0), palette.muted, palette.border),
        ButtonKind::Secondary => (
            match state {
                ControlState::Hovered => palette.hover,
                ControlState::Pressed => palette.active,
                _ => palette.surface_alt,
            },
            palette.text,
            if state == ControlState::Hovered {
                palette.border_strong
            } else {
                palette.border
            },
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
        ButtonKind::QuietDestructive => (
            match state {
                ControlState::Hovered => palette.danger_wash,
                ControlState::Pressed => palette.danger_wash.blend(palette.danger.opacity(0.18)),
                _ => palette.surface.opacity(0.0),
            },
            palette.danger,
            palette.surface.opacity(0.0),
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
    trailing_icon: Option<IconName>,
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
            trailing_icon: None,
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
            trailing_icon: None,
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

    /// Select-style trailing affordance (e.g. a chevron): the label becomes a
    /// left-aligned, data-font value that truncates within the button width
    /// while the trailing icon pins to the right edge (spec §3.1 selector
    /// fields).
    pub(crate) fn trailing_icon(mut self, icon: IconName) -> Self {
        self.trailing_icon = Some(icon);
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
        let select_style = self.trailing_icon.is_some();
        let content = div()
            .flex()
            .w_full()
            .items_center()
            .gap(px(6.0))
            .text_color(visual.foreground)
            .text_size(px(self.size.text_size()))
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .when(select_style, |content| content.justify_between())
            .when(!select_style, |content| content.justify_center())
            .children((state != ControlState::Busy).then_some(self.icon).flatten())
            .children(self.label.map(|label| {
                if select_style {
                    // Select-style value slot: data font, left aligned,
                    // truncated inside the field width (spec §3.1).
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(super::primitives::data_font_family())
                        .child(label)
                } else {
                    div().child(label)
                }
            }))
            .children(self.trailing_icon);

        let button = Button::new(self.id)
            .custom(variant)
            // `ButtonCustomVariant` derives its resting fill by mixing the
            // custom color toward the theme's transparent, which washes the
            // fill out entirely on a custom token; set the resting fill
            // explicitly and keep the variant for hover/pressed.
            .bg(visual.background)
            // A tab owns only its underline; giving it the shared 1px frame
            // would paint the caller's underline color on all four sides.
            .when(self.kind != ButtonKind::Tab, |this| {
                this.border_1().border_color(visual.border)
            })
            // Focus is an inset 2px accent ring: it never changes the control's
            // box, so keyboard focus cannot shift any layout (Carbon's rule).
            .when(state == ControlState::Focused, |this| {
                this.shadow(vec![gpui_kit::BoxShadow {
                    color: self.palette.focus_ring,
                    offset: gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(0.)),
                    blur_radius: gpui_kit::px(0.),
                    spread_radius: gpui_kit::px(2.),
                    inset: true,
                }])
            })
            .rounded(px(radius::CONTROL))
            .with_size(Size::Size(px(height)))
            .compact()
            .disabled(state == ControlState::Disabled)
            .loading(state == ControlState::Busy)
            // Selection is painted by GNSScope's own tokens above; passing it
            // to the wrapped button would add a second, library-owned
            // selection chrome (the box that used to surround a selected tab).
            .selected(false)
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
        _ => palette.border,
    };
    div()
        .h(px(30.0))
        .rounded(px(radius::CONTROL))
        .border_1()
        .border_color(border)
        .bg(if state == ControlState::Disabled {
            palette.disabled
        } else {
            palette.surface_alt
        })
        .child(
            Input::new(input)
                .bg(palette.surface.opacity(0.0))
                .border_color(palette.surface.opacity(0.0)),
        )
}

pub(crate) fn gnss_search_input(input: &Entity<InputState>, palette: Palette) -> impl IntoElement {
    div()
        .h(px(30.0))
        .rounded(px(radius::CONTROL))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface_alt)
        .child(
            Input::new(input)
                .prefix(IconName::Search)
                .bg(palette.surface.opacity(0.0))
                .border_color(palette.surface.opacity(0.0)),
        )
}

pub(crate) fn gnss_progress(value: f32, palette: Palette, busy: bool) -> impl IntoElement {
    Progress::new("gnss-progress")
        .h(px(4.0))
        .rounded(px(1.0))
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
