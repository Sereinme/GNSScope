# GNSScope Roadmap

Date: 2026-09-04

This is the authoritative product roadmap. Detailed receiver evidence remains in
`docs/receiver-readiness-audit.md`; historical M1/M2 progress notes and performance
experiments are not the current backlog.

The detailed execution plan for Priority 1 is
`docs/ui-visual-refactor-roadmap.md`. This roadmap remains authoritative for priority
and completion status; the detailed plan owns the UI phase breakdown, reference-code
policy, and evidence matrix.

## Product Direction

The active priorities are now:

1. Refactor the desktop application UI.
2. Extend the receiver beyond GPS L1 C/A to additional signals and constellations,
   including GPS L1C, GPS L2C, GPS L5, and BeiDou B1I.

Accuracy expansion, dynamic/high-rate scenario coverage, boundary campaigns, and
further performance optimization are intentionally deferred. Existing correctness,
determinism, persistence, and real-data regressions remain mandatory gates while the
UI and signal-processing architecture change.

## Current Baseline

- M0-M2 are implemented for recorded GPS L1 C/A IQ files.
- The bounded single-pass pipeline covers Int8/Int16 IQ input, PCPS acquisition,
  deterministic parallel tracking, navigation-message decoding, observables,
  continuous PVT, velocity/time output, channel admission, and reacquisition.
- The GPUI application provides editable receiver configuration, a processing queue,
  cooperative cancellation, an immutable SQLite run library, historical replay,
  domain plots, and Raw/Smoothed/Kalman candidate comparison.
- CTTC and LabSat real-data regressions cover the current GPS L1 C/A path, including
  incomplete final tracking epochs, forced channel loss/recovery, and complete
  serial-versus-parallel CTTC output equality.
- GNSScope v0.1.0 is the first public binary release for macOS, Windows, and Linux.
- Production signal-processing code still uses concrete GPS L1 C/A types. ADR-0002
  records accepted signal-abstraction intent, but its trait design is not yet the
  production implementation and must not be described as implemented multi-signal
  support.

## Priority Roadmap

### 1. Refactor the application UI

This is the immediate product priority. The work is a structural application refactor,
not another isolated visual-polish pass.

**2026-08-29 design decision:** the earlier structural pass proved the current
workflows, but its four-page navigation and card/dashboard composition are not the
visual or interaction baseline for the final application. GNSScope is being rebuilt as
a single GNSS signal-processing workbench inspired by Zed's pane-and-dock model. The
approved visual baseline is the compact graphite instrument concept, while the later
interaction corrections remain authoritative: there is no global activity bar,
Channels, Events, Configuration, and Candidates belong to the selected run, New
Analysis is a small action in the Runs header, and the bottom-left control collapses
the Runs dock. Existing behavior is the compatibility baseline; existing page
composition is not.

The target experience is a compact scientific desktop instrument rather than a web
dashboard. It uses a collapsible Runs dock, central selected-run analysis panes,
optional right epoch/PRN inspector, fixed epoch navigator, and status bar. Signal plots
and measurements dominate the canvas; controls remain quiet, dense, and contextual.
Decorative gradients, glass effects, excessive cards, large calls to action, and
generic "space" styling are explicitly out of scope.

**2026-08-29 concept-parity correction:** implementation must now be judged against
`docs/Codex 图像 2026年8月29日 22_07_48.png`, not merely against the broad pane-and-dock
idea. The reference controls proportion, typography, plot treatment, timeline density,
and alignment, with the following deliberate product corrections. The selected-run
workbench must not contain a separate progress strip: active progress belongs in the
Runs dock and the bottom-left status readout. The persistent right rail shows the
selected run's immutable configuration snapshot rather than per-satellite telemetry.
Per-satellite signal, tracking, navigation-message, geometry, and ephemeris details
belong to the run-local Navigation workbench. Overview may retain the bounded multi-PRN
C/N0 history shown in the concept, but not the detailed signal inspector. The timeline
must be a GNSScope-owned event/time instrument with labeled time ticks, event stems,
an explicit cursor, zoom controls, precise epoch stepping, and live-follow state—not a
generic slider presented as the primary visualization. The previously approved compact
Runs-header New Analysis action remains authoritative even though the reference image
contains an earlier full-width treatment.

**2026-09-04 visual synthesis decision:** the concept image and the GNSScope domain
contracts remain authoritative for information architecture. Waku is the reference for
compact native density, quiet structural controls, platform typography, focus/keyboard
discipline, sidebar treatment, and performance-conscious motion. Comet is the reference
for surface elevation, three-layer design tokens, complete component states,
light/dark construction, overlays, and a bounded motion catalogue. GNSScope retains its
cyan interaction color, receiver semantic colors, scientific plots, and product
identity. It must not adopt Waku's coral identity, Comet's chat-like roundness or broad
glass treatment, or either project's navigation model. Detailed implementation order,
upstream file references, licensing constraints, and acceptance gates are recorded in
`docs/ui-visual-refactor-roadmap.md`.

**Phase 0 checkpoint (2026-09-04):** the behavior-preservation matrix, source/module
inventory, hardcoded visual-value census, historical screenshot classification, and
scoped Phase 1-9 issues are recorded under `.scratch/ui-visual-refactor/`. Formatting,
GUI tests, complete workspace tests, strict Clippy, the release GUI build, macOS
packaging, and signature verification pass. Fresh 1440x900, 1100x800, and 800x720
packaged-app baselines were captured in both themes under
`artifacts/ui-visual-refactor-baseline/`. Phase 0 is complete. The baseline confirms the
concept-led structure while identifying Phase 1-3 work: third-party purple leakage,
overused light-theme borders, weak small-text contrast, overly strong active-row wash,
and failure to collapse the Runs dock at 800 px.

