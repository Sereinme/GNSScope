# GNSScope

GNSScope is an offline GPS L1 C/A software receiver and analysis desktop application written in Rust. It processes recorded Int8 or Int16 interleaved IQ files through acquisition, tracking, navigation-message decoding, observables, continuous PVT, and GUI persistence/visualization. "Streaming" in this repository means bounded, single-pass processing of a file; live SDR input is not implemented.

The main reference capture is GNSS-SDR's `2013_04_04_GNSS_SIGNAL_at_CTTC_SPAIN` at 4 Msps, zero IF, Int16 interleaved I/Q. A second LabSat capture exercises signed Int8 I/Q at 10.23 Msps. Both datasets are gitignored and live under `data/` when available.

Algorithm implementations deliberately follow GNSS-SDR where noted. When behavior is unclear, compare with the corresponding implementation under `reference/gnss-sdr/` rather than inferring from an old progress note.

## Current State

M0-M2 are implemented. The production path includes streaming IQ input, deterministic parallel acquisition/tracking, channel admission and periodic reacquisition, GPS NAV decoding, optional RINEX navigation fallback, continuous Raw/Smoothed/Kalman PVT, Doppler velocity, GPS/UTC time, a real GPUI worker, editable configuration, SQLite processing-run persistence, history replay, and candidate comparison.

`docs/roadmap.md` is the authoritative source for current priority and completion status. `docs/receiver-readiness-audit.md` records detailed real-data and performance evidence. `docs/M1-PROGRESS.md`, `docs/M2-PROGRESS.md`, and the PVT research documents are historical investigation records; their open-problem sections have been superseded by later fixes unless the roadmap or readiness audit repeats them.

The current roadmap priority is accuracy and scenario coverage: incomplete final 1 ms epochs, dynamic/high-rate PVT and rapid Doppler changes, independent reference comparisons, ephemeris/GPS-week/leap-second boundaries, and complete real-data serial-versus-parallel equality. Runtime and memory measurement comes before additional optimization.

## Commands

```bash
cargo run -p gnss-gui
cargo test --workspace
cargo test -p gnss-core --lib
cargo test -p gnss-gui --lib
cargo test --release -p gnss-core --test pvt_e2e -- --nocapture
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Long real-data regressions marked `#[ignore]` must be selected explicitly. For example:

```bash
cargo test --release -p gnss-core --test streaming_runner_real test_streaming_runner_real_data_resumes_pvt_after_multi_channel_reacquisition -- --ignored --nocapture
cargo test --release -p gnss-core --test labsat_int8_real -- --ignored --nocapture
```

Many real-data tests return early when their gitignored dataset is absent, so a green default test run does not prove end-to-end receiver behavior. Check test output and dataset presence before citing real-data verification.

Regenerate PRN data through its source script:

```bash
python scripts/gen_prn_codes.py
```

Do not manually edit `crates/gnss-data/src/prn_codes.inl`.

## Workspace

### `gnss-data`

Zero-dependency static GPS L1 C/A data. `gps_l1ca.rs` exposes the generated `PRN_CODES: [[i8; 1023]; 32]` table.

### `gnss-core`

- `io/`: sequential Int8/Int16 interleaved IQ reading and resampling helpers.
- `acquisition/`: FFT-based PCPS acquisition with Doppler search and noncoherent integration.
- `tracking/`: DLL/Costas PLL tracking, carrier aiding, lock/C/N0 metrics, and bit synchronization.
- `nav/`: GPS NAV parity/subframe decoding, ephemeris and ionosphere parsing, and optional RINEX navigation sources.
- `pvt/`: observables, pseudorange formation, satellite state, atmospheric corrections, WLS/RAIM, Hatch smoothing, Kalman filtering, velocity, coordinates, and time conversion.
- `receiver/`: `StreamingPvtRunner`, channel health/admission, fair cold-acquisition and reacquisition scheduling, deterministic tracking merge, navigation state, PVT cadence, events, and acquisition buffering.

`receiver/mod.rs` is the production orchestration path. Preserve absolute sample stamps, common receive epochs, channel-local temporal order, NCO continuity, and deterministic PRN ordering when changing it.

### `gnss-gui`

The GPUI application is fully wired, not a placeholder. It includes a background file-processing worker, receiver configuration and validation, task queue controls, SQLite run persistence, crash-to-interrupted recovery, historical epoch replay, responsive light/dark layouts, custom GNSS plots, and read-only Raw/Smoothed/Kalman candidate comparison.

Completed processing-run snapshots are immutable. Reprocessing creates a new processing run with a new configuration snapshot. The active processing run and selected processing run are separate concepts; browsing history must not stop background processing.

## Architecture Constraints

### Streaming pipeline

ADR-0001 defines a bounded, single-pass file-processing pipeline. IQ is read in configurable chunks and subdivided into 1 ms GPS L1 C/A tracking steps. Acquisition can integrate multiple milliseconds. Acquisition and active tracking can run in parallel, while results are merged by PRN before shared receiver state changes and PVT epochs advance serially.

### Signal abstraction

ADR-0002 accepts `SignalSpec`/`Acquisition`/`Tracking`/`NavDecoder` traits as the intended extension design, but these traits are not implemented. Current production code uses concrete GPS L1 C/A types. Treat multi-signal and multi-constellation support as future work and do not add speculative seams until a second implementation needs them.

### Persistence and GUI

ADR-0003 selects a versioned SQLite processing-run library under the user's application-data directory. It stores derived results and source metadata, not IQ file contents. ADR-0004 assigns standard controls to `gpui-component` and domain plots to native GPUI drawing; compatible GPUI dependency revisions must move together.

## Verification Guidance

- Unit tests live beside implementations; integration and real-data tests live under `crates/gnss-core/tests/` and in the GUI worker module.
- `pvt_e2e.rs` exercises the full CTTC receive chain; `streaming_runner_real.rs` exercises the production runner and recovery; `labsat_int8_real.rs` covers the LabSat format and long PVT path.
- Preserve serial/parallel numerical equivalence and deterministic event order.
- Do not weaken quality gates or replace unavailable/non-converged candidate values with zeros to make GUI or tests pass.
- Position, velocity, time, and performance claims should identify the dataset, duration, build profile, and whether an ignored regression actually ran.

## Conventions

- `CONTEXT.md` defines the canonical domain vocabulary. Use those terms in code, docs, and issues.
- Read the relevant ADR before changing pipeline, persistence, GUI, or signal-extension design. Surface conflicts explicitly.
- `docs/roadmap.md` owns priorities; do not create a competing backlog in progress or audit documents.
- The generated PRN table is derived output; change `scripts/gen_prn_codes.py` and regenerate it.
- Keep edits scoped and preserve user changes in the worktree.

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles use their default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: one `CONTEXT.md` plus `docs/adr/` at the repo root. See `docs/agents/domain.md`.
