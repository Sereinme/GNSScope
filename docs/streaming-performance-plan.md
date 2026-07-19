# Streaming and Dynamic Receiver Plan

This plan captures the agreed optimization design before changing the receiver path. It is intentionally scoped to backend receiver architecture. GUI visual design remains paused.

## Goals

- Reduce CTTC real-data processing memory from the current near-4 GB behavior to below 700 MB.
- Preserve PVT results exactly during read-path and deterministic parallelization changes.
- Make PVT output continuous and configurable-rate, with Kalman as the preferred final output.
- Prepare the receiver architecture for dynamic acquisition, reacquisition, and future GUI worker integration.
- Add GNSS-SDR NMEA comparison baselines for position, speed, and time.

## Non-Goals

- Do not introduce async runtime or async I/O before GUI implementation.
- Do not change numeric type from `Complex<f64>` in the first streaming phase.
- Do not fix Doppler velocity/time alignment in the same step as streaming and parallelization.
- Do not turn `pvt_e2e.rs` into the permanent receiver implementation.

## Current Problems

- `pvt_e2e` reads 60 s at 4 MSPS into `Vec<Complex<f64>>`.
  - 240,000,000 complex samples times 16 bytes is about 3.84 GB before other buffers.
- Acquisition and tracking are currently sequential at startup; per-epoch tracking is deterministic parallel.
- The test now uses 10 ms non-coherent PCPS acquisition Doppler instead of hard-coded GNSS-SDR reference Dopplers and the slow 2 s fine Doppler search.
- Continuous PVT currently builds epochs at 1 Hz, while the GNSS-SDR CTTC config uses `PVT.output_rate_ms=100`.
- Position is already close to GNSS-SDR/NMEA reference, but velocity and time are not yet strict GNSS-SDR comparisons.

## Phases

### Phase A: Streaming E2E, Single Thread

- Replace whole-file IQ loading with streaming reads.
- Default chunk size is 1 ms.
- Support `chunk_ms` configuration values of 1, 10, and 20 ms later, but chunks above 1 ms are only batch-read buffers. Tracking still receives 1 ms slices.
- Preserve current fixed CTTC PRNs and legacy fine Doppler behavior for exact regression before Phase C.
- Keep `Complex<f64>`.
- Keep current tracking history behavior for result equivalence.
- Add local memory measurement script or instructions for Windows peak Working Set/Private Bytes.

Acceptance:

- PVT outputs match the pre-streaming path within tight floating-point tolerances.
- Peak memory below 700 MB.
- Existing real-data e2e still passes.

### Phase B: Deterministic Tracking Parallelism

- Parallelize active tracking channels within the same 1 ms epoch.
- Preserve per-channel temporal order.
- Merge all per-channel outputs in fixed PRN order before nav decode, observables, WLS, logging, and Kalman.
- Keep Kalman and continuous PVT epoch processing sequential by epoch.
- Do not let thread completion order affect PVT inputs or logs.

Acceptance:

- Same PVT results as Phase A within tight tolerances.
- Stable deterministic logs and PRN order.

### Phase C: Replace Legacy Fine Doppler

- Status: PCPS non-coherent integration API is implemented and covered by synthetic and CTTC real-data acquisition tests. The e2e path initializes tracking from 10 ms non-coherent PCPS Doppler and no longer uses hard-coded reference Dopplers or the 2 s fine Doppler window.
- Remove hard-coded `REF_DOPPLERS`. Done in the e2e path.
- Remove 2 s fine Doppler search and its temporary cache. Done in the e2e path.
- Extend PCPS acquisition to support non-coherent integration:
  - Use 1 ms coherent PCPS per integration.
  - Accumulate the Doppler/code-phase power grid over `N` integrations, for example 10 ms.
  - Select peak from the accumulated grid.
- Initialize tracking directly from acquisition output:
  - `AcquisitionResult::doppler_hz`
  - `AcquisitionResult::code_phase_chips`
  - `AcquisitionResult::sample_index`
- If later refinement is needed, it must be centered on acquisition Doppler, not GNSS-SDR reference Doppler.

Important constraint:

- The legacy 2 s fine Doppler window is temporary and must be deleted in this phase. Do not merge it into the permanent acquisition ring buffer.

Acceptance:

- The same CTTC PRNs can be acquired and tracked without hard-coded Doppler truth.
- PVT remains within a GNSS-SDR/NMEA comparison threshold, even if exact values change due to removal of reference Doppler assistance.

### Phase D: Speed and Time GNSS-SDR Alignment

- Parse GNSS-SDR NMEA:
  - `$GPRMC`: UTC, speed over ground, course, date.
  - `$GPGGA`: UTC, lat/lon/alt, fix quality, satellite count, HDOP.
- Print and test GNSScope versus GNSS-SDR differences:
  - position delta
  - speed delta
  - receiver/corrected GPS time versus NMEA UTC
