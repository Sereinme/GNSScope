# GNSScope UI Visual Refactor Roadmap

Date: 2026-09-04

Status: Accepted execution plan for Priority 1 in `docs/roadmap.md`

This document is the detailed implementation roadmap for the GNSScope Signal
Workbench. `docs/roadmap.md` remains authoritative for product priority and overall
status; this document owns the UI work breakdown, implementation order, reference-code
policy, and acceptance evidence.

The refactor changes presentation and interaction structure without changing receiver
results, processing semantics, completed-run immutability, or persistence meaning.

## 1. Design authority and reference hierarchy

When two sources disagree, use this order:

1. **GNSScope domain behavior and persisted data contracts.** Receiver state,
   configuration, run identity, epoch synchronization, and available measurements are
   never changed or fabricated to satisfy a visual reference.
2. **Approved concept image:** `docs/Codex 图像 2026年8月29日 22_07_48.png` controls
   the information architecture, region proportions, plot prominence, timeline
   placement, and dense scientific-workbench character, subject to the corrections in
   `docs/ui-architecture.md`.
3. **Waku visual restraint:** compact native desktop density, quiet structural
   controls, system typography, platform-aware sidebar treatment, explicit focus and
   keyboard behavior, and performance-conscious motion.
4. **Comet visual-system rigor:** surface elevation, semantic/component tokens,
   complete light/dark themes, overlay treatment, contrast discipline, and a small
   shared motion catalogue.
5. **Existing GNSScope presentation:** preserve useful behavior, but do not preserve
   legacy page composition or one-off styling that conflicts with items 1-4.

The intended result is:

> The concept image's GNSS information architecture, Waku's restraint and density,
> Comet's token rigor and elevation quality, and GNSScope's own signal semantics.

Neither Waku nor Comet controls GNSScope navigation, terminology, domain plots, run
model, or receiver behavior.

## 2. Non-negotiable product constraints

- GNSScope remains a scientific desktop instrument, not a chat application, generic
  dashboard, IDE clone, or decorative dark-tech demo.
- The next-run draft, active run, and selected run remain distinct identities.
- Selecting history never interrupts, replaces, or mutates the active run.
- Completed runs and their processing snapshots remain immutable; reprocessing creates
  a new run.
- The shared epoch cursor and live-follow behavior remain synchronized across all
  run-local views.
- Receiver data that is not persisted or emitted is shown as unavailable, never
  synthesized for visual completeness.
- Receiver algorithms, event ordering, persisted values, queue semantics, and selected
  PVT outputs must not change as a side effect of UI work.
- No blocking filesystem, process, network, SQLite, or receiver work moves onto the GPUI
  render thread.
- Light and dark themes expose the same controls and semantic information.
- Status never relies on color alone; text, icon, shape, or pattern supplies the second
  cue.
- Mobile layout, live SDR input, multi-signal implementation, and new receiver data are
  outside this roadmap.

## 3. Upstream code-reference policy

### 3.1 Waku

Reviewed upstream: <https://github.com/egoist/waku>

Useful implementation references:

| Concern | Upstream file | What may be studied |
| --- | --- | --- |
| Theme and scalable chrome | `src/theme.rs` | Neutral graphite roles, `sp()`-style UI scaling, semantic sidebar treatment |
| Small controls | `src/ui/mod.rs` | Compact icon buttons, menu chips, toggle geometry, activation behavior |
| Focusable fields and menus | `src/ui/text_field.rs`, `src/ui/menu.rs` | Keyboard/focus ownership, highlighted row versus focused input |
| Motion | `src/ui/motion.rs` | Reduced motion, shared leased pulse clock, reversible panel width animation |
| Runs-like navigation | `src/app/sidebar.rs` | Dense row rhythm, grouping, virtualization, quiet hover/selection |
| Floating selectors | `src/app/composer.rs` | Search-owned focus with separately drawn highlighted results |
| Window integration | `src/app/window_chrome.rs`, `src/platform.rs` | Platform controls and macOS Sidebar material with opaque fallback |

