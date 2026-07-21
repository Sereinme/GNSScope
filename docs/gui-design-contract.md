# GNSScope GUI Design Contract

Receiver integration gates and performance work are tracked in `docs/receiver-readiness-audit.md`.

## Visual Language

GNSScope uses a precise signal-laboratory visual language for technical desktop work: moderate layout variance, restrained motion, and high but organized information density. It explicitly avoids neon dark-tech styling, glow-heavy charts, glassmorphism, oversized radii, marketing composition, and decorative card stacks.

- Light theme: cool white/neutral workspace, dark ink text, restrained surface separation.
- Dark theme: neutral graphite/black surfaces rather than a dark-blue monochrome palette.
- Primary interaction color: signal cyan-blue for selection, live state, progress, and primary commands.
- Semantic colors: green for healthy/valid, amber for acquiring/probation/suspect, red for lost/failed/excluded, and neutral gray for unavailable/absent.
- Components use stable dimensions, 4-8 px radii, fine separators, and unframed full-width work areas where possible.
- Numeric output uses tabular figures or a restrained monospace face so coordinates, time, speed, and percentages do not shift.
- Interface typography uses `Segoe UI Variable Text` on Windows with compact technical labels and stronger title/value contrast; the application does not depend on a separately installed web font.
- Motion is limited to progress, drawers, status transitions, chart cursors, and epoch interpolation, with reduced-motion behavior.

Light and dark themes preserve identical information hierarchy and semantic meaning; theme is never used to hide controls or change status interpretation.

Theme preference has three persistent application modes: `System`, `Light`, and `Dark`, with `System` as the default. Manual selection applies immediately and is stored as an application preference, not a processing-run configuration. All components and plots consume semantic theme tokens; system changes affect the app only while `System` is selected. Theme transition is restrained and disabled under reduced-motion preferences.

## Interface Language

The first GUI uses Simplified Chinese while retaining standard GNSS abbreviations and units such as PVT, PRN, C/N0, DLL, PLL, RAIM, WLS, ECEF, ENU, UTC, Hz, dB-Hz, m/s, ppm, and chips. Labels pair Chinese meaning with the abbreviation where useful, and tooltips provide concise Chinese explanations and parameter impact.

Visible strings are addressed through stable message keys so English can be added later, but the first release does not require a language switch. Persisted run data stores typed values and enums rather than translated status text.

## UI Technology Boundary

As recorded by ADR-0004, `gpui-component` owns standard desktop controls, themes, forms, menus, progress, drawers, virtualized lists/tables, title bar, and focus/keyboard behavior. `gpui-component-assets` supplies the embedded icon bundle. Familiar commands use icon buttons with concise hover tooltips; text remains on primary commands whose meaning or consequence is not obvious from an icon alone. Native GPUI drawing is reserved for GNSS domain plots: sky plot, ENU scatter/track, C/N0 bars and history, velocity/clock charts, and the shared epoch cursor. Standard buttons, inputs, menus, and dialogs must not be reimplemented as custom canvas elements.

This document defines the first GUI boundary before wiring gpui widgets. The GUI should consume structured state from `gnss-gui` instead of reading test-only logs.

## Current Readiness

The backend and first functional GPUI vertical slice are integrated. `StreamingPvtRunner` runs on a single background worker with bounded IQ reads, deterministic parallel acquisition/tracking, cooperative chunk-boundary cancellation, and PVT-epoch channel telemetry. The GPUI workspace persists immutable run/configuration snapshots, complete selected PVT view models, per-satellite WLS geometry, epoch-aligned channel snapshots, and discrete acquisition/tracking/navigation events in SQLite. Completed runs restore PVT, UTC, ENU, velocity, quality, sky geometry, C/N0, and receiver-event history after application restart; the shared timeline switches every view to the same epoch, manual movement exits live follow, and Return to live restores the active cursor. System/Light/Dark preference is stored independently from receiver configuration.

