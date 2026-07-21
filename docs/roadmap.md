# GNSScope Roadmap

Date: 2026-07-21

This is the authoritative product roadmap. Detailed receiver evidence remains in `docs/receiver-readiness-audit.md`; this file records priority and sequencing rather than repeating implementation notes.

## Current Baseline

- Streaming IQ, deterministic parallel acquisition/tracking, periodic reacquisition scheduling, continuous PVT, velocity/time output, editable configuration, history replay, and candidate persistence are implemented.
- Raw / Smoothed / Kalman selected-epoch comparison is implemented as a read-only responsive drawer.
- CTTC release GUI processing persists 810 epochs; targeted tests pass for `gnss-core` and `gnss-gui`.
- The 2.67 GB LabSat signed Int8 I/Q capture at 10.23 Msps now has acquisition, tracking, navigation, PVT, velocity, and UTC regression coverage under the normal GUI admission profile. Capture and tracking both retain the configured sample rate; an attempted automatic acquisition decimation was removed after real data showed degraded bit synchronization. Costas-compatible carrier-lock detection, per-channel 2 s pull-in, and a GNSS-SDR-aligned 50-epoch code-lock failure allowance prevent premature channel removal. Full-constellation cold acquisition and multi-channel reacquisition rotate fairly across configured per-scan batches instead of repeatedly starving higher-numbered PRNs.

## Priority Roadmap

### 1. GUI polish and PVT recovery hardening

Complete the first production-facing GUI pass before broad performance work:

- Polish light/dark themes, spacing, typography, icon hierarchy, hover guidance, chart labels, empty states, and drawer transitions.
- Review the candidate comparison drawer, settings drawer, fixed epoch timeline, title-bar controls, and narrow/compact layouts against the screenshot matrix.
- Completed: a release-only real-IQ stress regression forces G01/G11/G17 to `Lost` after converged epoch 79, records three non-converged PVT outputs, reacquires and restores all three channels, and resumes converged PVT at epoch 85 with four satellites.
- Keep the GUI read-only for completed results; visual polish must not change receiver selection or persisted values.

**Acceptance:** complete. The forced-loss PVT recovery evidence passes; the GUI now has synchronized component/custom light-dark themes, category-focused receiver configuration, localized validation, compact narrow-screen history, and stable desktop, compact, tracking, light-theme, narrow, and comparison screenshots.

Regression command: `cargo test --release -p gnss-core --test streaming_runner_real test_streaming_runner_real_data_resumes_pvt_after_multi_channel_reacquisition -- --ignored --nocapture`.

### 2. Accuracy and scenario coverage

- Completed: the LabSat `Int8` regression now covers acquisition, tracking, navigation decode, position, velocity, and UTC output against the supplied coarse reference.
- Add incomplete-final-1 ms epoch coverage.
- Validate dynamic/high-rate PVT, rapid Doppler changes, shortened output epochs, and velocity recovery.
- Add independent position/velocity/time comparisons and broaden beyond the five-PRN CTTC capture.
- Exercise long captures across ephemeris changes, GPS week boundaries, and leap-second conversion boundaries.
- Automate serial-versus-parallel equality for the complete real-data PVT stream.

**Acceptance:** each dataset has documented expected coverage, convergence, accuracy, and runtime results; failures identify the affected receiver stage.

### 3. Measure runtime and memory

- Capture CPU profiles and phase timings for acquisition startup, steady-state tracking, navigation/PVT, event generation, and total runtime.
- Record peak working set/private bytes for serial and parallel CTTC runs.
- Keep a repeatable release benchmark so optimization decisions use measured data.

**Acceptance:** benchmark artifacts are committed or reproducibly generated, and each optimization has before/after correctness and runtime evidence.

### 4. Optimize measured bottlenecks

Apply changes in this order, re-running deterministic real-data regressions after each change:

1. Incremental code-phase recurrence and equivalence thresholds.
2. Bulk contiguous acquisition circular buffer.
3. Reusable acquisition FFT plans, PRN spectra, and work buffers.
4. Multi-millisecond tracking dispatch or persistent bounded workers.
5. Smaller ephemeris, ionosphere, snapshot, and per-epoch allocation reductions.

## Longer-Term Capabilities

These are intentionally after the priority roadmap:

- Last-known-Doppler narrowing for reacquisition.
- Persistent acquisition worker/cache.
- Live SDR input, real-time deadlines, and dropped-sample recovery.
- Multi-constellation signals and navigation beyond implemented GPS records.
- Automatic absolute-week anchoring for raw IQ-only captures without RINEX or a configured reference week.

## Deferred GUI Configuration

- Deeper Kalman/Hatch parameters only after they become behaviorally active receiver configuration.
- Additional visual regression scenarios when new panels are introduced.

## Current Next Action

Priority 1 is complete. Continue with Priority 2 accuracy and scenario coverage by adding incomplete-final-epoch coverage, then dynamic datasets.