Waku is `GPL-3.0-only`. GNSScope may study behavior, measurements, and architectural
ideas, but must not copy Waku source, assets, or distinctive implementation expression
into a non-GPL codebase. Any Waku-inspired behavior must be independently implemented
against GNSScope's pinned GPUI API.

GNSScope must not adopt Waku's GPUI fork merely to obtain visual behavior. A fork or
new native-layer requirement needs a separate ADR and cross-platform risk review.

### 3.2 Comet

Reviewed upstream: <https://github.com/zeronsh/comet>

Useful implementation references:

| Concern | Upstream file | What may be studied or adapted |
| --- | --- | --- |
| Theme architecture | `crates/ui/src/theme.rs` | Surface ladder, semantic text roles, solid/inverse controls, theme variants, contrast handling |
| Motion catalogue | `crates/ui/src/motion.rs` | Menu/dialog/panel durations, easing, reduced motion, shared pulse scheduling |
| Application boundary | `ARCHITECTURE.md` | Owning the product theme/components above pinned GPUI instead of inheriting another app's UI |

Comet is MIT licensed, so focused reuse is possible when compatible with GNSScope's
GPUI revision. Any copied or substantially adapted code must retain the required
license notice and be recorded in the repository's third-party notices. Prefer a small
independent GNSScope implementation over importing Comet's product-specific component
hierarchy or GPUI fork.

### 3.3 Reference review checklist

Before adapting an upstream implementation:

1. Record the upstream URL, reviewed revision when available, file, behavior being
   studied, and license in the corresponding `.scratch/<feature-slug>/` issue.
2. Verify that the idea fits the current pinned GPUI and `gpui-component` revisions.
3. Extract only the behavior or design principle required by GNSScope.
4. Keep GNSScope naming, domain state, tokens, and accessibility behavior authoritative.
5. Add unit, interaction, and screenshot evidence around the adapted behavior.
6. Do not introduce an upstream dependency solely to reproduce a visual effect.

## 4. Target visual system

### 4.1 Three-layer token model

The current `Palette` is a useful semantic bridge but is not the final token model.
Migrate incrementally to three layers:

1. **Primitive tokens** contain raw graphite, neutral, cyan, green, amber, red, plot,
   spacing, type, radius, border, opacity, and duration values.
2. **Semantic tokens** assign purpose: canvas, sidebar, surface, raised surface,
   overlay, text levels, interaction, focus, selection, receiver health, warning,
   failure, chart grid, chart cursor, and unavailable state.
3. **Component tokens** define exact title-bar, dock, run-row, tab, button, input,
   popover, dialog, table, plot, timeline, status-strip, badge, and tooltip states.

Raw colors and duplicated dimensions must not spread through feature renderers. New
visual values first enter the primitive layer and are consumed through semantic or
component roles.

### 4.2 Required semantic color roles

Both themes must provide at least:

- `canvas`, `sidebar`, `surface`, `surface_alt`, `surface_raised`, `surface_dialog`,
  `overlay`, and `overlay_strong`;
- `border`, `border_strong`, `divider`, and `focus_ring`;
- `text_primary`, `text_secondary`, `text_tertiary`, `text_disabled`, and
  `text_inverse`;
- `interaction`, `interaction_hover`, `interaction_pressed`, `interaction_wash`,
  `selection`, `solid`, and `on_solid`;
- `healthy`, `attention`, `danger`, and `unavailable`, each with foreground and wash
  variants where required;
- `plot_grid`, `plot_axis`, `plot_cursor`, `plot_selection`, and a color-blind-aware
  series palette.

Color semantics:

- cyan: selection, focus, live-follow, active processing, and primary interaction;
- green: valid solution, healthy lock, completed success;
- amber: acquisition, queued work, probation, suspect data, attention;
- red: loss, failure, invalid configuration, destructive confirmation;
- neutral graphite: structure and ordinary controls;
- blue, violet, orange, and additional hues: plot-series identity only, not receiver
  health.

### 4.3 Typography

