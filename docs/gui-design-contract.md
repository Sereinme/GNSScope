# GNSScope GUI Design Contract

Receiver integration gates and performance work are tracked in `docs/receiver-readiness-audit.md`.

This document defines the first GUI boundary before wiring gpui widgets. The GUI should consume structured state from `gnss-gui` instead of reading test-only logs.

## Current Readiness

The backend is ready for GUI integration: `StreamingPvtRunner` has bounded IQ streaming, deterministic parallel acquisition/tracking, periodic lost-channel reacquisition, navigation decoding, continuous PVT, and structured UTC output. The remaining integration boundary is a worker adapter that owns the runner, converts `StreamingPvtReadStep`/`StreamingPvtEvent` into `GuiReceiverEvent`, and provides cancellation and failure handling.

The current GUI settings model does not yet expose `chunk_ms`, `pvt_output_rate_ms`, Doppler epoch averaging, or parallel acquisition/tracking. Those fields must be added before the GUI can claim full runtime configurability.

## First Screen

GNSScope opens directly into the receiver workspace, not a landing page.

- Left settings rail: IQ file, sample rate, IF, IQ format, optional RINEX nav file, colocated nav discovery, environment override, smoothing, Kalman, integer-ms ambiguity resolution.
- Main position pane: current latitude, longitude, altitude, ECEF coordinates, solution kind, convergence state.
- Velocity pane: ENU velocity, speed, horizontal speed, vertical speed, heading, clock drift, Doppler residual RMS.
- Time pane: receiver SOW, corrected GPS SOW, GPS week when available, clock bias, clock drift.
- Quality pane: satellites used/excluded, residual RMS, standardized residual RMS, HDOP/GDOP, horizontal/vertical sigma.
- Lower diagnostics: per-channel C/N0, Doppler, lock state, and residual chart.

## Settings Contract

`gnss_gui::GuiProcessingSettings` is the GUI-facing source of truth.

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

## Application State

`gnss_gui::GuiReceiverWorkspaceState` is the UI state container for the first gpui screen.

- Status lifecycle: idle, ready, processing, completed, failed.
- `settings_issues` drives disabled processing controls and inline validation messages.
- `progress_fraction` is a normalized 0-1 processing progress value.
- `latest_pvt` is the newest `GuiPvtViewModel`.
- `channels` feeds the channel diagnostics panel.
- `apply_event` accepts backend/worker events so the future GUI can update state without parsing logs or calling receiver internals directly.

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
- `gnss_gui::GuiPvtViewModel`: position, velocity, time, and quality display model.
- `gnss_gui::GuiReceiverWorkspaceState`: processing lifecycle, settings validation, latest PVT, and channels.
- `gnss_gui::ReceiverWorkspaceLayout`: first-screen panel layout.
- `gnss_gui::build_receiver_workspace_screen`: structured pre-render screen model with typed controls, metrics, and channel tables.
- `gnss_gui::sample_pvt_view_model`: static CTTC-style preview data for the first GUI render before processing starts.
- `gnss_gui::render_workspace_snapshot`: deterministic text renderer used as a golden reference for first-screen information architecture before gpui rendering is wired.

The snapshot renderer is not the final UI. It exists so the graphical renderer can be checked against a stable list of required sections: settings, position, ECEF, velocity, clock drift, time, quality, and channel diagnostics.

Visual design can begin now; end-to-end GUI processing remains pending the worker adapter and streaming-control settings.
