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
- Completed: a release-only real-IQ stress regression forces G01/G11/G17 to `Lost` after converged epoch 79, records three non-converged PVT outputs, reacquires and restores all three channels, and resumes converged PVT at epoch 105 with four satellites after the normal 2 s pull-in. The recovery test loads colocated RINEX to isolate channel recovery; the LabSat full-file regression separately verifies navigation-message-only PVT.
- Keep the GUI read-only for completed results; visual polish must not change receiver selection or persisted values.

**Acceptance:** complete. The forced-loss PVT recovery evidence passes; the GUI now has synchronized component/custom light-dark themes, category-focused receiver configuration, localized validation, compact narrow-screen history, and stable desktop, compact, tracking, light-theme, narrow, and comparison screenshots.

Regression command: `cargo test --release -p gnss-core --test streaming_runner_real test_streaming_runner_real_data_resumes_pvt_after_multi_channel_reacquisition -- --ignored --nocapture`.

### 2. Accuracy and scenario coverage

- Completed: the LabSat `Int8` regression now covers acquisition, tracking, navigation decode, position, velocity, and UTC output against the supplied coarse reference.
- Completed: a full-file LabSat A/B run with the same-day IGS broadcast navigation file reduced the final-20 Kalman altitude offset relative to the coarse INI value from `+10.832 m` to `+6.382 m`. Production defaults remain navigation-message-only; explicit RINEX selection and opt-in colocated discovery are retained for controlled fallback and comparison.
- Completed: files ending with an incomplete final 1 ms tracking epoch retain the exact final sample index but do not advance tracking or PVT cadence from stale observables.
- Validate dynamic/high-rate PVT, rapid Doppler changes, shortened output epochs, and velocity recovery.
- Add a real dynamic-IQ regression where a GPS satellite is absent at startup, becomes detectable later, completes navigation-message ephemeris decoding, and joins PVT without restarting the run.
- Add independent position/velocity/time comparisons and broaden beyond the five-PRN CTTC capture.
- Exercise long captures across ephemeris changes, GPS week boundaries, and leap-second conversion boundaries.
- Completed: a release-only 100 s CTTC regression compares serial acquisition/tracking with the parallel pipeline and requires identical event counts and structural equality for all 810 PVT outputs.

**Acceptance:** each dataset has documented expected coverage, convergence, accuracy, and runtime results; failures identify the affected receiver stage.

### 3. Measure runtime and memory

- Capture CPU profiles and phase timings for acquisition startup, steady-state tracking, navigation/PVT, event generation, and total runtime.
- Record peak working set/private bytes for serial and parallel CTTC runs.
- Keep a repeatable release benchmark so optimization decisions use measured data.
- Completed: the 100 s CTTC release baseline records 8.53-8.72 s serial and 8.95-9.09 s parallel before correlation changes, with 10.2-10.6 MiB peak RSS. Instruments identified per-sample code-phase modulo as the dominant avoidable cost. Incremental code-phase recurrence and scalar PRN multiplication reduce the final runtime to 5.40-5.45 s serial and 5.29-5.34 s parallel while preserving all 810 PVT epochs and real-data quality thresholds. `scripts/benchmark-cttc-macos.sh` reproduces the timing run; detailed evidence is in `docs/receiver-readiness-audit.md`.
- Completed: acquisition now plans FFTs once per engine, reuses an in-place work buffer, and advances carrier wipe-off by recurrence. The 32-PRN, 10-integration LabSat acquisition regression improves from about 0.39 s to 0.28 s, and the GUI-equivalent LabSat run improves from 38.61 s to 33.66 s with unchanged PVT. A fixed contiguous acquisition-ring experiment was measured and rejected because it regressed CTTC runtime.
- Completed: parallel PCPS now reuses the runner's bounded Rayon pool instead of creating scoped operating-system threads for every PRN scan. The same 130.5 s, 2.67 GB GUI-equivalent LabSat run fell from the immediate 90.38 s baseline to 47.65 s while preserving all 82 converged PVT outputs, the final `40.00150038 deg, 116.33019944 deg, 104.421 m` solution, and the final-20 altitude mean/sigma of `105.832/0.488 m`. A four-worker automatic pool retained the approximately 10 s first-PVT time while reducing scheduler system time versus the ten-thread global pool.
- Completed: PRNs with the same Doppler grid now share carrier wipe-off and the input forward FFT, with one bounded parallel dispatch across Doppler bins. Two full LabSat runs took 35.64-37.11 s versus the immediate 47.65 s baseline, a 22.1-25.2% reduction, while retaining the identical 82 converged PVT outputs and final solution/statistics above. The measured maximum RSS is 158.2 MiB with no swap. Different opt-in reacquisition grids remain isolated in deterministic groups.
- Completed: a post-PCPS Time Profiler run identified tracking and per-millisecond Rayon scheduling as the remaining hot path. Active tracking channels now use a PRN-sorted slice instead of `BTreeMap::par_iter_mut`, and navigation/ephemeris availability uses fixed stack tables instead of two heap vectors per millisecond. The default-parallel 100 s CTTC median improved from 4.223 s to 4.027 s while preserving all 810 PVT epochs; the 36.28 s LabSat result remains within the previous 35.64-37.11 s range, so no LabSat gain is claimed.
- Completed: after final-epoch hardening, two unprofiled full GUI-equivalent LabSat runs took 34.04-34.35 s with the identical 82 converged outputs and final solution. A direct precompiled test run measured 57.0 MiB maximum RSS, a 48.9 MiB peak memory footprint, and no swap; this narrower process-only measurement is not compared as a code improvement against earlier Cargo-wrapped RSS results. A 15 s attached Time Profiler trace collected 53,938 running-thread samples: tracking correlation was 46.29%, acquisition FFT kernels 32.48%, other PCPS work 5.85%, scheduling/synchronization 9.48%, and IQ I/O 0.68%.
- Completed: GPS L1 C/A chips are exactly `+1/-1`, so tracking applies them by flipping the correlator value's IEEE-754 sign bit instead of performing six floating-point multiplications per sample. Carrier wipe-off and recurrence now also use hardware fused multiply-add. The isolated real-IQ tracking benchmark median improved from 4.764 s before sign application to 4.295 s, then to 4.209 s with FMA, a cumulative 11.6% for 500,000 tracking epochs. The current 100 s CTTC medians are 4.438 s serial and 4.039 s parallel. Exact equality of all 810 serial/parallel CTTC PVT outputs and the unchanged 82-output full LabSat result passed; no LabSat runtime gain is claimed. `tracking_benchmark` provides the repeatable isolated release benchmark.

