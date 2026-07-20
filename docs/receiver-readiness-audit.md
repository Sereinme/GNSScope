# Receiver Readiness and Runtime Audit

Date: 2026-07-20

## Verified Baseline

- The production `StreamingPvtRunner` processes the 60 s CTTC capture in 14.39 s in a release build with parallel tracking, versus 37.98 s with serial tracking.
- The parallel run acquires five PRNs, emits 299,955 tracking updates, decodes 32 navigation subframes, and produces 410 PVT outputs.
- The final time is GPS week 1734, SOW 368663.029, or 2013-04-04 06:24:07.029 UTC.
- `cargo test --workspace` passes 177 tests with 1 ignored test.

This is a single-machine CTTC baseline, not a cross-platform benchmark. No CPU profiler or current peak-memory capture has been recorded, so the optimization list below is ranked from code inspection and must be measured before implementation.

## Not Implemented

### Required for functional GUI integration

- A receiver worker that owns `StreamingPvtRunner` and maps `StreamingPvtReadStep` and `StreamingPvtEvent` into `GuiReceiverEvent`.
- Cancellation and worker shutdown. Pause/resume semantics are also undefined.
- GUI settings for `chunk_ms`, `pvt_output_rate_ms`, Doppler epoch averaging, parallel acquisition, and parallel tracking.
- GUI-visible reacquisition state, counters, and recovery history.
- Event backpressure or display-rate throttling between the processing worker and GUI thread.

### Receiver hardening and later scope

- Last-known-Doppler narrowing for reacquisition; current reacquisition scans the complete configured Doppler grid.
- A persistent acquisition worker/cache; acquisition currently creates scoped OS threads and a new `PcpsAcquisition` per PRN scan.
- Live SDR input, real-time deadlines, and dropped-sample recovery.
- Multi-constellation signals and RINEX navigation beyond the implemented GPS records.
- An automatic absolute-week anchor for raw IQ-only captures whose navigation message contains only the 10-bit GPS week. Without RINEX or a configured reference week, UTC date remains intentionally unavailable.

## Implemented but Not Fully Tested

- Forced loss followed by successful reacquisition on real IQ data, including return through `TrackingProbation` to `Healthy` and re-entry into PVT.
- Failed reacquisition repeated over multiple configured intervals on real data.
- Dynamic cold acquisition and reacquisition across all 32 GPS L1 C/A PRNs on real data. CTTC validates a fixed five-PRN set.
- Automated serial-versus-parallel equality for the complete real-data PVT stream. Tracking-step equality is tested, and manual 60 s runs produced matching event counts and final time.
- High-dynamics velocity, shortened PVT epochs, rapid Doppler changes, and loss/recovery behavior.
- Long captures crossing ephemeris record changes, GPS week boundaries, or leap-second transitions. Time conversion boundaries have unit coverage, not end-to-end IQ coverage.
- Real `Int8` IQ input and files ending with an incomplete 1 ms tracking epoch.
- Peak working set/private bytes for the current bounded streaming implementation.
- Position, velocity, clock drift, and time accuracy across front ends and datasets other than CTTC.

## Runtime Optimization Candidates

### Priority 1: profile and optimize tracking correlation

`DllPllTracking::correlate` evaluates carrier sine/cosine, floating-point code phase, modulo, and three code lookups for every input sample and every active channel. Tracking is the dominant repeated computation by operation count.

Candidate changes:

- Replace per-sample trigonometry with a complex carrier rotator recurrence and periodic normalization.
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

The runner currently creates a `TrackingUpdated` event and clones `TrackingOutput` for every channel every millisecond. The 60 s CTTC run creates 299,955 tracking updates, while a GUI generally needs 10-20 updates per second.

Candidate changes:

- Continue health, navigation, and Doppler-history updates every millisecond.
- Emit GUI telemetry at a configurable display interval.
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

Visual GUI design can start. A functional GUI should not be called complete until the worker adapter, cancellation, runtime settings, event throttling, and a real forced-loss/reacquisition test are implemented.
