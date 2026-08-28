# GNSScope

[English](README.md) | [简体中文](README.zh-CN.md)

GNSScope is an offline GPS L1 C/A software receiver and analysis desktop application written in Rust. It turns recorded interleaved IQ files into acquisition, tracking, navigation-message, observables, position/velocity/time (PVT), and persisted visualization results.

> GNSScope currently processes recorded files only. Live SDR input, real-time deadlines, and multi-constellation reception are not implemented.

## Highlights

- Reads interleaved signed `Int8` and `Int16` IQ recordings with configurable sample rate and intermediate frequency.
- Uses FFT-based parallel code phase search (PCPS) for GPS L1 C/A acquisition.
- Tracks multiple channels with DLL and Costas PLL loops while preserving per-channel NCO continuity.
- Performs bit synchronization, GPS NAV subframe decoding, broadcast ephemeris selection, and optional explicit RINEX navigation fallback.
- Produces pseudoranges, WLS position, Doppler velocity, Hatch-smoothed and Kalman candidates, GPS time, and UTC when an absolute week reference is available.
- Supports deterministic bounded parallel acquisition/tracking, channel admission, loss detection, and reacquisition.
- Provides a GPUI desktop workflow with editable receiver settings, a processing queue, SQLite run history, replayable plots, sky/C/N₀ diagnostics, and Raw/Smoothed/Kalman comparison.

## Project status

The original M0–M2 path is implemented: recorded IQ can be processed through acquisition, tracking, navigation decoding, continuous PVT, persistence, and GUI replay.

Current verification includes static CTTC and LabSat recordings. These datasets are not distributed with the repository. The next priority is dynamic/high-rate IQ validation with independent trajectory and time truth, including rapid Doppler, velocity recovery, shortened output epochs, and satellites that become visible after startup.

See the [roadmap](docs/roadmap.md) for priorities and the [receiver readiness audit](docs/receiver-readiness-audit.md) for measured evidence and known accuracy limitations.

## Receiver pipeline

```text
Recorded IQ
    → streaming reader
    → PCPS acquisition
    → DLL / Costas PLL tracking
    → bit synchronization and GPS NAV decoding
    → observables
    → WLS / Hatch / Kalman PVT
    → GPUI worker, SQLite history, and plots
```

The pipeline is bounded and single-pass. Acquisition and active-channel tracking can run in parallel, while receiver state is merged deterministically by PRN and PVT epochs advance serially.

## Workspace

| Crate | Responsibility |
| --- | --- |
| `gnss-data` | Generated GPS L1 C/A PRN tables with no runtime dependencies |
| `gnss-core` | IQ I/O, acquisition, tracking, NAV decoding, observables, PVT, channel management, and streaming orchestration |
| `gnss-gui` | GPUI desktop interface, background processing, run queue, SQLite persistence, replay, plots, and candidate comparison |

## Downloads

Prebuilt packages are published on the [GitHub Releases](https://github.com/Sereinme/GNSScope/releases) page:

- macOS Apple Silicon and Intel disk images (`.dmg`)
- Windows x64 installer (`.exe`) and portable archive (`.zip`)
- Debian/Ubuntu x64 package (`.deb`) and portable Linux archive (`.tar.gz`)

Verify downloads against the attached `SHA256SUMS`. The macOS bundle is ad-hoc signed but not notarized, and the Windows installer is not code-signed; the operating system may therefore require explicit confirmation before first launch.

## Quick start

Install a stable Rust toolchain, then run from the repository root:

```bash
cargo run -p gnss-gui
```

Select an IQ file in the application and configure its format, sample rate, intermediate frequency, acquisition grid, tracking loops, navigation policy, and PVT settings. Completed processing runs are immutable; reprocessing creates a new run.

## Validation

Run the normal workspace checks with:

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Real-data regressions require the gitignored datasets under `data/`. Some complete-file and performance regressions are intentionally ignored and must be selected explicitly in a release build. A green default test run without those datasets is not full receiver validation.

## macOS bundle

On macOS, assemble an ad-hoc signed application bundle with:

```bash
./scripts/package-macos.sh
```

The result is written to `target/release/bundle/osx/GNSScope.app`. This local development signature is not a substitute for Developer ID signing and notarization. See [macOS packaging](docs/macos-packaging.md) for details.

## Documentation

- [Roadmap](docs/roadmap.md) — authoritative priorities and sequencing
- [Receiver readiness audit](docs/receiver-readiness-audit.md) — real-data, runtime, memory, and accuracy evidence
- [Domain glossary](CONTEXT.md) — canonical receiver and product terminology
- [Architecture decisions](docs/adr/) — pipeline, persistence, GUI, and deferred signal-extension design

## Current limitations

- GPS L1 C/A only; the deferred signal trait design must not be confused with implemented multi-signal support.
- Recorded-file processing only; no live SDR device input or dropped-sample recovery.
- Static validation datasets do not establish high-dynamics performance.
- Absolute accuracy is not claimed without surveyed antenna coordinates or an independent reference receiver.
- Raw-IQ UTC requires RINEX or a configured reference week when the navigation message exposes only the truncated GPS week.
