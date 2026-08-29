# ADR-0004: GPUI Component controls with custom domain plots

**Status:** Superseded by ADR-0005

GNSScope uses `gpui-component` for themes, forms, buttons, menus, progress, drawers, virtualized history lists, tables, and other standard desktop interaction, while native GPUI drawing implements the sky plot, ENU position views, C/N0 bars, velocity/clock charts, and shared epoch cursor. This avoids rebuilding accessible controls but preserves precise GNSS-specific visualization and interaction; compatible GPUI and `gpui-component` revisions must be pinned together because GPUI APIs can drift.
