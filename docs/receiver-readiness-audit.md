# Receiver Readiness and Runtime Audit

Date: 2026-07-20

## Verified Baseline

- The production `StreamingPvtRunner` processes the 60 s CTTC capture in 14.39 s in a release build with parallel tracking, versus 37.98 s with serial tracking.
- The parallel run acquires five PRNs, emits 299,955 tracking updates, decodes 32 navigation subframes, and produces 410 PVT outputs.
- The final time is GPS week 1734, SOW 368663.029, or 2013-04-04 06:24:07.029 UTC.
- GUI event/persistence tests pass; the latest workspace run reported 188 passed tests with 1 ignored test. The complete CTTC GUI run persisted 810 epochs and 113 discrete acquisition, channel-state, and navigation events.

This is a single-machine CTTC baseline, not a cross-platform benchmark. No CPU profiler or current peak-memory capture has been recorded, so the optimization list below is ranked from code inspection and must be measured before implementation.

## Not Implemented

### Remaining GUI integration work

- Field-level validation now blocks invalid drafts and reports the exact offending field for input sampling, Nyquist IF, chunk/PVT cadence, acquisition scheduling and Doppler grid, tracking loops, C/N0 estimation, channel admission, fixed PRNs, and the initial LLA prior. Expert controls for navigation policy and Kalman/Hatch internals remain future UI work.
- Historical search/filter/delete/reveal actions and queued-run reorder/cancel controls are implemented; queue UUID order is persisted in preferences across restarts.
- Plot focus, sky-marker hover details, and shared sky/C/N0 PRN linking/pinning are implemented. Deterministic screenshot coverage now launches `1440 x 900`, `1100 x 800` with the settings drawer open, and `800 x 720` layouts through `scripts/capture-gui-layouts.ps1`; window-handle capture avoids occlusion from unrelated desktop windows.
- Candidate comparison UI is still pending; Raw, Smoothed, and Kalman candidates are now carried through `PvtEpochOutput`, the GUI view model, and backward-compatible epoch JSON persistence.

The current worker owns `StreamingPvtRunner`, supports cooperative cancellation, maps discrete acquisition/reacquisition/channel-state/navigation events into GUI events, persists them without forwarding every millisecond tracking update, and exposes the main runtime settings through draft-only stepper/toggle controls.

### Receiver hardening and later scope

- Last-known-Doppler narrowing for reacquisition; current reacquisition scans the complete configured Doppler grid.
- A persistent acquisition worker/cache; acquisition currently creates scoped OS threads and a new `PcpsAcquisition` per PRN scan.
- Live SDR input, real-time deadlines, and dropped-sample recovery.
- Multi-constellation signals and RINEX navigation beyond the implemented GPS records.
- An automatic absolute-week anchor for raw IQ-only captures whose navigation message contains only the 10-bit GPS week. Without RINEX or a configured reference week, UTC date remains intentionally unavailable.

## Implemented but Not Fully Tested

- Real CTTC IQ covers established tracking, forced `Lost`, a detected `Reacquisition` scan, and restored tracking through `TrackingProbation`/`Healthy`. Re-entry into PVT after deliberately losing enough simultaneous channels remains a separate stress case.
- Failed reacquisition repeated over multiple configured intervals on real data.
- Dynamic cold acquisition and reacquisition across all 32 GPS L1 C/A PRNs on real data. CTTC validates a fixed five-PRN set.
- Automated serial-versus-parallel equality for the complete real-data PVT stream. Tracking-step equality is tested, and manual 60 s runs produced matching event counts and final time.
- High-dynamics velocity, shortened PVT epochs, rapid Doppler changes, and loss/recovery behavior.
- Long captures crossing ephemeris record changes, GPS week boundaries, or leap-second transitions. Time conversion boundaries have unit coverage, not end-to-end IQ coverage.
- Real `Int8` IQ input and files ending with an incomplete 1 ms tracking epoch.
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
- GNSScope raw WLS: 750 converged samples, mean `77.120 m`, population sigma `11.622 m`.
- GNSScope 10-point moving average: 741 samples, mean `77.174 m`, population sigma `5.849 m`.

