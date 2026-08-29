# Receiver Readiness and Runtime Audit

Date: 2026-08-14

## Verified Baseline

- The production `StreamingPvtRunner` processes the complete 100 s CTTC capture in 4.000-4.145 s in a release build with parallel tracking, versus 5.181-5.221 s with serial tracking on the current Apple Silicon machine.
- The complete run acquires five PRNs, emits 499,955 tracking updates, decodes 67 navigation subframes, and produces 810 PVT outputs. All 750 outputs after convergence remain continuous and 99.87% use all five satellites.
- The final time is GPS week 1734, SOW 368703.029, or 2013-04-04 06:24:47.029 UTC.
- The current targeted baseline passes 146 `gnss-core` library tests and 60 normal `gnss-gui` tests. The complete CTTC GUI run persists 810 epochs through the bounded worker channel and background SQLite writer.
- The release-only CTTC PVT recovery regression loads colocated RINEX to isolate recovery behavior, then forces G01/G11/G17 to `Lost` after converged epoch 79. Three subsequent PVT outputs are non-converged while fewer than four channels are eligible; all three PRNs are then reacquired and resume tracking, and converged PVT returns at epoch 105 with four satellites after the normal 2 s pull-in.
- The new LabSat capture `Data202306202155(2).dat` is verified as 2,670,030,000 bytes of interleaved signed Int8 I/Q at 10.23 Msps, approximately 130.5 s. Its byte distribution contains exactly the expected eight signed 3-bit levels (`-4..3`), so it is not an Int16 stream or packed LS3W register stream. The initial failed long run was caused by an overly coarse 500 Hz Doppler grid plus factor-5 acquisition decimation, not the IQ reader. Production acquisition now retains the original 10.23 Msps for this capture and uses the configured 250 Hz grid. A 90 s release run with no RINEX source decodes 81 navigation subframes from seven GPS channels, produces 71 PVT epochs, and converges to `40.00148524 deg, 116.33016726 deg, 85.462 m`, with `0.0754 m/s` speed, `8.2886 m/s` receiver clock drift, and UTC `2023-06-20 13:55:45.094`; the INI reference is `40.00154 deg, 116.33028 deg, 95 m` and local time `21:55` (UTC+8). The regression asserts that no RINEX catalog is loaded and that decoded broadcast ephemerides are nonempty before accepting its PVT result.

### LabSat full-file height result

The GUI-equivalent default 32-PRN worker completed the entire 2.67 GB, approximately 130.5 s LabSat capture in 35.64-37.11 s over two runs after same-grid PRNs began sharing carrier wipe-off and the input forward FFT. The immediate pre-change bounded-pool run took 47.65 s, and the earlier per-scan-thread run took 90.38 s. All runs produced the same 82 converged PVT outputs. Their altitude mean was 106.838 m, population sigma 4.012 m, and range 103.826-135.961 m. The high end is the first converged transient. The final 20 outputs stabilized at a 105.832 m mean with 0.488 m sigma, or 10.832 m above the INI's 95 m value. The final solution was `40.00150038 deg, 116.33019944 deg, 104.421 m`, corresponding to 8.164 m horizontal difference, 9.421 m vertical difference, and 12.466 m three-dimensional difference from the INI PVT value. `/usr/bin/time -l` measured 158.2 MiB maximum RSS, a 134.3 MiB peak memory footprint, and no swap on the second run.

This confirms that the first-convergence `+40.961 m` height difference does not persist. A stable approximately `+10.8 m` offset relative to the metadata remains. The INI does not establish that its PVT coordinates are surveyed antenna truth or document the altitude datum, so this is a metadata-relative result rather than a validated absolute receiver error.

The capture is too short to decode a valid GPS ionosphere page 18, so the IQ-only run has no Klobuchar correction. A controlled rerun used the IGS daily broadcast navigation file `BRDC00IGS_R_20231710000_01D_MN.rnx` for 2023 day 171. The same full GUI worker again produced 82 converged outputs. Their altitude mean became 102.195 m with 3.922 m population sigma and a 99.278-131.645 m range; the final 20 outputs stabilized at 101.382 m with 0.519 m sigma, reducing the metadata-relative mean offset from `+10.832 m` to `+6.382 m`. The final solution was `40.00150219 deg, 116.33019642 deg, 99.869 m`, approximately 8.27 m horizontal, 4.869 m vertical, and 9.60 m three-dimensional difference from the INI value.