The release GUI was exercised against the complete 100 s CTTC Spain capture using the explicit CTTC preset after satellite-geometry persistence was added. It acquired PRNs 1, 11, 17, 20, and 32, persisted 810 PVT epochs, produced real sky geometry from the first solved epoch onward, restored the final five-satellite plot after restart, and ended at latitude `41.2748131 deg`, longitude `1.9875802 deg`, altitude `81.116 m`, speed `0.30013 m/s`, clock drift `-5.41427 ppm`, and `2013-04-04 06:24:47.029 UTC`. The preset uses the same relaxed channel-admission parameters and Raw WLS filter configuration as the existing real-data streaming regression; filename matching does not silently apply it.

Streaming PVT epoch identity is monotonic across successive single-epoch solves. SQLite nevertheless uses a GUI-owned insertion ordinal as the epoch primary key rather than trusting a receiver-provided label to be unique. This prevents repeated core indexes from replacing earlier history rows. On startup, stored run summaries are reconciled against the actual epoch row count so older affected records report the recoverable count honestly; rows already replaced by the former schema cannot be reconstructed without reprocessing the IQ file.

The sky plot consumes the selected position solution's final WLS satellite diagnostics. Azimuth/elevation, residual, standardized residual, uncertainty, and solution-use flags are converted into the GUI view model and persisted with each epoch. The display uses zenith at center, horizon at the outer ring, north up, and east right. Markers are joined to the same epoch's channel snapshot by PRN and use semantic lock/solution colors. Satellites excluded before the final WLS diagnostics are not assigned fabricated geometry. Epoch rows written by versions before satellite persistence deserialize with an empty geometry list and require IQ reprocessing to populate the plot.

Continuous PVT applies a terrestrial position-quality gate before a mathematically converged WLS candidate can enter velocity, Hatch, Kalman, persistence, or the visible position series. This is required for exactly determined four-satellite epochs: one millisecond-class pseudorange error can be fitted with nearly zero residual while producing a receiver altitude above one million metres. The default profile accepts altitudes from `-2 km` to `20 km`, limits post-fit pseudorange residual RMS to `1 km`, and constrains inter-epoch displacement using a `12 km/s` maximum implied speed plus a `1 km` jump margin. The limits are configuration, not a static-position clamp. Historical rows written before this gate are defensively excluded from position, sky, and altitude views when they violate the same altitude range.

This vertical slice is usable. Remaining interaction work is explicit:

- add deeper Kalman/Hatch algorithm parameters only after they become behaviorally active receiver configuration rather than hard-coded defaults;
- extend visual regression beyond the implemented desktop, compact-drawer, and narrow screenshot matrix when additional panels are introduced.

The first GUI exposes only configuration fields that are implemented and behaviorally active in the receiver. Signal type currently offers only GPS L1 C/A. It does not expose a nonfunctional FLL-assist toggle or invented NWPR coherent/noncoherent counts; current C/N0 controls map to the implemented prompt-window length and smoothing factor. Planned receiver capabilities remain in development documentation until their backend behavior and tests exist.

All current `GuiProcessingSettings` fields are editable in a draft. The compact command title bar shows the selected source and its sample-rate, IF, and IQ-format summary; editing is centralized in the detailed drawer, which exposes one focused category at a time: Input, Acquisition, Tracking, PVT, or Performance. Numeric text is parsed into the field's concrete Rust type without implicit clamping. Malformed text and receiver-domain validation are reported separately in Simplified Chinese and both disable queue creation. Historical snapshots are read-only until explicitly copied into a new draft, preserving run reproducibility.

## Processing Run Library

The left sidebar represents a persistent local processing run library, not an in-memory recent-files list. Every processing attempt creates a distinct run with an immutable configuration snapshot, lifecycle status, source IQ identity, timestamps, summary, PVT epoch history, and per-PRN diagnostics aligned to the PVT output epochs. Reprocessing the same IQ file creates another run so results remain reproducible and comparable.

Completed runs remain inspectable after application restart. The library stores derived results and source-file metadata, but does not copy the original IQ file. A missing or moved IQ source must not prevent viewing results that were already persisted.

The history rail supports filename/path search, status filtering, deletion, and source-file reveal. Queued runs expose move-up, move-down, and cancel controls, with UUID order persisted in preferences across restarts.

The library uses a versioned SQLite database in the user's application-data directory, as recorded by ADR-0003. Runs and derived observations are written incrementally so an interrupted process can recover an accurate terminal or resumable status without rebuilding a history index from loose files.