- Use the platform UI font for Chinese and English interface text.
- Use the platform technical monospace font for PRN/signal identifiers, epochs,
  receiver time, coordinates, frequency, Doppler, phase, C/N0, progress, and aligned
  table values.
- Establish named roles rather than arbitrary sizes: caption, metadata, body, control,
  pane title, run title, and measurement.
- Use approximately 12-14 px for dense desktop chrome; allow 11-12 px only for
  secondary captions with verified contrast.
- Use tabular or monospaced digits for changing values so rows and progress labels do
  not shift.
- Validate Chinese glyph metrics at every supported density and width; never shrink
  type to rescue an overflowing layout.
- A Waku-style UI scale may resize chrome relative to a 14 px baseline, but plot ticks,
  line widths, hit targets, and data density need independent bounds.

### 4.4 Geometry and density

The concept image remains the proportional baseline at `1568x1003`.

- Title bar: approximately 40 px.
- Runs dock: 316 px preferred, 286 px contracted minimum before collapse.
- Run Configuration rail: approximately 276 px at the reference width.
- Pane/tab headers: 30-40 px according to function.
- Status strip: 22-28 px.
- Icon button: 22-24 px with a 13-14 px icon and sufficient combined hit area.
- Compact action/data rows: 28-32 px; run summary rows may use 48-52 px.
- Structural pane radius: 0-4 px.
- Interactive control radius: 4-7 px.
- Raised menu/dialog radius: 8-10 px.
- Avoid 16 px chat bubbles, fully pill-shaped general controls, nested card stacks, and
  large soft dashboard panels.
- Use 4 px for tightly related controls, 8 px as the normal rhythm, and 12-16 px for
  bounded inspector content.

### 4.5 Elevation and material

- Main plots and analysis panes remain opaque and visually stable.
- Separate adjacent work areas primarily with one-pixel dividers and surface changes,
  not shadows.
- Reserve `surface_raised`, `surface_dialog`, and overlay tokens for popovers, menus,
  dialogs, temporary inspectors, and focused plot surfaces.
- A Waku-inspired native macOS Sidebar material may be used only for the Runs dock and
  compatible title-bar region after a feasibility spike. It must preserve contrast and
  have an opaque semantic fallback on Windows, Linux, reduced-transparency systems,
  unsupported GPUI versions, and screenshots requiring determinism.
- Do not apply glass or blur to plots, tables, the timeline, or the complete window.

### 4.6 Motion catalogue

Motion is a shared capability, not per-component decoration.

| Motion | Target | Normal duration | Reduced motion |
| --- | --- | ---: | --- |
| Hover/background fade | buttons, rows, tabs | 120-150 ms | immediate |
| Menu/popover in | contextual overlays | 140 ms | opacity only or immediate |
| Menu/popover out | contextual overlays | 100 ms | immediate |
| Dialog in | settings and confirmations | 180 ms | opacity only or immediate |
| Dock/panel resize | Runs and configuration rail | 180-200 ms | immediate |
| Tab indicator | run-local tabs | 150 ms | immediate |
| Status transition | lifecycle label/icon | 150 ms | immediate |
| Spinner/pulse | indeterminate work only | shared 30 fps clock | static indicator |

Requirements:

- Use a single shared animation clock for repeating progress/pulse visuals.
- The clock must park when no animated element has an active lease.
- Do not schedule frames while the UI is idle.
- Determinate processing progress does not need a permanent spinner.
- Dock animations must be reversible from their current width.
- Avoid page-level entrance animation, glowing charts, continuous gradients, bouncing
  controls, or animation that delays data inspection.

### 4.7 Component states

Every interactive component must define and test:

- default;
- hover;
- pressed/active;
- keyboard focus-visible;
- selected;
- disabled;
- loading/busy where applicable;
- error/invalid where applicable.

State priority is: disabled, busy, pressed, focus-visible, selected, hover, default.

The following identities must remain visually distinct:

- active processing run;
- selected run being inspected;
- keyboard-highlighted row;
- hovered row;
- pinned PRN;
- selected epoch;
- live-follow cursor.