The candidate comparison rules out a filter-only cause. Over the final 20 epochs, the IQ-only Raw, Smoothed, and Kalman means were 111.896 m, 110.080 m, and 105.832 m; with the daily navigation file they were 106.331 m, 103.619 m, and 101.382 m. Kalman reduces the raw spread and bias rather than creating the offset. A 90 s fixed-seven-satellite diagnostic also showed that atmospheric and leave-one-satellite-out effects change direction by epoch, so disabling a physical correction or hard-coding a satellite blacklist would overfit this capture. `WlsConfig` now has default-on ionosphere and troposphere switches for controlled diagnostics, and the ignored LabSat test can emit model, observables-source, TGD, and leave-one-out comparisons with `GNSSCOPE_LABSAT_DIAGNOSTICS=1`. An external comparison is explicitly enabled with `GNSSCOPE_LABSAT_RINEX_NAV=<path>`.

Normal processing now defaults to navigation-message ephemerides and ignores RINEX environment and colocated-file sources. Users can still explicitly select a RINEX file, or opt into deterministic colocated discovery for uncompressed `.rnx`, `.nav`, or GPS RINEX 2 `.YYn` files. This keeps the measured comparison capability without allowing external navigation data to silently replace the receiver's decoded broadcast ephemerides.

### LabSat GUI no-output diagnosis

The GUI previously showed neither usable C/N0 history nor a converged PVT for the LabSat capture because newly acquired channels entered health admission after only the first 20 ms C/N0 window. Their PLLs were still pulling in, so the default five-bad-epoch limit removed them before bit synchronization and navigation decode. The former coherent-sum carrier-lock metric also interpreted GPS navigation-bit polarity reversals as loss of carrier lock.

The carrier-lock detector now uses the Costas-compatible doubled-phase statistic `abs(sum(prompt^2)) / sum(abs(prompt)^2)`, which is invariant to 180-degree data-bit reversals. Every initial or reacquired channel receives its own 2 s pull-in period before C/N0/PLL admission and bit synchronization begin. The default unified bad-epoch limit is 50 rather than 5: GNSS-SDR uses the same 25 dB-Hz C/N0 minimum but allows 50 code-lock failures and 5000 carrier-lock failures. GNSScope remains conservative because its current channel manager has one combined counter. The release LabSat regression with GUI-equivalent defaults now retains enough channels to decode navigation and produce converged PVT while emitting per-PRN C/N0 values.

The first GUI-equivalent rerun exposed a separate acquisition-scheduler starvation bug hidden by the core regression's fixed seven-PRN list. With the GUI default of all 32 PRNs and four PRNs per scan, every cold scan sorted all absent channels and truncated the list to G01-G04. Rejected channels returned to `Absent`, so later scans repeated the same lowest PRNs and never reached visible satellites such as G10, G12, G23, G24, G25, G28, and G32. Cold acquisition now advances a cursor through consecutive batches until the complete candidate round is covered, then applies the configured cold-scan interval. Reacquisition uses the same fair rotation across intervals. A release-only `gnss-gui` worker regression starts from `GuiProcessingSettings::default()`, imports the real `.dat/.ini`, leaves `fixed_prns=None`, and reaches a converged four-or-more-satellite PVT in 33.66 s on the current machine after the acquisition FFT work-buffer optimization.

After incomplete-final-epoch hardening, two full GUI-equivalent runs took 34.04 s and 34.35 s and retained the exact 82-output position, altitude, candidate, and satellite-residual statistics above. Running the precompiled test binary directly under `/usr/bin/time -l` measured 57.0 MiB maximum RSS, a 48.9 MiB peak memory footprint, and no swap. This process-only boundary excludes Cargo compilation and linking, so it is retained as the reproducible current measurement rather than claimed as a reduction from older Cargo-wrapped figures.

This is a single-machine CTTC baseline, not a cross-platform benchmark. Product sequencing is maintained in `docs/roadmap.md`.

### macOS CTTC runtime profile

