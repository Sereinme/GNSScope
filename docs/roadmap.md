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