Menus and searchable selectors should follow the Waku-style focus split: the search
field retains keyboard focus while the currently navigable result receives a drawn
highlight. Selection is committed only by activation.

Focus indicators must remain visible in both themes and reach at least 3:1 contrast
against adjacent colors. Normal text targets 4.5:1 contrast. Disabled state must not
be expressed only by lowering opacity below readability.

## 5. Target UI code organization

`crates/gnss-gui/src/app.rs` currently combines application orchestration, state,
workflow commands, shell composition, controls, and domain rendering. The visual
refactor must progressively extract stable modules without rewriting receiver or
persistence code.

Target organization:

```text
crates/gnss-gui/src/
├── app.rs                    # application orchestration and top-level actions
├── visual_system/
│   ├── mod.rs                # public visual-system boundary
│   ├── primitives.rs         # raw color/space/type/radius/duration values
│   ├── semantic.rs           # light/dark semantic themes
│   ├── components.rs         # component-specific tokens
│   ├── motion.rs             # shared motion specs and animation clock
│   └── platform.rs           # optional platform material/chrome adapters
├── ui/
│   ├── mod.rs
│   ├── primitives.rs         # buttons, rows, tabs, badges, dividers, text roles
│   ├── overlays.rs           # menu, popover, tooltip, dialog wrappers
│   ├── shell.rs              # title bar, docks, workbench frame, responsive policy
│   ├── runs_dock.rs
│   ├── analysis_draft.rs
│   ├── workbench.rs
│   ├── run_configuration.rs
│   ├── epoch_dock.rs
│   └── status_strip.rs
└── plots/
    ├── mod.rs
    ├── frame.rs              # axes, grid, title, legend, empty/loading/error states
    ├── sky.rs
    ├── enu.rs
    ├── cn0.rs
    ├── pvt.rs
    └── timeline.rs
```

This is a destination, not a one-shot file move. Each extraction must preserve behavior
and keep reviewable diffs. Existing domain/state modules remain in place unless an
extraction has a direct UI-boundary reason.

`gpui-component` remains behind GNSScope-owned adapters as required by ADR-0005. Feature
views must consume GNSScope primitives rather than style third-party controls directly.

## 6. Implementation phases

### Phase 0 — Baseline, inventory, and evidence freeze

**Goal:** establish a trustworthy starting point before structural edits.

Deliverables:

- Record the behavior-preservation matrix for IQ selection, validation, start, queue,
  cancel, reorder, history, search/filter, reveal, delete, duplicate/reprocess,
  interrupted recovery, theme, language, plots, candidate comparison, dock state,
  shared epoch, live-follow, and linked PRN selection.
- Capture the current deterministic screenshots for desktop, compact, and narrow
  layouts in both themes, including concept-parity preview data.
- Record which visual values are already centralized and which remain hardcoded in
  `app.rs` or plot renderers.
- Record baseline GUI test, formatting, Clippy, packaging, and deterministic receiver
  results before visual refactoring.
- Create scoped `.scratch/<feature-slug>/` issues for each later phase; do not turn one
  issue into a multi-month omnibus change.

Gate:

- Every existing user-visible behavior has a destination and regression method.
- Baseline screenshots and command outputs are available for comparison.
- Pre-existing worktree changes are identified and preserved.

### Phase 1 — Token foundation and third-party theme containment

**Goal:** make visual decisions centralized and theme-safe before changing more views.

Deliverables:

- Split raw values, semantic roles, and component roles as described in Section 4.1.
- Complete both light and dark surface ladders; light mode must be designed rather than
  generated by inversion.
- Add `solid/on_solid`, overlay, dialog, focus, disabled, plot-grid, plot-cursor, and
  status wash roles missing from the current `Palette`.
- Route the `gpui-component::Theme` adapter exclusively through semantic/component
  tokens.
- Establish named typography, spacing, radius, border, opacity, and motion roles.
- Add theme-level tests for role completeness, foreground/background contrast, and
  semantic consistency.
- Add a development-only audit or targeted search check preventing new raw colors in
  feature modules.