Each persisted PVT output epoch owns the channel-diagnostic snapshot for that same epoch: active PRNs, C/N0, Doppler, acquisition/tracking/navigation state, and available residuals. Changing `pvt_output_rate_ms` therefore changes both the persisted PVT cadence and diagnostic-chart cadence. Discrete acquisition, loss, reacquisition, navigation-lock, failure, and completion events are persisted immediately between epochs.

## Processing Queue

The first GUI supports one active processing run and any number of persisted queued runs. Starting another file while processing is active creates a queued run rather than another receiver worker. Users may cancel or reorder queued runs; completion, failure, or cancellation of the active run starts the next queued run automatically.

This is a resource policy, not a viewing restriction: users may continue browsing completed runs while the single active worker processes in the background.

## Cancellation and Interruption

The first GUI does not checkpoint receiver internals for resume. Cancellation is cooperative at an IQ read-chunk boundary; persisted PVT epochs, channel diagnostics, and events remain viewable and the run becomes `Cancelled`. On application startup, any run left in `Processing` becomes `Interrupted` while retaining its partial results.

Retrying a cancelled, interrupted, or failed run creates a new run with a new identity and starts from the beginning of the IQ file. It never mutates the historical run or attempts to restore DLL/PLL, bit synchronization, navigation decoding, Hatch, or Kalman state.

## First Screen

GNSScope opens directly into the receiver workspace, not a landing page.

- Left settings rail: IQ file, sample rate, IF, IQ format, optional RINEX nav file, colocated nav discovery, environment override, smoothing, Kalman, integer-ms ambiguity resolution.
- Main position pane: current latitude, longitude, altitude, ECEF coordinates, solution kind, convergence state.
- Velocity pane: ENU velocity, speed, horizontal speed, vertical speed, heading, clock drift, Doppler residual RMS.
- Time pane: receiver SOW, corrected GPS SOW, GPS week when available, clock bias, clock drift.
- Quality pane: satellites used/excluded, residual RMS, standardized residual RMS, HDOP/GDOP, horizontal/vertical sigma.
- Lower diagnostics: per-channel C/N0, Doppler, lock state, and residual chart.

The main workspace is a single scrollable overview rather than top-level `Overview`, `Signals`, or `PVT` tabs. Its stable order is: task command bar, processing status/progress, sky plot plus ENU position view, full-width channel/C/N0 diagnostics, velocity and altitude histories, and selected-epoch PVT details. The shared epoch timeline is a fixed bottom bar outside the scroll container, so it remains operable at every vertical scroll position. A persistent interface preference and header icon can hide or restore it without changing processing-run configuration. Height history reports mean, population standard deviation, and valid range. Receiver-event history remains persisted for diagnostics but is not shown in the primary workspace. Core correlations remain visible in one navigation context.

Major plots expose a focus command that temporarily expands one visualization to the work area and restores the overview without losing the selected run, epoch, PRN, or live-follow state. Segmented controls inside a plot may change its analytical mode; they do not replace top-level navigation.

The recommended initial window is `1440 x 900`. A client-decorated `gpui-component::TitleBar` provides the draggable region, double-click maximize/restore behavior, and native minimize, maximize/restore, and close controls. Below `1180 px`, sky/position and velocity/altitude plots stack and metric rows reflow; the settings drawer narrows from `470 px` to `340 px`. Below `840 px`, the run library becomes a bounded, vertically scrollable top region so the main workspace retains usable plot width. Typography does not shrink to force content into narrow widths; vertical scrolling and plot-focus mode preserve legibility. The fixed epoch bar stays outside the main scroll container at every breakpoint. Mobile layout is outside the first release scope.

`GNSSCOPE_WINDOW_WIDTH`, `GNSSCOPE_WINDOW_HEIGHT`, and the test-only startup switch `GNSSCOPE_OPEN_SETTINGS` make visual states deterministic. `scripts/capture-gui-layouts.ps1` builds the release GUI, captures desktop, compact-drawer, and narrow windows directly by native window handle, verifies nonempty PNG output, and terminates each spawned process. Generated images live under ignored `artifacts/gui-layouts/`.

## Processing Run Sidebar