The apparently large GUI sigma was therefore mainly a raw-versus-averaged comparison. Under the same 10-point display average, the sigmas differ by `0.286 m`. A remaining mean offset of about `19.2 m` is real and must not be hidden by averaging, but the supplied GNSS-SDR trajectory cannot establish which solution is absolutely correct. A diagnostic run with GNSScope ionosphere and troposphere corrections disabled moved the 10-point mean farther away, to `83.570 m`; the implemented atmospheric corrections have the correct net direction and reduce this offset by about `6.2 m`. Surveyed antenna coordinates or an independent reference receiver are still required to quantify absolute vertical bias.

## Runtime Optimization Candidates

### Priority 1: profile and optimize tracking correlation

`DllPllTracking::correlate` now uses a complex carrier rotator recurrence with periodic normalization instead of per-sample sine/cosine. Floating-point code phase, modulo, and three code lookups remain the dominant repeated operations.

Candidate changes:

- Replace per-sample trigonometry with a complex carrier rotator recurrence and periodic normalization. Implemented.
- Advance code phase incrementally with conditional wrap instead of recomputing from the sample index and using floating-point modulo.
- Benchmark explicit SIMD or batched correlator kernels only after recurrence changes.

Risk: high. Carrier/code phase continuity and serial/parallel numerical equivalence require real-data regression thresholds.

### Priority 2: replace the per-sample acquisition ring update

`AcquisitionRingBuffer::push_samples` performs `VecDeque` capacity checks plus push/pop for every sample, even while all channels are healthy. At 4 Msps this is 240 million ring updates for a 60 s capture.

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

Candidate changes:

- Let each channel worker process several consecutive 1 ms slices from the read batch and return ordered step vectors.
- Alternatively use persistent channel workers with bounded commands.
- Preserve acquisition timing by splitting batches whenever the active channel set changes.

Risk: high. Per-channel temporal order, same-millisecond event order, bit synchronization, and PVT boundaries must remain deterministic.

### Priority 5: cache acquisition work

Parallel acquisition creates scoped OS threads and a fresh FFT planner/engine for every PRN scan. This matters most for wide cold scans and repeated reacquisition, not steady-state CTTC tracking.

Candidate changes:

- Use the existing Rayon pool for PRN acquisition.
- Cache FFT plans, PRN code spectra, and reusable work buffers by sample rate/window size.
- Use last-known Doppler to reduce reacquisition bins after correctness is covered by a forced-loss test.

Risk: medium. Cached state must be keyed by all acquisition parameters.

### Priority 6: remove smaller recurring allocations and clones

- Avoid rebuilding navigation/ephemeris PRN vectors every millisecond.
- Avoid cloning the ephemeris catalog and ionosphere model at every PVT epoch by splitting borrows or using immutable shared ownership.
- Reduce state snapshot cloning to the configured GUI display cadence.

Risk: low to medium. Expected gain is smaller than tracking and ring-buffer work.

## Recommended Measurement Order

1. Add a release benchmark that separately reports acquisition startup, steady-state tracking, navigation/PVT, event generation, and total elapsed time.
2. Capture CPU profiles and peak memory for serial and parallel 60 s CTTC runs.
3. Implement carrier/code phase recurrences behind an equivalence test.
4. Replace the acquisition ring with a bulk circular buffer.
5. Add configurable telemetry throttling before wiring the GUI worker.
6. Re-profile before attempting batched workers, SIMD, or numeric-type changes.

## GUI Gate

The first functional GUI vertical slice now includes the real streaming worker, chunk-boundary cancellation, SQLite run/PVT/channel/satellite-geometry persistence, PVT-epoch telemetry throttling, historical replay, a shared epoch cursor, and a real WLS-derived sky plot. A complete 100 s CTTC release run persists 810 distinct epochs; epoch 60 is the first with solved satellite geometry, all 750 outputs from epoch 60 onward converge continuously, and the final five-PRN sky plot restores after restarting without IQ environment variables. Older epoch rows written before satellite persistence remain readable but require reprocessing before their sky plots can contain geometry. The overall GUI must not be called contract-complete until runtime settings are fully editable, discrete event history is persisted and replayed, and a real forced-loss/reacquisition workflow is tested through the GUI worker.

The restart replay regression verifies that the CTTC run stores 810 distinct rows with receiver epoch indexes `0..809`. Persistence uses an independent insertion ordinal, and startup reconciles summary counts with actual rows to expose legacy overwrite damage instead of presenting a disabled one-row timeline as an 810-epoch history.