Reference guidance:

- Study Comet's theme layering, solid/inverse controls, overlay roles, and separate
  light/dark construction.
- Study Waku's neutral graphite balance and scalable chrome, but retain GNSScope cyan
  and receiver semantic colors.

Gate:

- All existing shell and standard controls render through GNSScope tokens.
- Theme switching does not change layout or semantic meaning.
- No new feature renderer introduces raw product colors.

Checkpoint (2026-09-04): complete. The visual system is split into private primitive,
semantic, and component modules; both themes define the complete role set; the
third-party adapter consumes component tokens only; and `app.rs` contains no raw
product colors. Contrast, completeness, containment, full workspace, strict Clippy,
release packaging, and independent light/dark rendering checks pass. See
`.scratch/ui-visual-refactor/phase1-evidence.md`.

### Phase 2 — Primitive controls and complete interaction states

**Goal:** create a small, consistent GNSScope component layer.

Deliverables:

- Implement or wrap icon button, text button, segmented/tab control, run row, data row,
  badge/status indicator, divider, tooltip, menu item, text input, switch, progress,
  and scrollbar primitives.
- Define compact/default sizes and all states listed in Section 4.7.
- Centralize mouse plus Enter/Space activation semantics.
- Add visible focus navigation and keyboard-highlight behavior for lists, menus, tabs,
  timeline controls, and dock actions.
- Give icon-only controls localized tooltips and accessible names.
- Distinguish destructive actions from ordinary selection; deletion keeps an explicit
  confirmation path.
- Retain `gpui-component` for behavior-heavy controls until the GNSScope wrapper has
  equivalent keyboard, focus, accessibility, and platform evidence.

Reference guidance:

- Study Waku's compact icon-button/MenuChip geometry and activation helper.
- Study Waku's search-focus/highlight split for searchable menus.
- Use Comet's semantic component roles, not its chat-sized roundness.

Gate:

- A component gallery or deterministic preview exercises every state in both themes.
- Keyboard-only traversal can reach, activate, and leave every primitive.
- Disabled and busy controls cannot trigger duplicate actions.

### Phase 3 — Structural extraction and persistent shell

**Goal:** reduce `app.rs` coupling and establish the final four-region shell without
changing workflows.

Deliverables:

- Extract visual-system, primitive, shell, Runs-dock, configuration-rail, epoch-dock,
  status-strip, and plot-frame code in reviewable slices.
- Keep application actions and receiver/run state ownership in `app.rs` or focused
  state modules; render modules receive immutable view data and callbacks.
- Implement the persistent layout: Runs dock, selected-run workbench, optional Run
  Configuration rail, and full-width epoch/status dock.
- Preserve the concept proportions at `1568x1003` and the documented `1440x900`,
  `1100x800`, and `800x720` responsive states.
- Collapse the configuration rail first, then the Runs dock. Never stack the run
  library over plots.
- Persist only presentation-safe dock visibility and size; do not store it in run
  configuration.
- Ensure flex children and scroll regions have explicit minimum constraints so narrow
  layouts do not overflow or hide the epoch dock.

Reference guidance:

- Use Waku's dense native shell and panel-width transition as behavior references.
- Use Comet's surface ladder for raised/overlay regions.
- The concept image, not either upstream app, decides region placement.

Gate:

- Empty draft, processing run, completed run, and selected historical run inhabit one
  shell without legacy global page navigation.
- Active and selected run identities remain independently visible.
- Narrow layout preserves plots, active progress, and epoch access.

### Phase 4 — Runs dock and New Analysis workflow

**Goal:** finish the highest-frequency workflow with Waku-like density.

Deliverables:

- Runs dock contains compact header action, search/filter, Active, Queue, and History.
- Run rows use stable geometry for status, progress, percentage, time, and contextual
  actions without large dashboard cards.
- Selected, active, hovered, keyboard-highlighted, failed, completed, queued,
  interrupted, and unavailable-source states remain distinct.