The persistent left sidebar is approximately `280-320 px` wide and has three ordered regions: the pinned active run, queued runs, and history grouped by date. The active row shows filename, live state, compact progress, and percentage. Queued rows show queue order and may be reordered or cancelled. Historical rows show filename, signal type, creation time, terminal status, and source signal duration; detailed PVT values remain in the main workspace.

History supports filename search and terminal-status filtering. Row actions include viewing results, duplicating configuration into a new run, revealing the source IQ file, and deleting the derived run record. Deletion never deletes the source IQ file. The sidebar may collapse to a narrow task-status rail, but complete history remains available through expansion rather than being reduced to ambiguous icons.

## Processing Progress

Determinate progress is computed from processed IQ samples divided by total file samples and must be monotonic. PVT availability, acquisition success, and channel loss do not change file-processing progress. Indeterminate motion is reserved for short phases before the file length is known.

The main run header combines a refined progress rail, tabular percentage, processed/total signal duration, and a concise phase label such as opening, initializing, acquiring, tracking/navigation, solving, or finalizing. The active sidebar row uses a compact progress rail and percentage. A spinner is not shown continuously beside determinate progress.

Progress styling must remain precise in both themes: stable geometry, restrained accent fill, no rainbow gradient or glow, smooth transform-based interpolation, and reduced-motion support. Percentage alignment must not shift as digits change, and status text must remain readable at narrow window widths.

## Sky Plot

The constellation visualization is a receiver-centered sky plot: zenith at the center, horizon at the outer ring, north at the top, and east on the right. It is driven by the selected PVT epoch and uses the per-satellite azimuth/elevation already produced by WLS diagnostics.

- Every visible satellite has a readable PRN label.
- Color communicates channel/PVT state using the same semantics in light and dark themes.
- Marker size or a restrained outer ring may communicate C/N0.
- PVT-used and RAIM-excluded satellites must remain distinguishable without relying on color alone.
- Hover or selection reveals C/N0, Doppler, residual, elevation, azimuth, and lock state.
- Selecting a historical epoch updates the sky plot and all time-aligned diagnostics together.
- The plot must remain legible with overlapping satellites, low elevation, missing ephemeris, and 32 visible PRNs.

The sky plot should read as a precise instrument, not a decorative illustration: no glow-heavy markers, orbital artwork, 3D globe, or excessive animation. Motion is limited to interpolated epoch changes, selection, and status transitions, with reduced-motion support.

## C/N0 Presentation

The primary signal-quality view is a dynamic vertical bar chart with stable PRN 1-32 positions and C/N0 in dB-Hz. It draws bars only for channels whose epoch state is `Healthy`; probation, suspect, lost, reacquiring, and absent channels remain visible in channel-state diagnostics but do not enter the stable C/N0 comparison. Empty PRN slots preserve axis stability without implying a measurement. The active run updates bars at each PVT output epoch; moving the historical epoch cursor restores the bar snapshot for that time.

Selecting a bar links the PRN across the sky plot and channel details, and reveals that PRN's C/N0 history below the bar chart. A small set of PRNs may be pinned for historical comparison. State is not encoded by arbitrary per-PRN rainbow colors: acquisition/tracking/navigation/PVT state uses semantic markers and styling, while selection uses the primary interaction accent.

## Settings Contract

`gnss_gui::GuiProcessingSettings` is the GUI-facing source of truth.

A run receives an immutable processing-configuration snapshot when it is queued. Queued, active, completed, failed, cancelled, and interrupted runs expose that snapshot read-only. Changing settings for an existing run is expressed as "process again with this configuration": duplicate the snapshot into an editable draft, then create a new run with a new identity.

Application display preferences such as light/dark theme are not processing configuration and are not stored in the run snapshot. Changing defaults affects only future drafts, never queued or historical runs.

The custom window title bar and task toolbar form one compact command surface. It contains product identity, source selection and summary, live/timeline/settings actions, the essential start or cancel command, and the native minimize/maximize/close controls. Historical reprocessing uses an icon-only command with a hover hint. Receiver settings open in a responsive `360-500 px` right-side drawer so the technical workspace remains visible and the title bar never becomes a form row.