The 2026-08-14 release profile used the complete 100 s CTTC capture and the public `StreamingPvtRunner` real-data regression. Before optimizing the correlator, serial tracking took 8.53-8.72 s and parallel tracking took 8.95-9.09 s. `/usr/bin/time -l` reported peak RSS of 10.2 MiB serial and 10.6 MiB parallel, so memory is not the current constraint.

Instruments Time Profiler attributed 92.2% of serial CPU samples to `DllPllTracking::process`. Per-sample floating-point `fmod` accounted for 45.2% of leaf samples including its dynamic-link stub. Replacing per-sample code-phase recomputation with an incremental recurrence removed `fmod` from the sampled hot path and reduced serial sampled CPU time from 8,583 ms to 6,398 ms. Using scalar multiplication for the `+1/-1` PRN values removed the remaining general complex-code multiplications.

After both changes, three-run release ranges are 5.40-5.45 s serial and 5.29-5.34 s parallel. Event counts, UTC output, 810 PVT epochs, 99.87% five-satellite availability, and continuous convergence are preserved. The recurrence changes only floating-point evaluation order inside a 1 ms correlation; the observed difference is sub-nanosecond in SOW and millimetre-scale in aggregate altitude statistics.

Run the repeatable timing benchmark from the repository root:

```bash
GNSSCOPE_BENCHMARK_RUNS=3 scripts/benchmark-cttc-macos.sh
```

For a Time Profiler trace, first resolve the release test executable with Cargo, then launch it from `crates/gnss-core` so the gitignored relative data path resolves:

```bash
xcrun xctrace record --template 'Time Profiler' \
  --env GNSSCOPE_STREAMING_TEST_MS=100000 \
  --env GNSSCOPE_PARALLEL_TRACKING=0 \
  --launch -- ../../target/release/deps/streaming_runner_real-<hash> \
  test_streaming_runner_real_data_smoke --nocapture
```

### Final incomplete tracking epoch

`StreamingPvtRunner` now advances tracking, navigation, and PVT scheduling only after a complete 1 ms tracking step. A final partial step is still ingested into the bounded acquisition ring so `samples_read` and the absolute final sample index remain exact, but it cannot advance `next_pvt_output_ms` or solve from stale channel observations. The unit regression uses a 4 kHz, 2.5 ms IQ file: all 10 samples are retained, two complete epochs advance the schedule, and the final two samples do not create a third tracking/PVT epoch.

### Complete serial/parallel PVT equality

The ignored release regression `test_streaming_runner_complete_real_pvt_stream_matches_serial_and_parallel` runs the complete 100 s CTTC file twice. Serial acquisition/tracking and the bounded parallel pipeline produce identical event counts and exact structural equality for every field of all 810 `PvtEpochOutput` values, including candidate solutions, velocity/time, quality, and satellite diagnostics. The two complete runs finished in 9.43 s combined during the verification run.

### Current LabSat CPU profile

A 15 s Time Profiler attachment to the full 32-PRN GUI-equivalent LabSat regression collected 53,938 running-thread samples. Leaf samples grouped by receiver responsibility were 46.29% in `DllPllTracking::process`, 32.48% in acquisition FFT/transpose kernels, 5.85% in other PCPS work, 9.48% in Rayon scheduling and synchronization, and 0.68% in IQ I/O. The next performance experiment therefore belongs in the tracking correlator, not the reader or smaller allocation paths. Pre-expanding each PRN table from `i8` to `f64` was rejected: the immediate CTTC serial median regressed from 5.762 s to 6.139 s and the parallel median from 4.494 s to 4.779 s, while outputs remained unchanged.

The ignored release test `tracking_benchmark::benchmark_tracking_correlator_on_real_iq` isolates the public `DllPllTracking::process` seam with one real CTTC millisecond and a configurable repetition count. For 500,000 epochs, applying the `+1/-1` PRN chips by flipping the finite correlator value's IEEE-754 sign bit reduced the three-run median from 4.764 s to 4.295 s, or 9.8%. Replacing the two complex carrier multiplications with explicit `mul_add` then reduced the immediate median from 4.317 s to 4.209 s, another 2.5%; LLVM IR confirms four `llvm.fma.f64` operations in the sample loop. The cumulative isolated improvement is 11.6%. The current 100 s CTTC medians are 4.438 s serial and 4.039 s parallel. All 810 exact serial/parallel PVT outputs and the full LabSat result of 82 converged outputs ending at `40.00150038 deg, 116.33019944 deg, 104.421 m` remain intact. Thermally loaded LabSat runs were too noisy for a runtime claim.

