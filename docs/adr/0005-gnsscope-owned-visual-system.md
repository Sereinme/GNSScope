# ADR-0005: GNSScope-owned visual system over focused GPUI behavior

**Status:** Accepted

GNSScope owns the visual composition of its application shell, Runs dock, selected-run
tabs, inspector, timeline, status strip, data rows, and domain plots using native GPUI
layout and drawing. The application must look and behave like one compact scientific
instrument rather than inherit a general-purpose component library's visual language.

`gpui-component` remains a focused behavior dependency for controls whose input,
focus, keyboard, or platform behavior would be risky to reproduce during the current
migration, including Button, Input, Slider, Switch, Progress, and title-bar support.
Their use is styled and composed by GNSScope surfaces; feature modules must not treat
the library's default component appearance as the product design system. Native GPUI
continues to render GNSS domain plots and linked interaction overlays.

GPUI and `gpui-component` revisions remain pinned together. Any further reduction of
the dependency requires equivalent focus, keyboard, accessibility, and macOS, Windows,
and Linux behavior plus screenshot and interaction verification before replacement.