The secondary-settings drawer uses a fixed header, compact category navigation, one scrollable category body, and a fixed draft-status footer. Input contains sampling, IQ, navigation, and prior position; Acquisition contains search and reacquisition scheduling; Tracking contains DLL/PLL, C/N0, and channel admission; PVT contains output cadence, smoothing, Kalman, integer-ms ambiguity, and RAIM; Performance contains parallel scheduling. Queued and historical snapshots reuse the same categories in read-only mode with a command to duplicate the configuration into a new draft.

Standard mode exposes the commonly useful controls in those groups: IQ/chunk settings, acquisition integration and threshold, DLL/PLL bandwidth, lock and minimum C/N0 thresholds, C/N0 window/smoothing, PVT epoch and Doppler averaging, RINEX policy, Hatch/Kalman/integer-ms toggles, and parallel acquisition/tracking. Expert mode additionally exposes Doppler search details, acquisition/reacquisition scheduling, correlator/loop details, full channel-admission thresholds, WLS/RAIM and sigma calibration, Hatch/Kalman internals, GPS-week reference, and navigation timing parameters.

Expert-mode visibility is an application preference and is not part of a processing configuration. Every parameter value used by the receiver is part of the immutable run snapshot. Each category can restore its own defaults; risky expert fields use concise inline warnings rather than repeated blocking dialogs.

New run drafts support built-in `GPS L1 C/A Standard` and `CTTC Spain 4 Msps / Int16` presets, a custom state, and later user-saved presets. Selecting a preset copies values into the draft; edits mark it modified without creating a live dependency on the preset. Preset updates never change queued or historical snapshots, and filename matching may offer a non-blocking suggestion but must not silently apply dataset-specific settings.

- `iq_file_path`: required to process; nav is optional.
- `sample_rate_hz`: defaults to 4 MHz for the CTTC dataset.
- `intermediate_frequency_hz`: defaults to zero IF.
- `iq_format`: supports the current int I/Q modes.
- `rinex_nav_path`: explicit GUI setting for a RINEX nav file.
- `use_rinex_nav_environment`: allows `GNSSCOPE_RINEX_NAV` as a development/batch override.
- `auto_discover_colocated_nav`: finds nav files beside the IQ file.

Navigation file priority is:

1. Explicit GUI path.
2. Environment variable override.
3. Colocated nav file beside the IQ file.
4. IQ-only mode.

IQ-only mode is valid and should show a precision/status warning instead of blocking processing.

The resolved navigation source is exposed separately through `GuiNavSourceStatus`:

- `IqOnly`: no navigation file has been resolved; processing remains valid.
- `ExplicitPath`: the GUI-selected RINEX nav file is active.
- `Environment`: `GNSSCOPE_RINEX_NAV` or the configured environment override is active.
- `Colocated`: a nav file beside the IQ file has been auto-discovered.

The status also carries the resolved path when present and whether Klobuchar ionosphere parameters are available.
`GuiProcessingSettings::nav_source_status_for_iq_path` resolves the same optional source policy used by processing and can feed a `GuiReceiverEvent::NavSourceUpdated` event before or during processing.

## PVT Output Contract

`gnss_core::pvt::PvtEpochOutput` is the backend output. `gnss_gui::GuiPvtViewModel` converts it into GUI display units.
For navigation data, long captures or full RINEX files should keep multiple records per PRN in `EphemerisCatalog`; `ContinuousPvtRunner::solve_epochs_with_catalog` selects the closest healthy TOE/TOC record for each epoch. Position and Doppler velocity solvers should consume all satellites that survive filtering instead of truncating to a fixed first-N subset.

- Position display uses degrees for latitude/longitude and meters for altitude.
- Speed display uses m/s.
- Clock bias display uses microseconds.
- Clock drift display uses ppm in the GUI model, with raw m/s available in velocity.
- Quality display keeps PRN lists and DOP/sigma values numeric so charts can reuse them.

The main workspace renders only the final selected solution using the receiver preference `Kalman > Smoothed WLS > Raw WLS`, with an explicit solution-kind label. Default plots never overlay all candidate solutions. `PvtEpochOutput`, the GUI view model, and the persisted epoch record store every available Raw, Smoothed, and Kalman candidate plus the final selection so expert details can compare position, velocity, residual, and uncertainty without reprocessing the IQ source.

### Candidate Comparison Drawer