**Phase 1 checkpoint (2026-09-04):** the visual system is now split into raw primitive,
semantic, and component layers under `crates/gnss-gui/src/visual_system/`. Light and
dark themes are authored independently and define surface, text, interaction,
solid/inverse, overlay/dialog, focus/disabled, receiver-status wash, plot, and chart
roles. `gpui-component::Theme` is translated only through GNSScope component tokens,
and feature rendering contains no raw product colors. Theme contrast, completeness,
and containment tests pass; the full workspace reports 242 passed, 8 ignored, and 0
failed tests, with formatting, strict Clippy, release packaging, and light/dark visual
inspection also passing. Independent macOS captures showed that the earlier rounded
purple element was a screen-control overlay rather than an application theme leak.
Evidence is recorded in `.scratch/ui-visual-refactor/phase1-evidence.md`. Phase 1 is
complete and Phase 2 component-state work is next.

**Phase 2 checkpoint (2026-09-04):** GNSScope-owned compact wrappers now cover the
active button, switch, row, menu, input, progress, badge, divider, scroll, and tooltip
surfaces while retaining narrowly wrapped `gpui-component` behavior. Runs and Run
Library filters plus IQ-format selection use explicit checked popup menus rather than
cycling state. The packaged macOS Release application was inspected for anchored menu
placement, selected states, icon-only tooltip behavior, arrow/Return selection, and
Escape cancellation; light/dark deterministic component galleries and production
workspace captures remain available under `artifacts/ui-visual-refactor-phase2/`.
GUI tests report 71 passed and 2 ignored real-data regressions, with formatting,
strict workspace Clippy, Release build, and macOS packaging passing. Phase 2 is ready
for human review. Exhaustive every-control Tab traversal and Windows/Linux visual
evidence remain explicit boundaries; dormant legacy render functions are reserved for
the later parity cleanup rather than mixed into the accepted active-path migration.

**GPUI dependency migration to GPUI Kit (2026-10-09):** `gnss-gui` now depends on
`gpui-kit` 0.7.1 alone, replacing the direct `gpui` 0.2.2, `gpui-component` 0.5.1, and
`gpui-component-assets` 0.5.1 dependencies. The facade pins GPUI (published as the
`gpui-pre` 0.3.8 family), `gpui-base` 0.7.1, `gpui-component` 0.7.1, and
`gpui-kit-assets` 0.7.1; imports use `gpui_kit::` and `gpui_kit::component::` paths,
and `main.rs` boots through `gpui_kit::application()` and `gpui_kit::init`. Eight
upstream API breaks were adapted: `Timer::after` became the background-executor
timer, `Corner` became `Anchor`, the custom button variant lost its border builder
(border now applied through Styled), `Progress` requires element ids, `update` no
longer returns a result, `Theme::list` became a behavior flag rather than a color,
focus takes a context argument, and `SliderEvent` gained a `Release` variant. The
dev-only `runtime_shaders` feature no longer exists in gpui-pre 0.3.8; tests and the
Release build pass without it. The history context-menu adapter was rebased from the
vendored 0.5.1 copy onto the 0.7.1 API surface, and gpui-kit's Apache-2.0 attribution
joins `licenses/`. Gates: GUI suite 83 passed/2 ignored, complete workspace tests,
formatting, strict Clippy, Release build, and both ignored real-data regressions
(CTTC complete-capture persistence and LabSat full-constellation PVT, 466 s) pass.
A packaged-equivalent Release launch smoke runs stably and opens the SQLite run
library. Headless pixel capture (`examples/visual_capture.rs`, off-screen
`VisualTestAppContext`, deterministic preview fixtures) then provided the visual
evidence the blocked native screen capture could not: both themes render the
populated workbench with correct button borders (`border_1` + `border_color`
replacement), readable Runs row backgrounds, and matching theme geometry;
captures are indexed in `docs/ui-phase4-validation.md`. Native right-click
menu/confirmation interaction remains grouped with the Phase 4 mouse gates.

**Interface-wide design pass (2026-10-19):** the application shell, plots, and document
pages were rebuilt against the concept image plus the `raycast`/`linear.app` DESIGN.md
references (adoption table and change list in
`.scratch/ui-visual-refactor/design-pass-evidence.md`). Tokens moved to a cool graphite
ladder with a neutral solid primary action, hairline elevation, and an ordered six-step
sky-plot elevation ramp; `ButtonKind::{Tab, Toggle}` were added so tabs and persistent
toggles stop reading as filled chips; a gpui-component defect that washed out every
custom-variant button fill was fixed by setting the resting fill explicitly. Sky labels
now hug their markers (canvas-clamped, no leader lines), the ENU trajectory uses the
data-series blue, the C/N0 axis sits under the last lane, the overview grid uses two
explicit rows so panes never collide with the fixed timeline, the run-configuration rail
scrolls beneath a pinned header, and Run Library / Interface Settings became flat dense
lists instead of cards and chips. New headless capture harness
`examples/capture_surfaces.rs` writes ten surfaces plus the 1568/1440/1100/800 ×
theme × language matrix to `artifacts/ui-design-pass/`. Gates: `cargo fmt --all --check`,
strict workspace Clippy, and `cargo test --workspace` green with real datasets present
(gnss-core 146, gnss-gui lib 187 + 2 ignored). Resolution then moved to the
`apple` DESIGN.md for precision-only principles: separators became text-ink alpha
hairlines, the system keeps exactly one shadow (the command palette), every pane header
and table shares one 12px inset, PVT and channel measurements right-align with their
headers, the dark ladder desaturated toward neutral graphite and the light canvas adopted
Apple's parchment `#f5f5f7`. Still open: the draft form's two-column field grid, the
receiver-settings drawer's card composition, native mouse acceptance, and roadmap
Phases 7–9.