Several nearby experiments did not clear the benchmark threshold and were removed: normalizing the carrier recurrence every 1,024 rather than 256 samples changed the isolated median by only 0.36%; removing normalization regressed the full CTTC run; normalization-boundary chunking regressed the isolated run by 12.5%; and a decrementing normalization counter was 0.7% slower. LLVM IR showed three PRN-table bounds checks in the loop, but replacing them with statically bounded indices was 1% slower. Four-sample manual unrolling was 5.9% slower. A two-channel sample-major batch preserved exact output but reduced the number of Rayon tasks; its parallel CTTC median was 4.311 s versus 4.039 s before the experiment, a 6.7% regression, and restoring per-channel tasks returned the median to 4.008 s. An AArch64 NEON adapter that packed one complex value into two `f64` lanes took about 6.84 s versus the 4.209 s scalar-FMA isolated median, a roughly 62% regression from vector packing and load/store overhead. A subsequent lane-native design kept two independent channel states in the two NEON lanes and passed exact synthetic serial/parallel output equality, but the three-run parallel CTTC median was 7.437 s versus the immediate restored scalar median of 5.301 s, a 40.3% regression. Per-sample construction of three two-lane PRN sign masks, duplicated scalar code-index work, and halving the number of independent Rayon channel tasks outweighed the vector multiply-add savings. A final AArch64 experiment retained channel-level Rayon scheduling and vectorized two consecutive samples within each channel; its isolated median was 4.656 s versus an immediate restored scalar-FMA median of 4.497 s, a 3.5% regression. The remaining scalar code-index/sign-mask preparation and horizontal reduction still outweighed the saved carrier arithmetic, so the portable scalar/FMA kernel remains the production path.

The ignored release test `acquisition_benchmark::benchmark_batched_pcps_on_real_labsat_iq` now measures the production-shaped four-PRN, 81-bin, ten-integration batch on the first real LabSat IQ window. With 20 scans per measurement, the existing `num_complex` operations had a 134.951 ms/scan median under sustained load; replacing Doppler wipe-off, carrier recurrence, and spectrum multiplication with explicit scalar FMA produced 135.922 ms/scan, 0.7% slower, and was removed. This benchmark is the acceptance seam for any future optional FFT or Metal backend.

A workspace-wide release profile using ThinLTO and one code-generation unit was also rejected. Its isolated tracking median was 5.717 s for 500,000 epochs; after restoring the ordinary release profile, the same thermally loaded machine took 5.758 s, so no repeatable runtime improvement was established. The older 4.497 s ordinary-release median was collected under a cooler load state and is not used for this A/B. ThinLTO also increased the clean release build from about 7.5 s to 25 s, so production keeps Cargo's ordinary portable release profile.

## Current Product Status

### Implemented GUI foundation

- Every behaviorally active `GuiProcessingSettings` field is now selectable or directly editable in a draft through the category-focused receiver drawer, covering source sampling/IQ, navigation policy, initial LLA, acquisition/reacquisition, Doppler grid, tracking loops, C/N0, channel admission, fixed PRNs, PVT/filter toggles, RAIM, and parallelism. The merged command title bar keeps only the source summary and task actions. Field-level parsing and receiver validation both block invalid queue creation.
- Historical search/filter/delete/reveal actions and queued-run reorder/cancel controls are implemented; queue UUID order is persisted in preferences across restarts.
- Plot focus, sky-marker hover details, and shared sky/C/N0 PRN linking/pinning are implemented. Deterministic screenshot coverage includes `1440 x 900`, `1100 x 800`, and `800 x 720` workspaces, direct-edit receiver drawers, candidate comparison, and Chinese/dark plus English/light interface-settings pages; window-handle capture avoids occlusion from unrelated desktop windows.
- The selected-epoch Raw/Smoothed/Kalman comparison drawer is implemented with structured candidate disposition, receiver-selected highlighting, position/velocity/clock/quality deltas, immutable historical behavior, and epoch-timeline synchronization. Candidate status and values survive SQLite JSON round trips; legacy rows infer conservative status from their existing candidate payloads.
- The configuration redesign synchronizes `gpui-component` and custom plot themes, uses category-focused Input/Acquisition/Tracking/PVT/Performance pages with direct inputs and native switches, and adds a dedicated interface-settings page reached from a restrained lower-left sidebar item. Theme, language, and timeline visibility have no duplicate entry points.
- A prominent New task action now appears before the active, queued, and historical runs in the left sidebar. It clears the current draft/result selection and opens the IQ picker. Selecting the LabSat `.dat` automatically reads its sibling `.ini`, applying 10.23 Msps, Int8 interleaved I/Q, zero IF, and the supplied coarse LLA without adding a dataset-specific preset button.
The current worker owns `StreamingPvtRunner`, supports cooperative cancellation, maps discrete acquisition/reacquisition/channel-state/navigation events into GUI events, and persists them without forwarding every millisecond tracking update. Editable text is retained independently from the last valid typed value, so incomplete or malformed input remains visible with an inline error instead of being silently clamped. Queued and completed run snapshots remain immutable; selecting one seeds a separate directly editable next-run draft, and subsequent reprocessing uses those edits.

