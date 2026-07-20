# PVT Optimization Audit

See `docs/receiver-readiness-audit.md` for the current production gaps and runtime optimization backlog.

Current status: positioning, velocity, and absolute-time correctness are implemented and verified on CTTC real data. Streaming input, deterministic parallel acquisition/tracking, periodic reacquisition scheduling, and the GUI-facing state contracts are implemented; the real GUI worker adapter is not yet wired.

## User Checklist

| Item | Status | Evidence |
| --- | --- | --- |
| Build a real Observables layer instead of manual pseudorange assembly in tests | Done | `pvt/pseudorange.rs` defines `ObservablesInputBuilder`, `ObservablesBuilder`, `TowSample`, `TrackingTimestampObservation`, and `ObservablesSource`. `tests/pvt_e2e.rs` uses these builders for common receive epochs and continuous epochs. |
| Use common receive epoch and TOW history interpolation | Done | `interpolate_tow_sow` handles ordered TOW history and GPS week wrap. `from_tow_history_common_receive_epoch` and `from_tracking_timestamp_common_receive_epoch` form pseudoranges at common receive time. |
| Fix tracking sample counter start from acquisition stamp | Done | `AcquisitionResult::sample_index`, `PcpsAcquisition::acquire_at_sample`, and `DllPllTracking::new(..., start_sample_index)` preserve the absolute acquisition sample stamp. |
| Use `rem_code_phase_samples` for GNSS-SDR-style sub-sample receive time | Done | `TrackingOutput::rem_code_phase_samples` is produced by tracking and consumed by `TrackingTimestampObservation`; e2e records and uses the fractional sample remainder. |
| Apply NWPR C/N0 within the same navigation bit | Done | `DllPllTracking::compute_cn0_nwpr` uses narrowband/wideband prompt power. After bit sync, `cn0_window_len` clamps the estimator window to one 20 ms bit and `update_cn0` resets at bit boundaries. |
| Parse and apply Klobuchar ionosphere parameters | Done | `parse_iono` parses subframe 4 page 18; `parse_rinex_ionosphere_params` parses RINEX 2/3 iono headers; `load_optional_rinex_iono` provides optional RINEX fallback; WLS applies Klobuchar when `iono.valid` is true. |
| Keep RINEX nav optional for raw IQ-only use | Done | `RinexNavConfig` has explicit/env/colocated candidates and falls back to IQ-only. `GuiProcessingSettings::nav_source_status_for_iq_path` exposes the resolved source without requiring a nav file. Environment variables are retained as an override, not the primary GUI configuration path. |
| Add elevation mask and RAIM / residual outlier rejection | Done | `WlsConfig` includes elevation mask and RAIM thresholds; `raim_exclusion_candidate` excludes residual outliers only when redundancy exists. |
| Improve C/N0-to-ranging variance model | Done | WLS uses a sigma model combining DLL thermal jitter, C/N0-scaled code floor, elevation, missing iono uncertainty, and observed tracking jitter instead of the old rough `10^((cn0-30)/10)` weight. |
| Carrier smoothing pseudorange | Done | `HatchSmoother` smooths by PRN and `ContinuousPvtRunner` validates smoothed WLS residual quality before accepting it. |
| Doppler velocity output | Done | `solve_doppler_velocity` and `PvtVelocityOutput` expose ECEF/ENU velocity, speed, heading, clock drift, residual RMS, and used PRNs. The solver now uses all observations instead of truncating to the first 12. |
| Kalman filter for continuous positioning | Done | `PositionVelocityKalman` runs from continuous PVT results and `PvtEpochOutput` prefers converged Kalman solutions. |
| Time output | Done | `PvtTimeOutput` exposes receiver/corrected GPST, GPS week, structured UTC, Unix seconds, transmit SOW bounds, receiver clock bias, and clock drift; leap seconds and truncated broadcast-week expansion are tested. |
| Streaming IQ input | Done | `StreamingPvtRunner` reads bounded chunks with reusable conversion buffers and advances acquisition, tracking, navigation, and PVT without full-file loading. |
| Parallel acquisition/tracking | Done | Acquisition is PRN-parallel and active tracking uses Rayon with deterministic PRN-ordered merge; serial/parallel tracking equivalence is tested. |
| Periodic reacquisition | Implemented, acceptance pending | `AcquisitionScheduler` prioritizes `Lost` PRNs at `reacq_interval_ms`; `StreamingPvtRunner` executes scans and reinitializes detected channels. A real forced-loss/recovery test is still missing. |
| Parse full RINEX GPS navigation ephemerides | Done | `parse_rinex_ephemeris_catalog` parses GPS RINEX 2 `.n` records and GPS records in RINEX 3 nav files into `EphemerisCatalog`; `load_optional_rinex_nav_capabilities` resolves optional nav files while preserving IQ-only operation. |
| Multi-ephemeris selection for long RINEX/captures | Done | `EphemerisCatalog` stores multiple ephemerides per PRN; `EphemerisSelectionConfig` selects healthy, non-stale records nearest TOE/TOC; `ContinuousPvtRunner::solve_epochs_with_catalog` applies selection per epoch. |
| GUI pre-design module contract | Done | `gnss-gui` exposes settings, nav-source status, PVT view model, receiver state, event bridge, structured screen model, and deterministic snapshot renderer. `GuiNavSourceStatus` reports both ionosphere and ephemeris availability. |

## Verification

Latest verification commands run through `rtk`:

- `cargo fmt --check`: passed.
- `cargo test --workspace`: passed, 177 tests with 1 ignored test.
- `cargo clippy --workspace --tests -- -D warnings`: passed with no issues.
- `cargo test --release -p gnss-core --test pvt_e2e -- --nocapture`: passed on real CTTC data.
- `streaming_runner_real` passed the 60 s production stream: 5 acquisitions, 32 nav subframes, 410 PVT outputs, and UTC `2013-04-04 06:24:07.029`.

The real-data e2e covers acquisition, tracking, bit sync, nav decode, optional RINEX ionosphere fallback, observables, WLS, Doppler velocity, continuous PVT, Hatch smoothing, Kalman output, and GUI-facing PVT output conversion. RINEX ephemeris parsing is covered by fixed-width field unit tests, including glued negative fields common in broadcast nav files.

## Remaining Production Hardening

These are not blockers for the current optimization goal, but they are the next sensible production tasks:

- Wire `StreamingPvtRunner` into a real GUI worker that emits `GuiReceiverEvent` snapshots, handles cancellation, and reports completion/failure.
- Calibrate the WLS sigma model with more datasets and receiver front-end settings.
- Add a forced-loss/reacquisition real-data regression and expose reacquisition counters/events to the GUI.
- Expose streaming controls such as PVT epoch rate, chunk size, and parallel tracking in `GuiProcessingSettings`.
- For segmented GUI processing, pass nonzero acquisition sample stamps when acquisition windows do not start at file sample 0.
- Extend RINEX support beyond GPS `.n`/GPS RINEX 3 records if future datasets need multi-constellation navigation files.
