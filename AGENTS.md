# GNSScope

GNSScope is an offline GPS L1 C/A software receiver and analysis desktop application written in Rust. It processes recorded Int8 or Int16 interleaved IQ files through acquisition, tracking, navigation-message decoding, observables, continuous PVT, and GUI persistence/visualization. Live SDR input is not implemented.

## Project

- **Workspace:** Rust 2021 with `gnss-data`, `gnss-core`, and `gnss-gui` crates.
- **Entry point:** `crates/gnss-gui/src/main.rs`.
- **Reference implementation:** GNSS-SDR under `reference/gnss-sdr/` when present.
- **Current status:** M0-M2 are implemented. `docs/roadmap.md` is the authoritative priority and status document; `docs/receiver-readiness-audit.md` contains detailed verification evidence.

## Commands

```bash
cargo run -p gnss-gui
cargo test --workspace
cargo test -p gnss-core --lib
cargo test -p gnss-gui --lib
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Real-data regressions require the gitignored datasets under `data/`; some long release regressions are marked `#[ignore]` and must be selected explicitly. A green default test run without those datasets is not full receiver validation.

## Architecture

- `gnss-data`: generated GPS L1 C/A PRN tables with no runtime dependencies.
- `gnss-core`: IQ I/O, PCPS acquisition, DLL/PLL tracking, bit synchronization, GPS NAV decoding, RINEX navigation fallback, observables, WLS/Hatch/Kalman PVT, velocity/time output, channel admission, reacquisition, and `StreamingPvtRunner` orchestration.
- `gnss-gui`: GPUI desktop interface, editable processing settings, background worker, run queue, SQLite run library, historical replay, plots, and Raw/Smoothed/Kalman candidate comparison.

The receiver is a bounded, single-pass file-streaming pipeline. Acquisition and active-channel tracking can run in parallel, but state and events are merged deterministically by PRN and PVT epochs advance serially. Preserve absolute sample indices and per-channel NCO continuity across chunks.

ADR-0002's signal trait abstraction is accepted design intent but remains deferred: production code currently uses concrete GPS L1 C/A types. Do not describe multi-signal support as implemented.

## Current Priorities

Follow `docs/roadmap.md`. The next work is accuracy and scenario coverage, starting with files ending in an incomplete final 1 ms tracking epoch, followed by dynamic/high-rate datasets, independent reference comparisons, time/ephemeris boundaries, and complete serial-versus-parallel real-data equality. Measure runtime and memory before further optimization.

## Conventions

- Use the canonical domain terms from `CONTEXT.md` in code, docs, and issues.
- Read relevant ADRs before changing pipeline, persistence, GUI, or signal-extension design.
- Treat `docs/M1-PROGRESS.md`, `docs/M2-PROGRESS.md`, and PVT research notes as historical investigation records. They contain superseded failures and are not the current backlog.
- Edit `scripts/gen_prn_codes.py`, not generated `crates/gnss-data/src/prn_codes.inl`.
- Keep completed processing runs immutable; reprocessing creates a new run.
- Preserve deterministic serial/parallel behavior and verify signal-processing changes against real-data regressions.

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles use their default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: one `CONTEXT.md` plus `docs/adr/` at the repo root. See `docs/agents/domain.md`.