**Floating-chassis redesign (2026-10-20):** after two precision passes proved too
incremental, the shell was rebuilt on a floating panel chassis: a deeper canvas with 8px
gutters, 12px panel radius, and a two-level shadow language (soft for panels, strong for
the command palette). This explicitly supersedes the earlier flat-chassis constraints
(structure radius 0-4px, no shadows outside overlays); the arbitration table and rationale
are recorded in `.scratch/ui-visual-refactor/design-pass-evidence.md` §6. The workbench
header collapsed to a single identity line plus a 22px underline tab row, dock rows became
inset selection pills with quiet section labels, the three document pages moved into a
shared floating panel, and the draft form became a 232px wrapping field grid. Two defects
were fixed: gpui-component painted its own selection chrome over GNSScope's tokenised
tabs, and a temporarily shortened timeline (140px) broke the playhead hit test (caught by
the issue-20 regression). Gates: formatting, strict Clippy, and
`cargo test --workspace` 360 passed / 0 failed / 8 ignored with real datasets present;
36 evidence captures refreshed under `artifacts/ui-design-pass/`.

**Detail, colour, radius and interaction pass (2026-10-20):** the last pass replaced the
per-component radii with a four-step ladder (control 4 / nested surface 8 / panel 12 / pill),
switched hover and press from fixed greys to ink-alpha overlays that work on every surface
tier, adopted Carbon's field grammar (filled input, 2px accent underline on focus), gave
buttons a layout-stable inset 2px focus ring, added a 1px lit inner edge to panels, made
scrollbars a faint ink pill on a transparent track, and strengthened the plots themselves: a
flat 14% area under each C/N0 lane (closed correctly at data gaps, never a gradient) and a
full-strength tail over the last fifth of the ENU track. See
`.scratch/ui-visual-refactor/design-pass-evidence.md` §7 for the adoption table. Gates:
formatting, strict Clippy, and `cargo test --workspace` 360 passed / 0 failed / 8 ignored.

**Carbon chassis rebuild (2026-10-20):** on the product owner's instruction to discard the
existing concept work, the shell was rebuilt on IBM Carbon's language rather than tuned
again. Corners are 0px everywhere (2px tags, pill status only), the 8px guttered floating
cards became adjacent regions separated by a 1px hairline (the shell gap *is* the rule),
the title bar became a 48px inverse UI-shell header, the accent moved from cyan to IBM
Blue with a solid-blue primary action, selected rows use Carbon's 3px left bar on a
surface step, pane and section headers are tone bands closed by a hairline, and data type
rose to 12px. Structure carries no shadow; only genuinely floating layers do. See
`.scratch/ui-visual-refactor/design-pass-evidence.md` §8. Gates: formatting, strict
Clippy, and `cargo test --workspace` 360 passed / 0 failed / 8 ignored, 36 captures
refreshed.

**Timeline restructure (2026-10-20):** two rounds of recolouring could not fix the bar
because its problem was structural — 85px of band holding nine hairlines. The band now has a
ruler at the top (time labels with ticks hanging from it), four 13px event lanes
(ACQ/TRK/NAV/LOSS) in a 34px labelled rail, hairline lane rules, a 2px record-extent bar on
the bottom edge, and a playhead whose callout sits at the top of the lanes. The signal
envelope survives as a 44x14 readout in the header rather than as a competing chart behind
the lanes, and the whole bar dropped from 148px to 124px so the plots gain the difference.
See `.scratch/ui-visual-refactor/design-pass-evidence.md` §10. Gates: formatting, strict
Clippy, 360 passed / 0 failed / 8 ignored.

**Timeline mirrored around its axis (2026-10-20):** the band now reads as an instrument
line — acquisition and tracking lanes above, a 20px two-rule time rail in the centre with the
time labels set between its rules, navigation and loss lanes below, and the 2px record-extent
bar on the bottom edge. Event caps face away from the axis, the playhead's square cap sits on
the rail, and the timecode became a header readout so nothing occludes the track. The
in-track callout, `callout_flipped`, the `CALLOUT_*` constants, and `TrackView.epoch_number`
were deleted with it, and the bar dropped from 148px to 116px. See
`.scratch/ui-visual-refactor/design-pass-evidence.md` §11. Gates: formatting, strict Clippy,
359 passed / 0 failed / 8 ignored (one test removed with the callout).

