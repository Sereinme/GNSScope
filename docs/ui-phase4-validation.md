# Phase 4 Runs / New Analysis validation

Date: 2026-09-06
Status: in progress. Search, filtering, reordering, and representative keyboard paths
have native evidence; complete persisted workflow acceptance remains open.

## Isolated persisted fixture checkpoint

### Light English narrow and Finder follow-up

The stale idle test instance was restarted after two native clicks returned
`noWindowsAvailable`. The same marked database was launched at 800x720 with
`GNSSCOPE_THEME=light` and `GNSSCOPE_LANGUAGE=en`; no Rust code changed.

- Runs opens as a full-width replacement and shows `History 80/103 · Refine search`
  without clipping (`narrow-light-history-cap.jpg`).
- Searching `fixture-000` finds the oldest fixture outside the initial 80-row window
  and reports 1/1 (`narrow-light-search-oldest.jpg`).
- Tab/Return selects that record, closes Runs, restores the workbench and loads its
  persisted 1/1 epoch (`narrow-light-keyboard-replay.jpg`). This epoch remains a
  labelled synthetic display fixture, not a solution from zero IQ.
- An unmatched query displays the full English recovery hint and 0/0 footer
  (`narrow-light-empty.jpg`).
- The selected row's reveal action successfully opens Finder in the test directory.
  Finder accessibility confirms selected URL
  `file:///private/tmp/gnsscope-ui-workflow-20260906-a/fixture-000.dat`.
  Evidence: `finder-test-source-selected.jpg`. This supersedes the earlier failed
  Finder attempts, which remain historical notes below.

These results close the listed narrow/light/English Runs checks and successful Finder
reveal. Remaining native acceptance: concurrent queue/cancel and deletion interaction;
the full set of draft validation states is not covered by this small matrix.

### Valid-draft follow-up

The same isolated database was verified again after the initial native control call
returned `noWindowsAvailable`; reattaching and raising the application restored control.
No Rust code changed in this follow-up; strict package signature verification passed.

- Runs plus opened the native file picker. Selecting
  `/private/tmp/gnsscope-ui-workflow-20260906-a/synthetic-zero-iq.dat` produced a valid
  draft with the Start processing action enabled (`valid-draft.jpg`).
- Clicking Start processing changed the center from draft to the selected run
  workbench. The short zero-signal job completed (`draft-start-complete.jpg`).
- Read-only SQL shows ID `fdab7555-12bb-4587-b527-74e750ca2485`, completed,
  1,000,000 / 1,000,000 samples and 0 PVT epochs; total history is now 103.
- With search focused, then Tab moved to the first history row, the row and following
  list positions stayed unchanged in `row-before-focus.jpg` / `row-after-focus.jpg`.
  This accepts the observed desktop history-row geometry, not every component's focus
  behavior or the untested narrow/light matrix.

These results close valid-draft start and the observed history-focus geometry checks.
Concurrent queue/cancel, deletion interaction, successful Finder reveal, and updated
light/English/narrow checks remain open.

The next implementation is now available:

- `GNSSCOPE_TEST_LIBRARY` opens only an existing absolute database path whose
  `validation_fixture` preference equals `gnsscope-ui-workflow-v1`. A read-only marker
  check precedes recovery; invalid/missing/unmarked paths fail without default-library
  fallback. `GNSSCOPE_PREVIEW=1` still takes precedence and disables database access.
- `ui_workflow_fixture` creates a new directory (refuses existing targets), a 250 ms
  Int16 zero-IQ source and 101 labelled historical display fixtures. The synthetic
  persisted epoch is not derived from the zero-IQ recording and is not receiver proof.
- Active/history rows reserve border geometry before focus. Focus changes color only;
  padding compensates for the reserved border to avoid changing normal row height.
- New tests cover marker rejection before recovery and persisted copy/cancel/delete
  isolation from the original completed run and epochs.

Build SHA-256: `8b2820ce8759e598368eda5c555d167c49a5f79c8a4d882834a26194ef5fb037`.
GUI tests: 78 passed, 2 ignored. Strict workspace Clippy (all targets), formatting,
Release application/examples build, packaging, strict signature and shell syntax pass.

### Reproduction

Build the app and example, package the app, then use a new absolute directory:

```bash
cargo build --release -p gnss-gui --examples --bin gnss-gui
scripts/package-macos.sh --skip-build
target/release/examples/ui_workflow_fixture /private/tmp/gnsscope-ui-workflow-NEW
bash scripts/run-ui-workflow-macos.sh /private/tmp/gnsscope-ui-workflow-NEW/runs.sqlite3
```

The launcher accepts `GNSSCOPE_THEME`, `GNSSCOPE_LANGUAGE`, `GNSSCOPE_WINDOW_WIDTH`
and `GNSSCOPE_WINDOW_HEIGHT` overrides. It does not build automatically. These are
explicitly disposable test records, but are retained after the test for inspection.
Never point the workflow launcher at the ordinary user database.

### Native and database evidence

Test directory: `/private/tmp/gnsscope-ui-workflow-20260906-a`.

- Native app displays 80/101 with a refine-search hint and reads the selected fixture's
  persisted epoch. Capture: `persisted-history-cap.jpg`.
- Reprocess on fixture-100 starts the real worker against its zero-IQ file and finishes,
  increasing history to 102. Capture: `persisted-reprocess-complete.jpg`.
- Read-only SQL verifies original ID `bbd08455-1221-433c-9c2c-9257def5226b` remains
  completed with 1 epoch, while new ID `757f517e-2f5d-4dcf-81f0-3454ad42f2b5` is
  completed with 0 epochs. Both processed 1,000,000 samples and have identical
  `settings_json`. Zero signal producing no PVT is expected.
- Finder reveal is not accepted: its action returned `noWindowsAvailable`, and the
  subsequent Finder inspection suffered an exceptional tool delay and only showed
  Desktop. No successful file selection is claimed.

Still open: native valid-draft start and concurrent queue/cancel, deletion interaction,
successful Finder reveal, focused/unfocused geometry comparison, and the new
light/English/narrow matrix. Backend tests do not close those UI gates.

## Build and scope

Release executable SHA-256:
`1b3b837adfa4519678bcbeaf5533b85aef275fd52f00b3078ae308f17df0981d`.

The preceding implementation passed GUI tests (76 passed, 2 ignored), strict Clippy,
formatting, Release build and packaging. This continuation changed no Rust code;
strict deep signature verification passed again. Native inspection used the isolated
1440x900 dark Chinese preview, captured at 1229x768 by Computer Use. No real run
database was opened and no receiver was started.

## Observed results

- No-match search: History count 0, footer 0/0, explicit recovery hint. Active and
  Queue stay visible; selected workbench remains unchanged.
- `Run05` search: only the failed Run05 appears; count and footer both report 1.
- Filter menu: Down/Return selects an item. Choosing Completed leaves five rows and
  a 5/5 footer, with a visibly selected filter button.
- Queue reorder: moving Run02 down yields Run03 then Run02; moving Run02 up restores
  the initial order. Active run progress stays at 63%.
- Tab from search reaches Active, skips disabled queue controls, passes enabled queue
  actions, and reaches History. Focus is visibly distinct from selection.
- Return on focused Run09 selects Run09 without replacing active Run01. The historical
  plot is empty because this visual fixture has no persisted epoch payload.
- Tab/Return on selected Run09's reveal action displays the missing-source-file
  feedback. This verifies error handling, not successful Finder reveal.
- Tab then Space on Active restores Run01 and its 769-epoch workbench.

Screenshots under gitignored `artifacts/ui-visual-refactor-phase4/`:

- `history-empty.jpg`
- `history-one-match.jpg`
- `history-completed-keyboard.jpg`
- `queue-moved-down.jpg`
- `history-keyboard-focus.jpg`
- `history-return-selected.jpg`
- `reveal-missing-keyboard.jpg`

## Remaining gates

- Validate successful Finder reveal using a known test source.
- Use an explicitly isolated persisted test library for queue cancellation, deletion,
  reprocessing, valid draft start/queue, and immutable completed-run checks. The current
  database-free preview cannot supply evidence of these persisted workflows.
- Verify the over-80 hint visually with a larger fixture; unit tests already cover
  bounded row count, ordering, and search beyond the initial cap.
- Cover light/English and narrow layouts for the Phase 4 changes; Phase 3's matrix is
  not fresh proof of these new controls.
- Inspect focus geometry: row focus adds a border and visibly shifts following rows
  slightly. Stabilize this before final keyboard polish acceptance.

The next implementation slice should provide isolated persisted workflow fixtures
without exposing the user's ordinary run database, then exercise the remaining gates.