### Implemented candidate comparison

Raw, Smoothed, and Kalman candidates are carried through `PvtEpochOutput`, the GUI view model, backward-compatible epoch JSON persistence, and a read-only expert comparison drawer for the currently selected epoch:

- an icon action in the PVT result header opens a right-side comparison drawer;
- the comparison and advanced-settings drawers are mutually exclusive, and the workspace reflows instead of being covered;
- moving the persistent epoch timeline updates all candidate values in the drawer;
- the default workspace continues to render only the receiver-selected solution;
- the drawer compares position, altitude, velocity, clock bias/drift, solution quality, satellite use, and deltas relative to the selected solution;
- unavailable or non-converged candidates are shown explicitly rather than replaced with zero values;
- candidate selection remains receiver-owned and cannot be rewritten from historical UI.

The candidate contract now distinguishes selected, available-but-lower-priority, non-converged, quality-gated, and unavailable candidates without asking the GUI to infer new-run rejection reasons. Rejected Smoothed candidates retain their diagnostic values without becoming eligible for selection or Kalman input.

### Receiver hardening and later scope

- Last-known-Doppler narrowing for reacquisition is available in scan plans but remains opt-in. Full-file LabSat trials with `+/-1 kHz` and `+/-2.5 kHz` windows produced only 66 converged PVT outputs versus the 82-output zero-centered full-grid baseline, so production keeps the complete grid.
- A persistent acquisition engine/cache; acquisition now reuses a bounded Rayon pool but still creates a new `PcpsAcquisition` per PRN scan.
- Live SDR input, real-time deadlines, and dropped-sample recovery.
- Multi-constellation signals and RINEX navigation beyond the implemented GPS records.
- An automatic absolute-week anchor for raw IQ-only captures whose navigation message contains only the 10-bit GPS week. Without RINEX or a configured reference week, UTC date remains intentionally unavailable.

## Implemented but Not Fully Tested

The repository's CTTC and LabSat captures are both static: the CTTC reference NMEA reports `0.00` speed and the LabSat metadata supplies one fixed position. They cannot substantiate the dynamic scenarios below; a real capture with independent trajectory/time truth is required before those items can move to verified.

- Failed reacquisition repeated over multiple configured intervals on real data.
- Dynamic cold acquisition and reacquisition across all 32 GPS L1 C/A PRNs on real data. CTTC validates a fixed five-PRN set.
- High-dynamics velocity, shortened PVT epochs, rapid Doppler changes, and loss/recovery behavior.
- Long captures crossing ephemeris record changes, GPS week boundaries, or leap-second transitions. Time conversion boundaries have unit coverage, not end-to-end IQ coverage.
- Peak working set/private bytes for the current bounded streaming implementation.
- Position, velocity, clock drift, and time accuracy across front ends and datasets other than CTTC.

## Position Outlier Diagnosis

The 100 s CTTC GUI history exposed 201 mathematically converged four-satellite WLS epochs with physically impossible altitude, including epochs around `1.12-2.10 Mm`. Four satellites are sufficient for the four unknowns `x/y/z/clock bias`, but provide no residual redundancy: an incorrect four-satellite pseudorange combination can therefore have an almost-zero post-fit residual without being physically correct.

