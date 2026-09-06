# Phase 3 shell extraction validation

Date: 2026-09-05
Status: implemented and macOS shell acceptance passed; ready for human review.

## Scope

The concept drawing remains the layout authority. This phase extracts presentation
into `ui/shell`, `title_bar`, `runs_dock`, `run_rows`, `run_configuration`,
`epoch_dock`, `status_strip`, and `plot_frame`. Receiver, worker, queue, historical
replay, selected/active identity, and persistence ownership remain in `app.rs`.
Dormant legacy render paths are retained. This is not final plot or accessibility
acceptance for the later phases.

## Packaged build

Release executable SHA-256:
`365e8a54c67a25fb68305f9f0ab91daa177f9fe686f643083e4fde0f037c8941`.

The preceding implementation turn passed complete workspace tests (including available
real-IQ tests), GUI library tests (74 passed, 2 ignored), strict workspace Clippy,
formatting, Release build, and macOS packaging. This verification-only continuation
changed no Rust code and rechecked strict deep signature verification. Ignored long
release campaigns were not run.

All visual interactions used `GNSSCOPE_PREVIEW=1`; this mode does not open the real
run database. The active 63% progress and epoch history are deterministic fixtures,
not proof of new live receiver execution. A stale preview process was closed before
each size transition after the quit shortcut did not terminate it.

## Size and theme matrix

The preview launcher supplies the requested logical size. Computer Use scales large
window captures to 768 pixels high; encoded image dimensions are not viewport sizes.
Each row was inspected in dark Chinese and light English navigation. The desktop
captures use Overview; the narrow captures use Channels.

| Requested size | Encoded capture | Observed shell |
| --- | --- | --- |
| 1568x1003 | 1201x768 | Runs + two-column Overview + configuration; full-width epoch dock |
| 1440x900 | 1229x768 | Same three regions; configuration close/restore expands/contracts workbench |
| 1100x800 | 1056x768 | Configuration collapsed, Runs retained, vertically arranged plots |
| 800x720 | 800x720 | Runs/configuration collapsed by default; C/N0 and epoch access remain visible |

Eight local captures are under `artifacts/ui-visual-refactor-phase3/`, named
`matrix-{800,1100,1440,1568}-{dark-zh,light-en}.jpg`. Screenshots are intentionally
gitignored local artifacts. The purple screen-sharing pill at the top-left is macOS
capture chrome, not a GNSScope theme element.

## Interaction evidence

- Narrow Runs toggles to a full-width list and back, preserving the destination.
  Evidence: `narrow-runs-open.jpg`.
- Selecting completed Run09 closes narrow Runs and displays its identity; the global
  active run stays at 63%. History fixtures have no persisted epochs, so the empty
  epoch view is expected. Evidence: `narrow-history-selected.jpg`.
- Return Live restores Run01 and 769 epochs. First selects 1/769; Next advances to
  2/769; zoom changes 1x to 2x; Follow restores 769/769. Evidence:
  `narrow-epoch-first-zoom.jpg` and `matrix-800-dark-zh.jpg`.
- Opening Global Settings while narrow Runs is expanded closes that transient list.
  Theme/language controls work and Back restores the workbench. Evidence:
  `narrow-settings-from-runs.jpg` and `matrix-800-light-en.jpg`.
- Desktop configuration close expands the workbench; its toolbar action restores the
  rail, also after visiting Settings. Evidence: `desktop-configuration-closed.jpg`
  and `matrix-1440-light-en.jpg`.
- The Runs plus action opens New Analysis. Cancelling the native file picker leaves
  an empty draft with validation and active processing still visible. Verified at
  both reference and narrow sizes. Evidence: `desktop-draft-picker-cancel.jpg` and
  `narrow-draft-picker-cancel.jpg`. Selecting the active row restores its workbench.

## Explicit follow-ups

- Some plot titles remain Chinese in English mode; only shell/navigation localization
  is accepted here. Complete plot localization in the workbench phase.
- Overview PVT values appear visually clipped even with the configuration rail closed;
  inspect table cell layout in the workbench phase before claiming full numeric
  readability. No receiver data change or cause is inferred from the screenshot.
- Full historical replay, all queue mutations, exhaustive keyboard/accessibility
  coverage, cross-platform behavior, and final plot polish are not new visual evidence
  from this phase. Their respective implementation tests/later gates remain required.