- History is virtualized or otherwise bounded for large libraries.
- Queue reordering and cancellation remain explicit and keyboard operable.
- New Analysis occupies the central workbench with IQ source, metadata, receiver
  configuration, inline validation, and one contextual Run or Queue action.
- Starting transforms the draft context into its run workspace; it does not navigate
  to an unrelated page.
- Cancelled file selection preserves a valid empty draft.

Reference guidance:

- Study Waku's sidebar row rhythm, virtualization, selection wash, and quiet action
  hierarchy.
- Do not copy Waku's session-card identity or coral accent.
- Use Comet's raised treatment only for menus, file choices, and confirmation dialogs.

Gate:

- All run-library and draft behaviors in the preservation matrix pass without using a
  legacy surface.
- Large history remains responsive and does not rebuild receiver data on hover.

### Phase 5 — Selected-run workbench and domain plots

**Goal:** make measurements dominate the canvas while keeping navigation and state
quiet.

Deliverables:

- Finish Overview, Channels, Position, Navigation, and Candidates as run-local views.
- Build one shared plot frame with title, subtitle/unit, legend, axes, grid, cursor,
  selection, tooltip, empty, loading, error, and unavailable states.
- Overview preserves the concept hierarchy: Sky Plot, ENU trajectory, selected-epoch
  PVT table, and bounded multi-PRN C/N0 history.
- Channels links the C/N0 history, channel table, acquisition/tracking/navigation
  state lanes, and PRN selection.
- Position aligns Raw/Smoothed/Kalman-derived presentation where available without
  changing the receiver-selected result.
- Navigation presents only persisted signal, tracking, navigation-message, geometry,
  event, and ephemeris data; absent ephemeris parameters remain explicitly unavailable.
- Candidates compares aligned values and unavailable/rejected/converged dispositions
  without mutating a historical run.
- Plot focus mode preserves selected run, epoch, PRN, and live-follow state.
- Tables provide textual counterparts for critical chart status where useful.

Reference guidance:

- Use Comet's restrained elevation and text hierarchy around plots and inspectors.
- Use Waku's compact controls and typography rhythm.
- Plot geometry, units, labels, selection, and data semantics remain GNSScope-owned.

Gate:

- Every run-local view works against real persisted records and deterministic preview
  fixtures.
- Units, legends, cursor, gaps, unavailable states, and non-color status cues are
  present and correct.
- No UI-only value is presented as receiver evidence.

### Phase 6 — Epoch dock, status strip, overlays, and command access

**Goal:** complete cross-view interaction and secondary surfaces.

Deliverables:

- Timeline shows labeled receiver-time ticks, minor/major ticks, event stems, shared
  cursor, zoom, first/previous/next/last stepping, precise epoch jump, and live follow.
- Timeline zoom and manual movement preserve exact epoch identity; manual movement
  exits live follow and Return to live selects the newest available epoch.
- Bottom status strip distinguishes selected-run context from global active processing.
- Add application settings, run configuration, menus, popovers, tooltips, and dialogs
  through the raised-surface token roles.
- Add context-sensitive commands for New Analysis, IQ selection, Run/Queue, dock
  toggles, run search, PRN selection, epoch steps/jump, plot focus, and Return to live.
- Prevent shortcut activation while text entry owns the same key sequence.

Reference guidance:

- Use Comet's 140/180/200 ms menu/dialog/panel timing where motion improves continuity.
- Use Waku's focus discipline, restrained overlays, and leased animation clock.

Gate:

- Mouse and keyboard paths produce the same state transitions.
- Timeline synchronization and active-versus-selected independence pass interaction
  regressions.
- Overlays remain inside the window, above native surfaces, and readable at narrow
  sizes.

### Phase 7 — Platform integration and accessibility

**Goal:** achieve native quality without compromising portability.

Deliverables:

- Run a bounded macOS Sidebar-material spike behind the visual-system platform adapter.
- Validate foreground contrast over light/dark desktop backgrounds and deterministic
  opaque fallback behavior.
- Verify client-decorated title-bar dragging, double-click, minimize, maximize/restore,
  close, and keyboard behavior on supported platforms.