- Reference GNSS-SDR behavior for:
  - observables timestamp
  - receiver/transmit time handling
  - Doppler sign convention
  - satellite velocity
  - receiver clock drift
  - UTC model and leap seconds

Expected static CTTC behavior:

- GNSS-SDR RMC speed is 0.00 knots.
- GNSScope velocity should converge toward a near-zero static solution after Doppler/time alignment.

### Phase E: Minimal StreamingPvtRunner

- Status: initial backend runner contract is implemented in `gnss_core::receiver`. `StreamingPvtConfig` covers IQ/nav policy, chunk size, output rate, acquisition, channel admission, tracking, and continuous PVT config. `StreamingPvtRunner` owns deterministic channel state, periodic acquisition scan planning, the acquisition ring buffer, structured PVT output events, can stream an IQ file through configured chunks, executes PCPS acquisition scans from the ring buffer, initializes tracking directly from acquisition results, advances active tracking channels in deterministic PRN order, maintains bit sync plus 20 ms nav-symbol accumulation/decoding state per channel, stores navigation state including ephemerides, ionosphere, subframe counts, and per-PRN TOW history, forms PVT-eligible `ObservablesInputSet` snapshots from healthy tracking channels, builds `PseudorangeEpoch` values, and now drives `ContinuousPvtRunner` to emit `PvtOutput` at the configured output cadence. The next step is migrating the real-data e2e main loop to consume this runner-owned PVT path directly.
- Extract the stable e2e flow into core receiver modules:
  - `StreamingPvtConfig` - initial contract implemented.
  - `StreamingPvtRunner` - initial contract implemented.
  - `StreamingPvtResult`
  - `StreamingChannelState`
- Keep the first runner small:
  - open IQ - initial configured file streaming implemented.
  - load optional nav - RINEX nav explicit/env/colocated discovery is wired into runner startup; missing nav keeps IQ-only mode.
  - stream samples - implemented for chunked IQ input into runner; reader now handles short reads, advances acquisition, 1 ms tracking, and PVT cadence internally.
  - acquire configured PRNs - PCPS scan execution implemented with deterministic PRN-ordered parallel acquisition.
  - track - active tracking initialization and deterministic per-ms stepping implemented.
  - decode nav - bit sync, 20 ms symbol emission, NavDecoder, and EphemerisCollector entry points implemented.
  - build observables - implemented for healthy channel snapshots with TOW history and tracking timestamp inputs.
  - solve continuous PVT - runner entry point implemented.
  - emit structured results - configured-rate `PvtOutput` event entry point implemented; read steps and run summary include state snapshots, latest PVT output, and structured acquisition/tracking/nav/PVT event counts.
- Do not include GUI worker, async runtime, multi-constellation, or full UI schema yet.

Real-data smoke coverage:

- `streaming_runner_real` verifies the runner-owned path loads colocated CTTC RINEX nav, acquires the fixed CTTC PRNs, and advances tracking directly from the IQ reader.
- `StreamingPvtConfig::cttc_spain_real_data` captures the CTTC dataset defaults used by the smoke path, so future e2e migration and GUI presets share one configuration source.

### Phase F: Dynamic Acquisition and Reacquisition

- Status: backend primitives are implemented in `gnss_core::receiver`: deterministic channel state/admission, periodic acquisition scheduling, reacquisition priority, and a bounded acquisition ring buffer. The next step is wiring these primitives into the streaming runner/e2e processing loop.
- Introduce:
  - `AcquisitionScheduler` - implemented.
  - `ChannelManager` - implemented.
  - `ReceiverChannelState` - implemented.
  - `AcquisitionRingBuffer` - implemented.
- Use periodic acquisition scans, not continuous background scanning.
- Prioritize reacquisition before cold acquisition.
- Use a short acquisition ring buffer for cold acquisition and reacquisition windows.
- Keep the acquisition ring buffer separate from the temporary legacy fine Doppler window.

Dynamic channel state:

```text
Absent
  -> Acquiring
  -> TrackingProbation
  -> Healthy
  -> Suspect
  -> Lost
  -> Reacquiring
```

Newly acquired and reacquired signals follow the same clean initialization path:

```text
AcquisitionResult
  -> DllPllTracking::new(prn, doppler_hz, code_phase_chips, config, sample_index)
  -> TrackingProbation
```

Reacquisition can use last-known Doppler to narrow the search, but the old lost PLL/DLL state should not be reused in the first implementation.

### Phase G: GUI Worker and Async Later

- GUI should consume structured receiver events and snapshots.
- Async, prefetch queues, or bounded I/O channels should be considered after GUI worker architecture is defined.
- Do not add async to the current CLI/e2e receiver path.

## Continuous PVT Policy

Dynamic scenarios require one WLS per PVT epoch:

```text
epoch observables -> WLS position/clock -> Doppler velocity -> Kalman -> final output
```