Candidate comparison is an implemented read-only expert view for the currently selected epoch. An icon action in the PVT result header opens a right-side drawer; it is mutually exclusive with the advanced-settings drawer, and opening either drawer causes the main workspace to reflow. The persistent bottom epoch timeline remains visible, and changing its cursor updates the comparison synchronously.

The drawer presents Raw, Smoothed, and Kalman as aligned comparison columns or rows with stable dimensions. It identifies the receiver-selected candidate and compares latitude/longitude or local ENU delta, altitude, ECEF position, 3D/horizontal/vertical velocity, heading, receiver clock bias, clock drift, residual RMS, DOP/sigma, used PRNs, excluded PRNs, and delta from the selected solution. Missing numeric values render as unavailable, never as zero. A compact disposition label distinguishes selected, available but lower priority, non-converged, rejected by a quality gate, and unavailable; this requires structured backend data where the current persisted candidate record is insufficient.

The drawer never changes a completed result's selected solution. A future manual candidate preference, if required, belongs to processing configuration and creates a new run; it must not mutate historical output. Default position, altitude, velocity, and clock plots continue to show only the receiver-selected series.

`scripts/capture-gui-layouts.ps1` exercises dark and light settings, a dense Tracking category, candidate comparison at `1440 x 900` and `1100 x 800`, and the narrow workspace using deterministic preview data. The preview hook is environment-only and does not alter the production receiver or run library.

Sky markers expose hover detail for PRN, C/N0, azimuth, elevation, residual, and PVT use. Clicking a sky marker or healthy C/N0 column pins the PRN and links its highlight across both panels. Every plot panel supports a full-width focus mode with an icon-only enter/exit control.

## Application State

`gnss_gui::GuiReceiverWorkspaceState` is the UI state container for the first gpui screen.

Application state must distinguish `active_run_id` from `selected_run_id`. Receiver events update the active run in the processing run library even when a historical run is selected; they must never overwrite the historical view model currently shown in the main workspace.

- Status lifecycle: idle, ready, processing, completed, failed.
- `settings_issues` drives disabled processing controls and inline validation messages.
- Queue creation normalizes dataset compatibility settings first, then runs the same authoritative validation used by the state model and settings drawer; configuration builders no longer silently make an invalid draft appear runnable.
- `progress_fraction` is a normalized 0-1 processing progress value.
- `latest_pvt` is the newest `GuiPvtViewModel`.
- `channels` feeds the channel diagnostics panel.
- `apply_event` accepts backend/worker events so the GUI updates state without parsing logs or calling receiver internals directly.

While processing is active, users may select and inspect any completed run without pausing the worker. The active run stays pinned at the top of the sidebar with live progress, a narrow global activity indicator remains visible, and a clear command returns the workspace to the live run. Completion updates the sidebar without forcing navigation away from the selected historical run.

## Epoch Timeline

A PVT output epoch is a complete time-aligned solution snapshot, not only a position point. It contains position, ECEF/ENU velocity, speed, receiver time, UTC, quality, used/excluded satellites, sky-plot geometry, and the per-PRN diagnostic snapshot for that epoch.

Completed runs initially select their last valid PVT epoch. A shared timeline cursor synchronizes the sky plot, numeric position/velocity/time values, current PRN states, and all diagnostic charts. Historical charts retain the complete run range and highlight the selected epoch. Clicking a chart point or moving the timeline selects another epoch.

Active runs follow the latest epoch by default. Manual timeline movement exits live-follow mode without stopping processing; a clear "Return to live" command restores it. Runs without a valid PVT solution still expose acquisition/tracking epochs while position, velocity, and time show an unavailable state.

## Velocity Presentation

The velocity panel combines selected-epoch values with complete-run history. Persistent headline values include 3D speed, horizontal/vertical speed, ENU components, heading, receiver clock drift, and Doppler residual RMS. A compact segmented control switches the history chart between `Speed`, `ENU`, and `Clock drift`; every mode shares the global epoch cursor and highlights the selected sample. Aggregate statistics are secondary summaries and never replace the epoch solution.

## Time Presentation

Corrected UTC is the primary selected-epoch time and is always labeled explicitly, for example `2013-04-04 06:24:07.029 UTC`; it is not silently converted to the operating-system timezone. GPS week, corrected GPS SOW, receiver SOW, receiver clock bias in microseconds, and clock drift in ppm are secondary technical values, with raw clock-drift m/s available in details. Clock bias/drift may be charted over epochs, while calendar time remains a value rather than a plotted series.