The receiver did have five healthy tracking channels. Each PVT epoch is formed at one common receiver sample from the latest PVT-eligible observation of every channel; it is not formed only from whichever channel happened to finish its 1 ms tracking update last. The failure occurred after epoch formation: inconsistent integer-millisecond code ambiguities made the initial five-satellite residual large, then RAIM sometimes excluded the wrong satellite and left an exactly determined bad four-satellite set.

The production fix enables integer-millisecond ambiguity resolution for the CTTC preset and uses the last accepted position as the next WLS/ambiguity-resolution numerical prior. The CTTC preset supplies a coarse initial position only for cold-start ambiguity resolution, then replaces it with the first accepted solution. RAIM defaults to disabled and is available as an advanced GUI option. This preserves per-epoch observations while selecting the physically continuous ambiguity branch without residual-driven satellite exclusion. The terrestrial altitude and inter-epoch motion gates remain final safety nets before velocity and filtering. In the 100 s release regression, 750 outputs converge, 99.87% of them use all five satellites, and no converged output violates the altitude gate. The GUI also applies the gate when reading legacy SQLite rows so old outliers no longer distort ENU or altitude history.

## PVT Continuity at HOW Boundaries

The former real-data stream produced 24 unconverged outputs after the first solution: two consecutive 100 ms epochs every six seconds, while all five channels remained healthy. `NavDecoder` stopped advancing its per-symbol TOW during the eight-symbol preamble search after each decoded subframe. All five transmit timestamps therefore collapsed onto the same HOW integer second for 160 ms, leaving WLS with no inter-satellite pseudorange geometry. TOW now advances for every fed 20 ms symbol once established, including unsynchronized preamble-search symbols; successful HOW decode then reanchors it exactly. The obsolete default 160 ms sample-tag compensation is now zero but remains configurable for front ends with real external latency. The 100 s regression asserts that no unconverged PVT appears after the first solution.

## CTTC Altitude Comparison

The supplied GNSS-SDR NMEA/KML files are reference receiver solutions, not surveyed ground truth. GNSS-SDR writes WGS84 ellipsoidal height to GGA for this artifact: its geoid separation is fixed to `0.0 m`, so its height unit and datum match GNSScope. The dataset configuration uses a 100 ms output period, `averaging_depth=10`, and arithmetic moving averaging.

On the same 100 s capture:

- GNSS-SDR NMEA/KML: 739 samples, mean `57.965 m`, population sigma `5.563 m`, range `42.8-71.3 m`.
- GNSScope raw WLS: 750 converged samples, mean `77.106 m`, population sigma `11.574 m`.
- GNSScope 10-point moving average: 741 samples, mean `77.161 m`, population sigma `5.829 m`.

The apparently large GUI sigma was therefore mainly a raw-versus-averaged comparison. Under the same 10-point display average, the sigmas differ by `0.266 m`. A remaining mean offset of about `19.2 m` is real and must not be hidden by averaging, but the supplied GNSS-SDR trajectory cannot establish which solution is absolutely correct. A diagnostic run with GNSScope ionosphere and troposphere corrections disabled moved the 10-point mean farther away, to `83.570 m`; the implemented atmospheric corrections have the correct net direction and reduce this offset by about `6.4 m`. Surveyed antenna coordinates or an independent reference receiver are still required to quantify absolute vertical bias.

## Runtime Optimization Candidates

### Priority 1: profile and optimize tracking correlation

`DllPllTracking::correlate` now uses complex carrier and code-phase recurrences instead of per-sample trigonometry and floating-point modulo. GPS L1 C/A PRN signs are applied with an exact sign-bit flip instead of scalar multiplication, and complex carrier multiplication uses fused multiply-add. Tracking correlation remains the dominant repeated operation after these changes.

Candidate changes:

- Replace per-sample trigonometry with a complex carrier rotator recurrence and periodic normalization. Implemented.
- Advance code phase incrementally with conditional wrap instead of recomputing from the sample index and using floating-point modulo. Implemented and profiled.
- Apply `+1/-1` PRN chips without floating-point multiplication. Implemented and profiled with the isolated real-IQ benchmark.
- Fuse the carrier wipe-off and recurrence multiply-add operations. Implemented and verified in optimized LLVM IR.
- Benchmark explicit SIMD or batched correlator kernels against the isolated real-IQ baseline.