Single-shot WLS is retained only as a debug and regression snapshot.

Final output preference:

```text
if Kalman converged:
    output Kalman PVT
else if smoothed WLS converged:
    output smoothed WLS
else if raw WLS converged:
    output raw WLS
else:
    no fix
```

Output rate should be configurable:

- Current continuous e2e: 1 Hz.
- GNSS-SDR CTTC config: `PVT.output_rate_ms=100`, or 10 Hz.
- Initial production target should support at least 1 Hz and 10 Hz modes.

## Channel Admission to PVT

Newly acquired or reacquired channels should not enter PVT immediately.

Policy:

- `TrackingProbation`: track and decode, but do not use in PVT.
- `Healthy`: default eligible for PVT.
- `Suspect`: continue tracking, default not used in PVT.
- `Lost`: not tracked and not used in PVT; schedule reacquisition.

Promotion from probation to healthy requires stable behavior over several PVT epochs or a minimum time interval:

- PLL lock metric above threshold.
- C/N0 above threshold.
- DLL/PLL errors stable.
- Doppler jumps within limits.
- navigation time tag available when required.
- ephemeris available when required.

This prevents a newly acquired or recently reacquired signal from disturbing WLS and Kalman before it is stable.

## Lock Detection

Use PLL lock metric as a core lock indicator, not C/N0 alone.

Status: `TrackingOutput::pll_lock_metric` is implemented as a prompt-phase coherence metric over the current lock window, and `ChannelHealthObservation::from_tracking_output` carries it into receiver channel admission. The metric is now visible in the real-data e2e tracking diagnostics for calibration.

Implemented tracking output:

```rust
pll_lock_metric: f64 // 0..1
```

Possible metric forms:

```text
I^2 / (I^2 + Q^2)
```

or a windowed prompt metric:

```text
abs(sum(P))^2 / sum(abs(P)^2)
```

Lock detector should combine:

- PLL lock metric.
- C/N0.
- DLL discriminator.
- PLL discriminator.
- Doppler jumps.
- nav/bit sync stalls.

Use continuous bad-epoch confirmation before declaring `Lost`, for example 200-1000 ms. Thresholds must be calibrated with real data.

## Acquisition Scheduler

Use periodic scans:

```rust
AcquisitionSchedulerConfig {
    cold_acq_interval_ms,
    reacq_interval_ms,
    max_acq_prns_per_scan,
    acq_window_ms,
    doppler_max_hz,
    doppler_step_hz,
    noncoherent_integrations,
}
```

Priority:

```text
reacquisition PRNs
  > configured visible/assisted PRNs
  > remaining PRNs
```

For CTTC regression, keep a fixed PRN mode:

```text
[1, 11, 17, 20, 32]
```

For general receiver mode, default to GPS PRNs 1..32.

## Determinism Rules

- Channel order is always fixed by PRN before feeding downstream logic.
- Acquisition candidate order is fixed by PRN and priority.
- Same-epoch parallel tracking results are collected and sorted before use.
- Per-channel tracking state is advanced only in time order.
- WLS input order is deterministic.
- Kalman is advanced sequentially by PVT epoch.
- Logs are collected and printed in deterministic order.

Parallelism may change which CPU core computes a channel, but it must not change what order the algorithm observes.

## Memory Measurement

Do not put peak-memory assertions inside default Rust unit tests initially.

Use the Windows-friendly local measurement script:

```powershell
rtk powershell -NoProfile -ExecutionPolicy Bypass -File scripts/measure-pvt-e2e-memory.ps1
```

It runs:

```text
cargo test -p gnss-core --test pvt_e2e test_pvt_e2e -- --nocapture
```

and records:

- peak Working Set
- peak Private Bytes
- wall time
- exit code
- process tree samples

Use it to compare:

- pre-streaming e2e
- streaming single-thread e2e
- deterministic parallel e2e

Default outputs:

```text
.scratch/pvt_e2e_memory.json
.scratch/pvt_e2e_memory_stdout.txt
.scratch/pvt_e2e_memory_stderr.txt
```

The script should still be launched through `rtk powershell ...` from the shell. It starts `cargo` directly inside the script so Windows process-name sampling can include the `cargo` and `pvt_e2e*` processes.

Current streaming single-thread baseline:

```text
elapsed_s=242.36
peak_working_set_mib=194.43
peak_private_mib=166.77
```

Current deterministic tracking-parallel baseline:

```text
elapsed_s=132.38
peak_working_set_mib=194.13
peak_private_mib=167.74
```

## Open Calibration Items

- PCPS non-coherent integration threshold.
- Doppler bin step for acquisition-only tracking initialization.
- PLL lock metric threshold.
- probation duration in ms or PVT epochs.
- output rate defaults for static, dynamic, and GUI modes.
- speed/time thresholds against GNSS-SDR NMEA.