**Timeline rebuilt as a signal profile (2026-10-20):** four styling rounds failed for the
same reason — at a 20:1 aspect ratio every vertical layering (lanes, dual axis, row rules)
collapses into thin stripes. The band is now a single filled profile of per-bucket peak C/N0
(neutral grey area with a 1px crest, auto-mapped to the run's own min..max for legibility),
punctuated by full-strength event flags, standing on a hairline baseline with a 4px tick ruler
and 11px time labels underneath. The playhead is a wash band, a 1.5px line, and a triangular
handle on the baseline; the timecode lives in the header. The bar-equivalent "coverage ridge",
the mirrored lane stack, the envelope backdrop, and the whole callout/boundary-caption
machinery were deleted rather than left dormant. See
`.scratch/ui-visual-refactor/design-pass-evidence.md` §12. Gates: formatting, strict Clippy,
357 passed / 0 failed / 8 ignored.

**Timeline measurement and ruler scrubbing (2026-10-21):** the profile now plots the MEAN
C/N0 of the tracked channels per epoch instead of the peak — a peak stays high while every
other satellite drops out, which hides exactly the moment worth finding — and the header
subtitle states the measure (`C/N0 均值` / `C/N0 mean`). The time labels moved inside the band
under the baseline so the seek layer covers body and ruler alike: dragging the scale now
seeks, with `timeline_ruler_row_seeks_like_the_body` as its regression. See
`.scratch/ui-visual-refactor/design-pass-evidence.md` §13. Gates: formatting, strict Clippy,
358 passed / 0 failed / 8 ignored.

**Timeline plots satellite count (2026-10-21):** C/N0 is a per-channel quantity, so any single
aggregate of it has no referent; the profile now plots tracked satellites per epoch (bucket
MINIMUM so a short outage cannot hide), on a fixed 0..=12 domain, drawn as a step line with a
20% fill and a dashed 4-satellite floor labelled `4` — the 3D-fix minimum that gives the level
its scale. The subtitle names the measure (`卫星数`). The fixture's constant 9 satellites make
the line flat; that is the data, not the render. See
`.scratch/ui-visual-refactor/design-pass-evidence.md` §14. Gates: formatting, strict Clippy,
358 passed / 0 failed / 8 ignored.

**Timeline body is PDOP (2026-10-21):** three candidate scalars were tried and rejected —
aggregate C/N0 (per-channel, no referent), tracked satellite count (constant on a healthy
record), and speed (a zero-based area read as a solid block, while a deviation band
exaggerated ±0.25 m/s into a full-amplitude sine). PDOP is the one series that is a single
per-epoch value with an absolute scale and an interpretable direction, and it is already
persisted (`quality.pdop`, `None` honoured — never a fabricated zero). It is drawn inside its
own quality zones (0–2 / 2–5 / 5–10 / >10, only the risk zones tinted) with the curve coloured
by the zone it occupies, so a constant record still reads. See
`.scratch/ui-visual-refactor/design-pass-evidence.md` §15. Gates: formatting, strict Clippy,
357 passed / 0 failed / 8 ignored.

**Phase 4 deletion safety correction:** history reveal/delete actions now live in
right-click menus, not selected-row inline controls. Delete requests a native
Cancel-first confirmation bound to the original UUID; changing selection/order
cannot change its target. The GPUI prompt regression passes; GUI suite 81 passed/2
ignored, strict Clippy and Release packaging pass. Native inspection confirms
inline buttons removed, but automated right-click delivery still fails, so native
menu/confirmation and concurrent cancellation acceptance remain open.

**Phase 4 real-worker regression (2026-09-08):** the unattended
`cancelling_queued_copy_does_not_stop_real_worker` test drives production enqueue,
worker events, cancellation and deletion with an isolated in-memory library and a
temporary zero-IQ source. A real worker is unfinished when B is cancelled, then
advances and completes all 4,000 samples; B remains cancelled with zero processed
samples, and original history/source survive. GUI tests: 80 passed/2 ignored.
This closes the automated real-worker scheduling check, not native mouse acceptance
or RF/PVT validation. Native control remains unresolved; Phase 4 is still open.

**Phase 4 app-command regression:** a GPUI TestAppContext regression now invokes
production enqueue/cancel/delete methods against an injected in-memory library,
proving queue selection clears stale replay, active buffers survive, cancellation is
stored, and deleting the copy preserves original completed epochs. GUI tests report
79 passed/2 ignored; strict Clippy, Release build/package/signature pass. Dev-only
runtime shader support avoids the host's missing offline Metal compiler. Native
concurrent-worker/click acceptance is still distinct from this passing state test;
see `docs/ui-phase4-validation.md`.

**Phase 4 narrow/Finder follow-up:** the isolated 800x720 light/English instance now
passes 80/103 cap text, oldest-record search (1/1), keyboard selection/workbench return,
persisted epoch reload and English empty results. Finder explicitly selects the
generated fixture-000.dat, closing the earlier reveal blocker. Five fresh captures
are indexed in `docs/ui-phase4-validation.md`. Native concurrent queue/cancel and
deletion interaction remain open; this is not full Phase 4 completion.

**Phase 4 draft-start follow-up (2026-09-06):** native file selection produced a valid
draft; Start processing switched to a new workbench and completed the zero-IQ input.
SQL confirms a new ID, all 1,000,000 samples processed, zero PVT epochs as expected,
and 103 total test records. Desktop history-row focus comparison shows stable positions.
Four captures are indexed in `docs/ui-phase4-validation.md`. Concurrent queue/cancel,
deletion interaction, Finder success, and the refreshed theme/narrow checks remain open.

**Phase 4 persisted-fixture checkpoint (2026-09-06):** a marked, explicit-path test
library and non-overwriting fixture generator now isolate workflow verification from
the ordinary database. Native inspection verifies 80/101 history and real zero-IQ
reprocessing to a new completed run; read-only SQL confirms the original epoch and
configuration remain intact. Focus border geometry is now reserved rather than added
on focus. GUI tests: 78 passed/2 ignored; strict Clippy, Release app/examples build,
packaging/signature and syntax checks pass. Native queue/cancel/delete, successful
Finder reveal and the refreshed layout/focus matrix remain open; Phase 4 is not complete.
Reproduction and evidence: `docs/ui-phase4-validation.md`.

**Phase 4 visual follow-up (2026-09-06):** after unlock, the packaged 1440x900 preview
passed no-match/one-match search, keyboard Completed filtering (5/5), queue down/up
reordering, Tab focus, Return history selection, Space active selection, and keyboard
missing-source feedback. Active progress remains independent. Seven captures and
remaining persisted-workflow, narrow/theme, and focus-geometry gates are recorded in
`docs/ui-phase4-validation.md`. Phase 4 remains in progress; the next slice should
provide an isolated persisted workflow fixture rather than use the user's run library.

**Phase 4 checkpoint (2026-09-06):** Runs history now shares a bounded selection
helper for displayed rows and matching counts, excluding Active and Queue. It renders
at most 80 rows, reports shown/matching totals, explains truncation, and supplies an
explicit no-match state. Search can reach records beyond the initial cap. Two tests
cover count/filter/order/cap behavior. macOS source-file reveal now uses `/usr/bin/open
-R` rather than Linux `xdg-open`. GUI tests: 76 passed, 2 ignored; strict workspace
Clippy, Release build, packaging and signature verification pass. Native inspection is
blocked by a locked Mac; neither new visual acceptance nor actual Finder reveal is
claimed. Phase 4 remains open for workflow and keyboard validation.

**Phase 3 latest (2026-09-05):** implementation and macOS shell acceptance are complete,
ready for human review. The four-size matrix (1568x1003, 1440x900, 1100x800, 800x720)
was inspected in dark Chinese and light English navigation after manual unlock.
Runs replacement/return, independent active/selected identity, epoch controls,
configuration close/restore, Settings return, and cancelled-file-picker draft paths
passed. See `docs/ui-phase3-validation.md` for build identity, 15 captures, scaling
notes, and explicit later plot-localization/numeric-readability follow-ups.

**Phase 3 implementation checkpoint (2026-09-05, before unlock):** structural implementation is complete: shell,
title bar, Runs frame/rows, configuration rail, epoch drawing/buttons, status strip,
and plot frame now have presentation-only modules. Run/worker/persistence state stays
in the application. Configuration visibility is an interface preference; narrow
Runs closes when selecting a run, creating a draft, or opening Global Settings.
Preview is isolated from the user's database. GUI tests report 74 passed/2 ignored;
complete workspace tests, strict Clippy, formatting, Release build, macOS packaging,
strict signature verification, shell syntax and diff checks pass. The final
four-size light/dark interaction matrix is **not accepted**: native inspection found
the Mac locked and unable to auto-unlock. Manual unlock is required to close Phase 3.
See `.scratch/ui-visual-refactor/phase3-evidence.md` for the exact outstanding checks.

**Phase 3 checkpoint (2026-09-05):** the first structural slice introduces a
presentation-only `ui/shell.rs` boundary for responsive policy and the root,
Runs/content, and workspace/side/timeline frames. `GnsscopeApp` still owns every
receiver, run, queue, persistence, and action callback. Policy tests cover the 1568,
1440, 1100, and 800 px targets; GUI and complete-workspace tests, strict Clippy,
Release build, and macOS packaging pass. The gate remains open for region-level
extraction and fresh visual evidence because the macOS host was locked during this
slice's packaged-app inspection. The narrow policy is implemented: below 900 px the
workbench remains visible by default and the status-strip control opens Runs as a
temporary full-width replacement without overwriting the desktop dock preference.

**Phase 3 follow-up (2026-09-05):** the global status strip now renders from
`ui/status_strip.rs`, consuming display values and an app-owned toggle control.
Selecting a run or creating a draft closes the transient narrow Runs view so the
destination becomes visible immediately. The unsupported Cmd+B tooltip was removed.
GUI tests (73 passed, 2 ignored), strict Clippy, Release build, packaging, and signature
verification pass. Fresh narrow-window visual acceptance remains pending: automation
attached to a duplicate ordinary window instead of the requested preview instance.

**Run Configuration extraction (2026-09-05):** `ui/run_configuration.rs` now renders
the read-only configuration rail from display entries and an app-owned close action.
Source selection and formatting still resolve from the selected run in `app.rs`.
The existing six sections and 24 values retain their order; rail widths now use
shared geometry tokens. GUI tests (73 passed, 2 ignored), strict Clippy, Release
build, packaging, and signature verification pass. A fresh narrow Release preview
confirms the default workbench, C/N0 plot, epoch dock, and active status remain visible
with Runs/configuration collapsed. The screenshot is
`artifacts/ui-visual-refactor-phase3/narrow-workbench.png`. Automated clicks report
`noWindowsAvailable`, so narrow Runs open/return interaction and the remaining
size/theme matrix are still pending; Phase 3 is not yet complete.

**Epoch drawing extraction (2026-09-05):** `ui/epoch_dock.rs` now draws the epoch
dock frame, time labels, event markers, and cursor from display-only `EpochView` data.
The app supplies the slider and action controls and retains epoch/zoom/live-follow
state. GUI tests (73 passed, 2 ignored), strict Clippy, Release build, packaging,
signature, formatting, and diff checks pass. The latest Release Overview was visually
inspected with the timeline still spanning below the workbench and configuration
rail; evidence is `artifacts/ui-visual-refactor-phase3/overview-epoch-extraction.png`.
Runs rendering and the full size/theme/interaction matrix remain open.

**Runs frame extraction (2026-09-05):** `ui/runs_dock.rs` now composes the header,
search slot, Active/Queue/History sections, shared scroll area, and footer from
app-supplied rows and controls. Filtering, the 80-row history cap, row actions, and
queue ordering remain in the app. The obsolete duplicate section-header helper was
removed. GUI tests (73 passed, 2 ignored), strict Clippy, Release build, packaging,
and signature verification pass. Latest Release visual evidence:
`artifacts/ui-visual-refactor-phase3/overview-runs-extraction.png`. Row-level
composition, plot frames, and the full responsive/interaction matrix remain open.

#### 1.1 Freeze the behavior contract before replacing the UI

- Inventory every current workflow and encode it in a preservation matrix before
  deleting an existing surface. The matrix must cover IQ selection, metadata and
  receiver validation, start/queue/cancel, queue ordering, immutable history, search,
  filtering, reveal, deletion, interrupted-run handling, reprocessing, theme,
  language, and candidate comparison.
- Preserve the independent identities of the editable next-run draft, the active
  processing run, and the selected run being inspected. Browsing history must never
  interrupt or replace the active run.
- Preserve the shared epoch-selection contract: moving the epoch cursor updates PVT,
  sky, channel, position, and navigation views together; manual movement exits live
  follow; returning to live restores the newest epoch.
- Preserve linked PRN selection and pinning across channel lists, C/N0 views, sky plots,
  diagnostics, and the contextual inspector.
- Capture reference screenshots and interaction tests for the current empty,
  configuring, queued, processing, completed, failed, cancelled, and interrupted
  states. These are behavior evidence, not a visual template.
- Rewrite `docs/ui-architecture.md` around the approved Signal Workbench before broad
  implementation. The current four-destination page model is superseded by this plan.

**Gate:** every existing user-visible capability has an explicit destination in the
new workbench and a testable preservation rule.

#### 1.2 Resolve the GPUI component boundary

- Do not use `gpui-component` as the application-wide visual language. GNSScope-owned
  GPUI primitives must define the shell, Runs dock, pane headers, tabs,
  toolbars, buttons, status indicators, data rows, epoch navigator, and domain plots.
- Run a bounded feasibility spike for behavior that is expensive or risky to rebuild:
  text input, focus traversal, keyboard navigation, menus, popovers, dialogs,
  virtualized lists, resizable panels, window chrome, and accessibility semantics.
- Prefer unstyled `gpui-base` behavior where it is compatible with the pinned GPUI
  version. If it is not a safe drop-in dependency, retain only narrowly wrapped
  `gpui-component` behavior until an equivalent GNSScope primitive is verified.
- Prevent visual-library leakage by putting every retained third-party control behind a
  GNSScope-owned UI interface. Feature modules must not directly assemble styled
  `gpui-component` controls.
- Supersede ADR-0004 before the broad migration. The replacement decision record must
  state which behavior primitives remain external, which visual primitives GNSScope
  owns, the pinned version strategy, and the cross-platform verification requirement.

**Gate:** input, focus, keyboard, popup, list-virtualization, resizing, title-bar, and
theme behavior work in a small cross-platform prototype, and the component boundary is
recorded before legacy surfaces are removed.

#### 1.3 Build the GNSScope visual system and application shell

- Establish semantic light/dark tokens for graphite canvas and pane surfaces, subtle
  dividers, readable text, cyan interaction/cursor state, green valid/locked state,
  amber acquisition/attention state, and red lost/error state. Status must also use
  text, shape, or icon rather than color alone.
- Separate UI typography from measurement typography. Use a platform-appropriate UI
  face and a tabular monospace face for PRNs, epochs, time, coordinates, Doppler,
  code phase, C/N0, clock, progress, and status values.
- Use a dense 4/8 px spacing system, 28-32 px data rows, approximately 30 px pane
  headers, a 36 px toolbar, a 22-24 px status bar, 0-4 px radii, one icon family, and
  minimal shadow or animation.
- Implement the persistent four-region shell: collapsible Runs dock, central tabbed
  selected-run panes, collapsible right inspector, and bottom epoch/status dock.
- Keep dock visibility and size as presentation state. Narrow layouts collapse the
  inspector first and then the contextual dock; they must not stack the entire run
  library above plots or obscure the analysis canvas.
- Add a context-sensitive command palette and stable keyboard actions for creating a
  draft, choosing IQ, starting or queueing, toggling docks, selecting a PRN, stepping
  epochs, jumping to an epoch, and returning to live.
- Keep all receiver, processing, SQLite, and immutable-run behavior outside the visual
  shell. Presentation state must not reinterpret persisted values.

**Gate:** an empty workbench, a populated historical run, and an active processing run
can all inhabit the new shell without relying on legacy page navigation or dashboard
cards.

**Implementation checkpoint (2026-08-29):** the first integrated shell now provides
the collapsible Runs dock, selected-run header and local analysis tabs, four-pane
Overview, run-local Candidates view, contextual epoch/PRN inspector, receiver-event
timeline, shared epoch stepping/live-follow, and bottom status bar. The next UI work is
visual screenshot verification, keyboard command coverage, New Analysis density
refinement, and removal of the dormant legacy render paths after parity tests.

**Concept-alignment checkpoint (2026-08-29):** the selected-run timeline now spans the
entire workbench width beneath both the analysis panes and right inspector. Overview
and Channels use persisted per-epoch channel snapshots to render linked multi-PRN C/N0
history lines instead of a selected-epoch bar chart. The Overview PVT surface is now a
dense parameter/value/unit/status table, and deterministic preview-only history covers
the line-chart and shared-cursor visual regressions without changing production data.

**Instrument-refinement checkpoint (2026-08-29):** the desktop shell now uses a
functional title-bar run search, a denser selected-run identity/metadata header, and
quieter dock proportions. Sky geometry includes a compact elevation/state legend;
ENU is rendered as a continuous trajectory over a measurement grid; the C/N0 surface
adds time-axis anchors and the shared selected-epoch cursor. The right inspector now
groups signal, tracking, geometry/quality, position, navigation/event, and ephemeris
availability in one run-local diagnostic chain. The epoch dock adds first/previous/
next/last controls and an explicit epoch readout while preserving slider and live-follow
behavior. Remaining UI work is keyboard command coverage, light-theme and full
responsive screenshot coverage, and removal of dormant legacy render paths after
parity tests.

**Concept-parity implementation target (2026-08-29):** remove the center progress
surface; preserve active progress in the Runs dock and status bar; replace the epoch/
PRN right inspector with a scrollable, read-only Run Configuration rail backed by the
selected run snapshot; rebuild Navigation as the home for signal, tracking, decoded
navigation, geometry, events, and ephemeris availability; and redraw the timeline,
sky plot, ENU trajectory, C/N0 history, and PVT table using the proportions, tick marks,
legends, measurement typography, and restrained graphite contrast of the approved
reference. Do not fabricate ephemeris parameters, hardware utilization, or receiver
events when production snapshots do not expose them.

**Concept-parity implementation checkpoint (2026-08-29):** the center progress strip
has been removed; active progress remains in the Runs dock and bottom status strip.
The wide-layout right rail now renders the selected run's immutable configuration
snapshot, while Navigation contains the linked signal, tracking, geometry/quality,
position, receiver-event, and ephemeris-availability diagnostics. The timeline now has
event stems, dense minor/major ticks, receiver-time labels, a shared cursor, functional
1x/2x/4x zoom, exact epoch stepping, and live follow. Overview proportions, graphite
contrast, Sky Plot elevation rings and marker scale, ENU grid/trajectory treatment,
C/N0 time labels, PVT row density, and deterministic active-run preview data have been
retuned against the 1568x1003 reference. Full light-theme, language, state, and
cross-platform screenshot/package coverage remains open; ephemeris parameters also
remain unavailable until the immutable run snapshot persists them.

#### 1.4 Rebuild workflows inside the workbench

Implement and verify the workflows in this order so that each step can replace a
complete legacy surface rather than layering another UI over it.

1. **Runs dock:** show Active, Queue, and History in one dense contextual panel with
   inline search, compact filtering, preview versus pinned opening, context actions,
   queue progress, and immutable replay.
2. **Analysis draft:** open a draft as a central workspace tab; keep IQ source and
   metadata in the Receiver dock, receiver settings in the Inspector, validation next
   to fields, and one contextual Run or Queue action. Starting a run transforms the
   same tab into its result workspace instead of navigating away.
3. **Live and historical workspace:** expose Overview, Channels, Position, Navigation,
   and Candidate Comparison as run-local analysis views. The active run continues in
   the background while any historical run is selected.
4. **Overview:** prioritize the sky plot, ENU trajectory, and compact epoch/PVT
   measurement strip. Replace repeated metric cards with aligned instrument readouts.
5. **Channels:** link C/N0 plots, a virtualized channel table, per-PRN acquisition /
   tracking / navigation state lanes, and the right-side PRN inspector.
6. **Position and candidates:** compare Raw, Smoothed, and Kalman outputs on shared
   axes and aligned measurements rather than three disconnected cards.
7. **Navigation:** present decoded frames, ephemeris/time state, parity or CRC results,
   and receiver events as a dense list-plus-inspector workflow.
8. **Interface settings:** expose theme, language, typography, density, and display
   preferences without mixing them with receiver configuration.

The workbench must reserve stable constellation, signal, and space-vehicle labels such
as `G03 · L1CA · NAV`, so Priority 2 can add GPS L1C/L2C/L5 and BeiDou B1I without
another navigation redesign. Color remains primarily a state indicator; constellation
and signal identity must also use text and shape.

**Gate:** every legacy workflow is usable in the new shell, and no user must return to
the legacy page hierarchy to complete a run or inspect a result.

#### 1.5 Rebuild the shared epoch and status dock

- Make the epoch navigator a persistent global instrument instead of a view-local
  slider. It must show epoch index and receiver time, support drag, precise entry,
  single-step and larger keyboard steps, fit/zoom, and an explicit live-follow state.
- Synchronize the cursor with every run-local view and keep the selected epoch stable
  when switching Overview, Channels, Position, Navigation, and Candidate Comparison.
- Add acquisition, tracking, navigation, loss/reacquisition, and PVT availability
  markers only where the underlying event data exists; do not fabricate receiver
  states for visual completeness.
- Use the status bar for active-run progress, signal profile, IQ format, sample rate,
  IF, tracked/NAV/PVT contributor counts, solution type, elapsed time, and warnings.
- Keep processing cancellation and queue state visible without allowing the selected
  historical run to masquerade as the active run.

**Gate:** the existing epoch-slider semantics and live-follow behavior pass regression
tests, with keyboard and precise-jump coverage added.

#### 1.6 Visual, interaction, and release verification

- Verify `1440x900`, `1100x800`, and `800x720` in light and dark themes and in Chinese
  and English. The smallest layout may collapse docks, but it must preserve plots,
  active-run visibility, and epoch access.
- Cover empty draft, valid draft, validation failure, queued, processing, completed,
  failed, cancelled, interrupted, no-fix, and historical-replay states.
- Add stable screenshot comparisons for the shell and each analysis view, plus
  interaction regressions for dock toggles, preview/pinned runs, queue actions, linked
  PRN selection, epoch dragging, keyboard stepping, and live-follow restoration.
- Verify that charts retain units, legends, empty/loading states, readable contrast,
  non-color status cues, and a table or textual diagnostic counterpart where useful.
- Run normal GUI and workspace tests, formatting, strict Clippy, and macOS, Windows,
  and Linux packaging checks. Real-data receiver outputs and persisted snapshots must
  remain unchanged by the UI migration.
- Remove legacy page composition and unused styled controls only after feature parity,
  interaction coverage, and the screenshot matrix pass. Do not carry both shells into
  a release as an indefinite compatibility mode.

**Priority 1 acceptance:** GNSScope opens into the Signal Workbench with no legacy
dashboard or four-page navigation; all existing workflows remain functional; active
processing continues independently of the selected historical view; completed runs
remain immutable; epoch and PRN synchronization are preserved; persisted values and
receiver-selected PVT outputs do not change; the complete theme/language/state matrix
passes; and GUI, workspace, formatting, strict Clippy, and cross-platform packaging
checks succeed.

### 2. Add multi-system and multi-frequency signal processing

This is the next receiver priority after the UI architecture is stable. The target set
includes GPS L1C, GPS L2C, GPS L5, and BeiDou B1I while retaining GPS L1 C/A without
regression.

#### 2.1 Define and prove the signal-processing seam

- Revisit ADR-0002 before implementation. Define the interface from two real adapters
  rather than wrapping the current L1 C/A implementation in hypothetical traits.
- Model constellation, signal, space vehicle, RF frequency, code/data/pilot components,
  modulation, primary and secondary codes, symbol cadence, navigation-message family,
  and time system explicitly.
- Keep signal-specific acquisition, tracking, synchronization, and navigation decoding
  behind a small interface consumed by channel scheduling and the streaming runner.
- Preserve absolute sample indices, per-channel NCO continuity, bounded memory, serial
  PVT epoch advancement, and deterministic merge order. Channel identity must expand
  from a GPS PRN alone to a stable constellation/signal/space-vehicle identity.
- Use GPS L1 C/A as the compatibility adapter and one new signal as the second adapter
  that proves the seam. Select that first signal based on available IQ data, reference
  implementations, and independently checkable navigation content.

#### 2.2 Generalize shared receiver data and persistence

- Carry signal identity through acquisition results, tracking outputs, channel health,
  navigation records, observables, PVT diagnostics, GUI events, SQLite persistence, and
  historical replay.
- Make tracking cadence and coherent/non-coherent integration signal-defined rather than
  assuming a GPS L1 C/A 1 ms code epoch everywhere.
- Extend navigation storage for the required GPS CNAV/CNAV-2 and BeiDou D1/D2 records,
  health information, clock models, and time-system conversion.
- Extend observables and PVT inputs for multiple frequencies and constellations,
  including explicit inter-system clock terms and signal biases where required.
- Migrate old databases and processing-run snapshots without changing existing GPS L1
  C/A history.

#### 2.3 Implement signal adapters

Target adapters:

- Treat the current IS-GPS-200, IS-GPS-705, IS-GPS-800, and
  BDS-SIS-ICD-B1I specifications as normative; reference receiver code is comparative
  evidence rather than the signal definition.
- GPS L1 C/A compatibility adapter over the proven production path.
- GPS L1C, including its data/pilot structure, ranging codes, overlay codes,
  acquisition/tracking behavior, and CNAV-2 decoding.
- GPS L2C, including CM/CL handling and CNAV decoding.
- GPS L5, including I5/Q5 components, secondary-code synchronization, and CNAV decoding.
- BeiDou B1I, including constellation-specific PRNs, D1/D2 navigation messages, BDT time,
  ephemeris/clock records, and observables.

Implementation order after the compatibility adapter is data-driven: do not claim a
signal is supported merely because its PRN generator or acquisition detector exists.

#### 2.4 Expose signal selection and diagnostics in the app

- Add constellation/signal selection to the next-run draft and persist it in the run
  snapshot.
- Label channels, plots, events, navigation records, and PVT contributors with stable
  constellation and signal identities.
- Present signal-specific settings only when behaviorally active; do not expose unused
  generic parameters.
- Allow mixed-signal and mixed-constellation diagnostics without making completed runs
  editable.

#### 2.5 Verify each signal by receiver stage

Track the highest independently verified stage for every signal:

1. Code and waveform structure.
2. Acquisition.
3. Tracking and symbol/secondary-code synchronization.
4. Navigation-message decoding with parity/CRC and time checks.
5. Observables.
6. Contribution to a PVT solution.
7. Serial/parallel and chunk-boundary regression on recorded or independently generated
   IQ.

**Acceptance:** GPS L1 C/A retains its current real-data results; every new signal has
documented datasets and stage-specific evidence; complete support means acquisition,
tracking, valid navigation decoding, observables, and demonstrated PVT contribution;
mixed processing preserves deterministic ordering and bounded streaming behavior; and
the GUI and persisted run history identify every contributing constellation and signal
without ambiguity.

### 3. Deferred accuracy and scenario coverage

Keep existing quality gates, but pause expansion of this campaign until Priorities 1
and 2 are complete enough to stabilize their interfaces.

Deferred work includes:

- Dynamic/high-rate PVT, rapid Doppler changes, shortened output epochs, and velocity
  recovery.
- A real satellite that is absent at startup, becomes detectable later, decodes fresh
  navigation data, and joins PVT without restarting the run.
- Independent position/velocity/time truth comparisons across additional front ends and
  datasets.
- Long captures across ephemeris changes, GPS/BeiDou time boundaries, week rollovers,
  and leap-second conversion boundaries.
- Broader real-data coverage of repeated loss and reacquisition.

Resume this priority with explicit per-dataset convergence, accuracy, continuity, and
runtime acceptance criteria. Failures must still identify the affected acquisition,
tracking, navigation, observables, or PVT stage.

### 4. Deferred performance optimization

Do not start another optimization campaign while the UI and signal-processing seams are
changing.

- Keep the existing CTTC, LabSat, acquisition, and tracking benchmarks as regression
  budgets.
- Measure runtime and peak memory when UI or signal changes materially affect them.
- Preserve the current portable scalar/FMA implementation unless a measured alternative
  improves production-shaped workloads without changing receiver results.
- Re-profile after at least two signal adapters share the new signal-processing seam;
  optimize only the measured multi-signal bottlenecks.

## Longer-Term Capabilities

These remain outside the active roadmap:

- Live SDR input, real-time deadlines, and dropped-sample recovery.
- Carrier-phase positioning, ambiguity resolution, and RTK/PPP workflows.
- Additional GPS, BeiDou, Galileo, GLONASS, or regional signals beyond the Priority 2
  target set.
- Optional Accelerate, GPU, or Metal acquisition/tracking backends.
- Automatic absolute-week anchoring for raw IQ-only captures without RINEX or a
  configured reference week.

## Current Next Actions

1. Build the Phase 2 deterministic component-state gallery and keyboard coverage before another
   broad visual pass. Prove default, hover, pressed, focus-visible, selected, disabled,
   busy, and error states in both themes.
2. Extract the persistent shell, Runs dock, Run Configuration rail, epoch dock, status
   strip, and plot frame from `app.rs` in behavior-preserving slices; keep receiver and
   persistence ownership unchanged.
3. Refine Runs/New Analysis first, then the selected-run workbench and domain plots,
   then the timeline, overlays, command access, and optional native platform material.
4. Add shared, reduced-motion-aware animation only after static layout, focus, state,
   and responsive gates pass; the idle application must not continuously redraw.
5. Complete the required size/theme/language/lifecycle/view screenshot and interaction
   matrices in the freshly built packaged app before removing dormant legacy render
   paths.
6. After Priority 1 gates pass, inventory recorded/generated IQ data and independent
   references for GPS L1C, L2C, L5, and BeiDou B1I, then revise ADR-0002 before
   multi-signal production code.