When a full GPS week is unavailable, the panel displays `UTC unavailable`, continues to show available GPS SOW, and identifies the missing week anchor. It never guesses an absolute date from a truncated broadcast week.

## Position Presentation

The primary position visualization is a local ENU horizontal scatter/track plot whose stable origin is the first valid PVT epoch. Every valid PVT epoch contributes one East/North point and the selected epoch uses a high-contrast outline plus crosshair. Static captures naturally form a precision cloud; dynamic captures add a restrained line between consecutive points to reveal the path.

Absolute latitude, longitude, altitude, and ECEF remain visible as selected-epoch numeric values. A segmented position view switches between `Horizontal Track` and `ENU vs Time`; altitude is shown in the time-series view rather than projected into the horizontal plane. Both modes share the global epoch cursor. A geographic map is a later enhancement and must not replace the ENU precision view.

The altitude history preserves the raw accepted WLS curve and overlays a 10-point arithmetic moving-average trend. Its summary labels raw population sigma and 10-point population sigma separately. The moving average is a presentation statistic compatible with the supplied GNSS-SDR CTTC configuration; it is not a replacement PVT solution and must not bridge rejected or missing epochs.

RAIM fault detection and exclusion is an advanced PVT toggle and defaults to disabled. Disabling it sets the WLS maximum sequential exclusions to zero; enabling it restores the solver's configured residual thresholds and exclusion limit. Physical position gates and integer-millisecond ambiguity resolution remain independent safeguards. Dataset presets may provide a coarse initial position to bootstrap integer-millisecond ambiguity resolution without RAIM; the prior is replaced by the first accepted solution and is never reported as an observation-derived result.

Channel diagnostics should show:

- PRN
- acquisition state
- tracking lock
- navigation lock
- C/N0
- Doppler
- latest residual when available

## Implementation Notes

The first gpui implementation should keep signal processing on a worker thread and send immutable snapshots of `GuiPvtViewModel` to the UI thread. Avoid parsing console output. Avoid putting receiver settings in environment variables except as explicit override toggles.

Current GUI-facing Rust modules:

- `gnss_gui::GuiReceiverEvent`: stable event bridge from settings changes, resolved navigation source, worker progress, channel updates, PVT updates, completion, and failure into UI state.
- `gnss_gui::GuiNavSourceStatus`: resolved navigation source status for GUI settings and processing diagnostics.
- `gnss_gui::GuiProcessingSettings`: receiver settings and navigation-file policy.
- `gnss_gui::GuiPvtViewModel`: position, velocity, time, quality, and per-satellite WLS geometry display model.
- `gnss_gui::GuiDiagnosticEvent`: persisted acquisition, reacquisition, channel-state, and navigation event model.
- `gnss_gui::GuiReceiverWorkspaceState`: processing lifecycle, settings validation, latest PVT, and channels.
- `gnss_gui::ReceiverWorkspaceLayout`: first-screen panel layout.
- `gnss_gui::build_receiver_workspace_screen`: structured pre-render screen model with typed controls, metrics, and channel tables.
- `gnss_gui::sample_pvt_view_model`: static CTTC-style preview data for the first GUI render before processing starts.
- `gnss_gui::render_workspace_snapshot`: deterministic text renderer used as a golden reference for first-screen information architecture before gpui rendering is wired.

The snapshot renderer is not the final UI. It exists so the graphical renderer can be checked against a stable list of required sections: settings, position, ECEF, velocity, clock drift, time, quality, and channel diagnostics.

The worker adapter and first visual workspace are implemented. Full contract completion remains gated by the explicit Current Readiness items.

## Completed First Implementation Milestone

The first milestone is complete as a real vertical slice rather than a disposable static prototype. It opens a GPUI window, creates and persists processing runs, executes real IQ through `StreamingPvtRunner` on the single worker, reports progress and cancellation, persists epochs incrementally, updates the live workspace, and reopens completed results from SQLite after restart. Fixtures remain appropriate for unit and visual tests, but the shipped application path does not depend on preview-only data.