**Acceptance:** benchmark artifacts are committed or reproducibly generated, and each optimization has before/after correctness and runtime evidence.

### 4. Optimize measured bottlenecks

Apply changes in this order, re-running deterministic real-data regressions after each change:

1. Incremental code-phase recurrence and equivalence thresholds. Completed.
2. Bulk contiguous acquisition circular buffer. Measured and rejected after a runtime regression.
3. Reusable acquisition FFT plans, PRN spectra, and work buffers. FFT plans and work buffers are complete; cross-scan PRN spectrum caching was measured on LabSat and rejected because 10.34 s did not improve on the 10.12 s uncached run.
4. Multi-millisecond tracking dispatch or persistent bounded workers. A runner-owned bounded Rayon pool is complete; cross-millisecond batching remains deferred because acquisition, channel admission, navigation, and PVT have ordered 1 ms dependencies.
5. Smaller ephemeris, ionosphere, snapshot, and per-epoch allocation reductions. Per-millisecond PRN availability vectors and the `BTreeMap` parallel-iteration copy are removed. The fresh LabSat profile shows I/O and these smaller costs are no longer primary targets.
6. Tracking correlation remains the next compute area. Branchless PRN sign application and fused complex multiplication are complete and measured. Pre-expanding each PRN from `i8` to `f64` was rejected because CTTC serial and parallel medians regressed by 6.5% and 6.3%; changing carrier normalization frequency, removing normalization, chunking at normalization boundaries, replacing the sample-index check with a countdown, clamping indices to remove bounds checks, and four-sample manual unrolling were also measured and rejected. A pure-Rust two-channel sample-major batch reduced Rayon parallelism and regressed the parallel CTTC median by 6.7%; wrapping each individual complex multiply in two-lane AArch64 NEON regressed the isolated kernel by about 62%. A lane-native two-channel NEON design also failed: despite exact synthetic equality, its 7.437 s parallel CTTC median was 40.3% slower than the immediate restored 5.301 s scalar median because PRN lane construction and reduced Rayon task parallelism dominated. Vectorizing two consecutive samples while retaining per-channel Rayon tasks came closest, but its 4.656 s isolated median was still 3.5% slower than the immediate restored 4.497 s scalar-FMA median. Keep the portable scalar/FMA kernel unless a future backend can amortize code-index and sign-mask preparation over wider batches.
7. A production-shaped ignored benchmark now covers four-PRN batched PCPS on the real 10.23 Msps LabSat window. Explicit FMA in Doppler wipe-off, carrier recurrence, and spectrum multiplication was measured and rejected at 135.922 ms/scan versus the existing 134.951 ms/scan sustained median. Use this benchmark before introducing an optional Accelerate, GPU, or Metal FFT backend.
8. Workspace-wide ThinLTO with one code-generation unit was measured and rejected: its 5.717 s sustained tracking median was indistinguishable from the thermally matched 5.758 s ordinary-release result, while clean compilation increased from about 7.5 s to 25 s. Keep Cargo's ordinary portable release profile.

## Longer-Term Capabilities

These are intentionally after the priority roadmap:

- Last-known-Doppler narrowing for reacquisition is implemented as an opt-in scan-plan capability but is not the production default. LabSat full-file trials at `+/-1 kHz` and `+/-2.5 kHz` reduced converged PVT outputs from 82 to 66; the default remains the complete zero-centered grid.
- Persistent acquisition worker/cache.
- Live SDR input, real-time deadlines, and dropped-sample recovery.
- Multi-constellation signals and navigation beyond implemented GPS records.
- Automatic absolute-week anchoring for raw IQ-only captures without RINEX or a configured reference week.

## Deferred GUI Configuration

- Deeper Kalman/Hatch parameters only after they become behaviorally active receiver configuration.
- Additional visual regression scenarios when new panels are introduced.

## Current Next Action

Incomplete-final-epoch coverage and complete CTTC serial/parallel equality are complete. Continue Priority 2 by obtaining a real dynamic/high-rate IQ capture with independent trajectory/time truth; the current CTTC and LabSat datasets are both static and cannot validate rapid Doppler, velocity recovery, or a satellite that first becomes visible mid-run.