- Honor reduced motion and, where available, reduced transparency.
- Validate focus order, visible focus, accessible names, tooltip alternatives, text
  scaling, localization expansion, and non-color status cues.
- Validate pointer hit areas independently from compact visible geometry.
- Keep native integrations optional; unsupported platforms use semantic opaque
  surfaces with identical information hierarchy.

Gate:

- macOS native material is enabled only if it passes contrast, resizing, overlay,
  screenshot, and packaging checks.
- Windows and Linux do not lose controls, spacing, or contrast when native material is
  absent.
- Keyboard-only use covers all primary workflows.

### Phase 8 — Motion, idle performance, and large-data behavior

**Goal:** add polish without creating continuous work or data-path regressions.

Deliverables:

- Implement the shared motion catalogue and reduced-motion behavior.
- Add the leased shared animation clock for indeterminate activity only.
- Verify that the app schedules no repeating redraw while idle.
- Keep plot and list rendering bounded; do not rebuild all epochs or run rows for a
  hover, spinner tick, or unrelated state change.
- Preserve run-list virtualization and avoid cloning complete histories into every
  component render.
- Measure representative completed and active runs before and after the phase; document
  any material CPU, memory, or interaction-latency change.
- Keep determinate progress driven by processed samples and stable geometry.

Gate:

- Idle workbench has no animation-driven invalidation.
- Repeating animations stop when their lease expires or reduced motion is enabled.
- Dense persisted histories remain usable without changing receiver cadence or stored
  snapshots.

### Phase 9 — Visual verification, legacy retirement, and release gate

**Goal:** prove the new visual system in the real packaged app and remove the old shell
only after parity.

Deliverables:

- Capture actual application screenshots, not mockups or source-only review.
- Compare the reference-width shell to the concept image for region proportions,
  hierarchy, alignment, plot density, timeline treatment, and typography.
- Exercise the visual matrix in Section 7.
- Exercise the interaction matrix in Section 8.
- Run GUI/workspace tests, formatting, strict Clippy, packaging, signing, and receiver
  regression checks in Section 9.
- Fix clipping, overlap, weak contrast, unstable row geometry, misleading state, and
  theme/localization drift found in screenshots.
- Remove dormant legacy page/drawer render paths only after behavior and evidence gates
  pass.
- Remove an external component dependency only when its replacement has equivalent
  focus, keyboard, accessibility, and platform evidence.
- Update `docs/gui-design-contract.md`, `docs/ui-architecture.md`, screenshots, and ADRs
  to describe the shipped state rather than implementation history.

Gate:

- Priority 1 Definition of Done in Section 10 is satisfied.
- No indefinite legacy/new-shell compatibility mode remains in a release.

## 7. Visual verification matrix

### 7.1 Required window matrix

| Size | Purpose |
| --- | --- |
| `1568x1003` | concept-proportion reference |
| `1440x900` | recommended desktop |
| `1100x800` | compact desktop |
| `800x720` | narrow supported desktop |

Each size requires light and dark coverage. The shell, controls, plot labels, units,
timeline, status strip, dock collapse, and overlays must remain usable in Simplified
Chinese and English.

### 7.2 Required lifecycle states

- empty New Analysis;
- valid draft;
- validation failure;
- queued;
- processing with determinate progress;
- completed with PVT;
- completed/no-fix;
- failed;
- cancelled;
- interrupted;
- missing or moved source with persisted results still available;
- historical run selected while another run is active.

### 7.3 Required run-local views

- Overview;
- Channels;
- Position;
- Navigation with data;
- Navigation with honest unavailable sections;
- Candidates with all three candidates;
- Candidates with missing/rejected candidates;
- focused plot;
- Run Configuration rail;
- settings, menu, popover, tooltip, and destructive confirmation.

### 7.4 Screenshot acceptance questions

Every screenshot review answers:

1. Does the selected measurement dominate over navigation chrome?
2. Are active run, selected run, selected epoch, and live-follow state unambiguous?
3. Do typography and rows remain stable while values change?
4. Are all units, plot axes, legends, and unavailable states readable?
5. Does color preserve meaning and have a non-color companion?
6. Are hover, focus, selected, disabled, and busy states distinguishable?
7. Does light mode preserve depth without excessive borders or shadow?
8. Does dark mode avoid both crushed blacks and blue/neon styling?
9. Is any glass, blur, radius, animation, or accent color decorative rather than
   functional?
10. Does the layout still match the concept hierarchy after GNSScope's deliberate
    product corrections?

## 8. Interaction verification matrix

Automated interaction coverage should include:

- create and abandon a draft;
- choose/cancel IQ selection;
- validate malformed and domain-invalid settings;
- start, queue, reorder, and cancel;
- open history without disturbing the active run;
- search/filter runs and use contextual actions;
- collapse/restore Runs and Run Configuration docks;
- switch every run-local tab without losing epoch/PRN/live-follow state;
- hover, select, pin, and clear a PRN across linked views;
- drag, step, jump, zoom, and return the epoch timeline to live;
- enter and leave plot focus;
- operate menus, dialogs, lists, tabs, and primary actions by keyboard;
- prevent disabled/busy duplicate activation;
- switch System/Light/Dark and language without changing run data;
- restore persisted presentation preferences without putting them in run snapshots.

## 9. Verification commands and evidence

Minimum implementation checks:

```bash
cargo fmt --all --check
cargo test -p gnss-gui --lib
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Release evidence additionally includes:

- deterministic screenshot preview runs;
- `scripts/package-macos.sh` and `scripts/run-gui-preview-macos.sh` on macOS;
- macOS bundle signature verification;
- Windows and Linux build/package checks when those release targets are in scope;
- receiver regression evidence showing unchanged persisted values and selected outputs;
- explicit identification of real-data tests that were run, ignored, or unavailable.

A green default unit test run is not proof of visual correctness or full real-data
receiver validation. Visual acceptance requires screenshots from the freshly built,
packaged application at the required sizes.

## 10. Priority 1 Definition of Done

The visual refactor is complete only when all statements are true:

- GNSScope opens directly into the concept-led Signal Workbench.
- The final application has no legacy dashboard or global four-page navigation.
- Concept layout, Waku-inspired density, and Comet-inspired theme hierarchy are visible
  without copying either upstream product identity.
- All visual values flow through primitive, semantic, and component token roles.
- Standard controls expose complete, accessible interaction states.
- Active run, selected run, draft, epoch, PRN, and live-follow identities remain
  independent and testable.
- All existing run, queue, configuration, history, plot, candidate, and timeline
  workflows remain usable.
- Completed runs remain immutable and receiver/persistence results remain unchanged.
- Plots use real or clearly marked deterministic preview data and never fabricate
  receiver evidence.
- Idle rendering has no permanent animation clock; reduced motion is honored.
- The required size, theme, language, lifecycle, and view matrices pass actual visual
  review.
- Keyboard/focus/accessibility checks pass for primary workflows.
- GUI/workspace tests, formatting, strict Clippy, and required packaging checks pass.
- Dormant legacy render paths are removed only after parity evidence exists.
- Documentation and ADRs describe the final shipped boundary.

## 11. Immediate execution order

The next implementation cycle should proceed in this exact order:

1. Complete Phase 0 evidence and hardcoded-style inventory.
2. Finish Phase 1 token layering on top of the existing `visual_system.rs` work.
3. Build the Phase 2 component-state gallery before further broad restyling.
4. Extract the Phase 3 shell modules from `app.rs` without changing behavior.
5. Refine Runs/New Analysis, then run-local workbench views, then timeline/overlays.
6. Attempt native platform material only after the opaque shell is correct.
7. Add motion only after static states, focus, and responsive behavior pass.
8. Complete packaged-app visual validation before deleting legacy paths.

Do not start Priority 2 multi-signal production work merely because one UI phase is
visually attractive. Priority 1 is stable only after its behavior, screenshot,
interaction, and release gates pass.