Risk: high. Carrier/code phase continuity and serial/parallel numerical equivalence require real-data regression thresholds.

### Priority 2: replace the per-sample acquisition ring update

`AcquisitionRingBuffer::push_samples` already performs chunk-level `VecDeque::drain` and `extend` operations rather than explicit per-sample push/pop calls. A fixed preallocated `Vec` circular-buffer experiment using at most two `copy_from_slice` calls per input chunk was rejected: its CTTC serial median was 5.687 s versus 5.358 s after restoring the current implementation, and its parallel median was 5.516 s versus 5.391 s. The existing buffer remains in place, with added wrap, oversized-input, zero-capacity, and absolute-index coverage.

Candidate changes:

- Use a fixed contiguous circular buffer with bulk copies.
- Keep the ring continuously warm for reacquisition, but avoid per-sample container operations.
- Return one or two borrowed slices for a wrapped acquisition window, copying only when the PCPS API requires contiguous input.

Risk: medium. Absolute sample indices and acquisition-window ordering must remain exact.

### Priority 3: decouple processing cadence from event cadence

The runner now emits one PRN-sorted `TrackingBatch` per millisecond and moves each `TrackingOutput` into that batch. Event summaries still count the contained channel updates. GUI telemetry is already reduced to the configured PVT epoch; further multi-millisecond tracking batches remain a higher-risk optimization.

Candidate changes:

- Continue health, navigation, and Doppler-history updates every millisecond.
- Emit and persist GUI telemetry at the configurable PVT output epoch.
- Keep loss, acquisition, navigation decode, and PVT events immediate.
- Let summaries count internal tracking epochs without retaining one public event per epoch.

Risk: medium. Tests and diagnostics that depend on millisecond events need an explicit raw-event mode.

### Priority 4: reduce parallel dispatch overhead

Tracking invokes a Rayon parallel iterator once per millisecond. With five channels this performs 60,000 dispatch/merge cycles for a 60 s file.

The runner now owns one bounded Rayon pool shared by parallel tracking and acquisition instead of relying on the process-global pool for tracking and creating scoped OS threads for each acquisition batch. The automatic size is bounded by available cores and `max(4, max_acq_prns_per_scan)`; `StreamingPvtConfig::parallel_worker_threads` can override it. On the 10-core LabSat machine, first-PVT runs took about 10.1 s with the global 10-thread pool, 9.99 s with six threads, 9.94 s with four threads, and 13.49 s with three threads. Four threads reduced scheduler system time from about 16 s to 2.84 s without changing the PVT result. Cross-millisecond batching is still deferred because acquisition, loss/admission, navigation decode, and PVT boundaries must remain serially ordered.

A post-PCPS Time Profiler trace of the 100 s parallel CTTC run contained 17,606 running-thread samples. `DllPllTracking::process` was the dominant compute leaf at 38.1%; explicit Rayon wait, wake, and steal leaves accounted for about 7%, while `swtch_pri` represented another 47.8% of sampled worker switching/waiting. Rayon implements `BTreeMap::par_iter_mut` by first materializing a vector of mutable references, so the runner now stores the at-most-32 active channels in a PRN-sorted vector and parallelizes its slice directly. This also makes result order structural and removes the per-millisecond sort. Navigation-time and ephemeris availability now use fixed 33-slot stack tables instead of collecting two heap vectors per millisecond.

Before these allocation changes, three default-parallel CTTC runs were 3.978-4.468 s with a 4.223 s median. Afterward, five runs were 4.000-4.145 s with a 4.027 s median, a 4.6% median reduction with much lower spread. Five serial runs were 5.181-5.221 s and did not improve on the immediate 5.093-5.137 s serial baseline. The full LabSat run took 36.28 s, inside the prior 35.64-37.11 s range, so the retained change is a small steady-state parallel improvement rather than another full-chain step change. CTTC still produced the same 810 PVT outputs and LabSat retained the exact 82-output final solution and altitude statistics.

Candidate changes:

- Let each channel worker process several consecutive 1 ms slices from the read batch and return ordered step vectors.
- Alternatively use persistent channel workers with bounded commands.
- Preserve acquisition timing by splitting batches whenever the active channel set changes.

Risk: high. Per-channel temporal order, same-millisecond event order, bit synchronization, and PVT boundaries must remain deterministic.

### Priority 5: cache acquisition work

Parallel acquisition uses the runner-owned bounded Rayon pool and creates a fresh FFT planner/engine for every PRN scan. This matters most for wide cold scans and repeated reacquisition, not steady-state CTTC tracking.

Each `PcpsAcquisition` now plans its forward and inverse FFTs once, reuses one in-place complex FFT buffer across all Doppler bins and non-coherent integrations, and advances carrier wipe-off with a normalized complex recurrence. The 32-PRN CTTC acquisition regression improved from about 0.21 s to 0.16 s, while the 32-PRN, 10-integration LabSat regression improved from about 0.39 s to 0.28 s. The full GUI-equivalent LabSat run improved from 38.61 s to 33.66 s with unchanged detections and PVT output.

Same-grid multi-PRN scans now compute each Doppler-bin carrier wipe-off and input forward FFT once, then correlate that spectrum with each requested PRN. Doppler bins are the parallel work units, so a four-PRN scan uses one Rayon collection boundary rather than synchronizing once per Doppler/integration pair. Reacquisition requests with different Doppler centers or widths are grouped separately, and results are merged in PRN order. The full GUI-equivalent LabSat regression improved from the immediate 47.65 s bounded-pool baseline to 35.64-37.11 s while preserving all 82 converged PVT outputs and the exact final position and altitude statistics. The default four-PRN, 81-bin, 10.23 Msps scan retains about 25.3 MiB of temporary magnitude grids during the scan; the measured full-process maximum RSS is 158.2 MiB with no swap.

Candidate changes:

- Use the runner-owned bounded Rayon pool for PRN acquisition. Implemented.
- Cache FFT plans and reuse per-engine work buffers. Implemented.
- Share carrier wipe-off and input forward FFT across PRNs on the same Doppler grid. Implemented.
- Cache PRN code spectra across acquisition scans by sample rate/window size. Measured and rejected: the 10.34 s cached first-PVT run did not improve the 10.12 s uncached result.
- Use last-known Doppler to reduce reacquisition bins only after a stronger Doppler-validity model is available; the current LabSat trials changed PVT continuity.

Risk: medium. Cached state must be keyed by all acquisition parameters.

### Priority 6: remove smaller recurring allocations and clones

- Avoid rebuilding navigation/ephemeris PRN vectors every millisecond. Implemented with fixed GPS L1 C/A availability tables.
- Avoid the `BTreeMap::par_iter_mut` reference-vector allocation and post-collection sort. Implemented with a PRN-sorted active-channel slice.
- Avoid cloning the ephemeris catalog and ionosphere model at every PVT epoch by splitting borrows or using immutable shared ownership.
- Reduce state snapshot cloning to the configured GUI display cadence.

Risk: low to medium. Expected gain is smaller than tracking and ring-buffer work.

## Recommended Next Work

See `docs/roadmap.md` for the authoritative priority order. The active priorities are a structural App UI refactor followed by multi-system and multi-frequency signal processing. The accuracy and scenario-coverage items documented in this audit remain valid receiver gaps, but their expansion is deferred.

## GUI Gate

The first functional GUI vertical slice now includes the real streaming worker, chunk-boundary cancellation, complete draft configuration editing, SQLite run/PVT/channel/satellite-geometry persistence, PVT-epoch telemetry throttling, historical replay, a shared epoch cursor, and a real WLS-derived sky plot. A complete 100 s CTTC release run persists 810 distinct epochs; epoch 60 is the first with solved satellite geometry, all 750 outputs from epoch 60 onward converge continuously, and the final five-PRN sky plot restores after restarting without IQ environment variables. Older epoch rows written before satellite persistence remain readable but require reprocessing before their sky plots can contain geometry.

The restart replay regression verifies that the CTTC run stores 810 distinct rows with receiver epoch indexes `0..809`. Persistence uses an independent insertion ordinal, and startup reconciles summary counts with actual rows to expose legacy overwrite damage instead of presenting a disabled one-row timeline as an 810-epoch history.
