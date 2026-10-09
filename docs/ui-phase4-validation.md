# Phase 4 Runs / New Analysis validation

Date: 2026-09-06
Status: in progress. Search, filtering, reordering, and representative keyboard paths
have native evidence; complete persisted workflow acceptance remains open.

## GPUI dependency migration to gpui-kit (2026-10-09)

`gnss-gui` replaced its direct `gpui` 0.2.2 / `gpui-component` 0.5.1 /
`gpui-component-assets` 0.5.1 dependencies with the single `gpui-kit` 0.7.1 facade,
which pins the `gpui-pre` 0.3.8 GPUI family, `gpui-base` 0.7.1, `gpui-component`
0.7.1, and the default asset bundle. All imports moved to `gpui_kit::` and
`gpui_kit::component::` paths; `main.rs` boots through `gpui_kit::application()`
plus `gpui_kit::init`; the vendored history context-menu adapter was rebased onto
the 0.7.1 API surface (`Anchor`, focus arity). The old dev-only `runtime_shaders`
feature does not exist in gpui-pre 0.3.8 and is no longer referenced.

Verification evidence:

- `cargo test -p gnss-gui`: 83 passed, 0 failed, 2 ignored (baseline parity).
- `cargo test --workspace`: all targets pass.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D
  warnings`: clean (a `block v0.1.6` future-incompat notice originates inside the
  new dependency tree, not this codebase).
- `cargo build --release -p gnss-gui`: succeeds.
- Both ignored real-data regressions pass: CTTC complete-capture background
  persistence and LabSat full-constellation acquisition to PVT (466 s total).
- Release launch smoke (1440x900): the process runs stably, opens its SQLite run
  library, and terminates cleanly on SIGTERM without writing to the existing
  database.

Headless visual acceptance (2026-10-09): `examples/visual_capture.rs` renders the
production `GnsscopeApp` with the deterministic preview fixtures
(`GNSSCOPE_PREVIEW=1`) in an off-screen `VisualTestAppContext` window (real Metal
rendering, texture-read capture; no screen-recording permission involved) and
stores `artifacts/gpui-kit-migration/workbench-{dark,light}.png` at 1440x900
logical (2x device). Assertions pass: mean luminance 28.2 (dark) versus 242.0
(light), distinct color counts ~4.8k, and non-uniform frames. Programmatic
review of both captures confirms the populated workbench: title-bar search, Runs
dock with 当前运行/等待队列/历史记录 rows in every run state, sky plot with PRN
labels, ENU trajectory, PVT table, C/N0 panel, configuration snapshot rail,
timeline with NAV/REACQ state segments, and status strip; button controls render
with ~1 logical px borders in both a neutral and a teal-accent weight (the
`border_1` + `border_color` replacement for the removed custom-variant border
builder), Runs rows keep readable uniform backgrounds with an elevated active
row, and the two themes share geometry (region-variance correlation 0.989).

Boundary: native interaction acceptance remains open for the right-click
context-menu surface (delivery of a real right-click and the confirmation
dialog), which stays grouped with the existing Phase 4 native mouse gates.

## Workbench-wide visual hierarchy pass

Presentation-only refinement preserves the concept's three-column workbench,
existing breakpoints and shadowless history menu. Active/history selection uses
a left accent marker instead of a heavy full outline, keeping the selected wash
and existing focus behavior. Runs metadata and section labels were enlarged;
configuration rows now use 11 px text and 28 px minimum height, with neutral
section/readonly styling instead of semantic green. Plot padding was aligned to
12 px, and title/status-bar auxiliary typography was slightly increased.

GUI tests: 83 passed, 2 ignored. Strict GUI Clippy, Release build, packaging and
diff checks passed. The 1440×900 isolated instance (PID 37445) was verified by
lsof before native inspection. After the tool reported user activity, AX state
was refreshed before obtaining a screenshot. The screenshot showed readable
Runs/configuration text and neutral configuration sections in the current
scrolled workspace; user scroll position was preserved. This is a first-pass
hierarchy check, not full light/dark, narrow-window or populated-chart acceptance.
No receiver behavior, persistent run contents or queue policy was changed.

## Right-click target selection and compact menu styling

### Menu spacing and state refinement

The shared history-menu content now uses 30 px rows and 13 px interface text,
retaining 14 px icons and 8 px icon/text gaps. Duplicate horizontal padding was
removed (the parent already provides 8 px). Content spans the row width with
4 px corner rounding, semantic hover/pressed colors and a danger wash for the
delete item. No menu shadow was reintroduced; global styles are unchanged.

GUI tests: 83 passed, 2 ignored. Strict GUI Clippy, Release build and packaging
passed. Native 800×720 inspection on verified isolated PID 36449 confirmed the
flat menu silhouette, readable aligned text, and unclipped rows. Right-clicking
the second record selected it while keeping Runs expanded. Clicking Delete
opened the correct source/UUID confirmation (`229e52be-025a-46ff-b6a1-def9a75b6312`);
AX Cancel closed it and retained the selected row. Hover/pressed colors were
not captured in a stable native screenshot, so their visual acceptance remains
separate from the verified resting state and click/confirmation behavior.

### Scoped shadow correction

Latest user revision: the replacement shadow has now been removed entirely.
`menu_surface()` is a single overflow-clipping wrapper with no box shadows; the
dependency's default exterior shadow is clipped, with no outer elevation layer.
The regression asserts both clipping axes and absence of a wrapper shadow.
GUI tests: 83 passed, 2 ignored. Previous visual acceptance below applies only to
the superseded light-shadow version, not this newly shadowless version.

Zed source verification: its current main-branch ContextMenu uses `elevation_2`;
`StyledExt::elevation_2` selects `ElevatedSurface`, whose shadow implementation
contains a 2 px-offset/3 px-blur/12%-opacity shadow plus a weaker contact shadow.
Thus Zed is not literally shadowless. GNSScope intentionally follows the user's
explicit preference for no menu shadow instead of copying that elevation.
Sources:
- https://github.com/zed-industries/zed/blob/main/crates/ui/src/components/context_menu.rs
- https://github.com/zed-industries/zed/blob/main/crates/ui/src/traits/styled_ext.rs
- https://github.com/zed-industries/zed/blob/main/crates/ui/src/styles/elevation.rs

Visual follow-up: the same verified isolated process (PID 32682) subsequently
returned a stable native screenshot with the new context menu already open over
the selected `fixture-100.dat` row. The broad diffuse black halo is absent; a
short edge shadow, fine border, aligned icons and both action labels are visible
without clipping. The compact Runs list remains visible with the target selected.
This confirms the new shadow's rendered dark-theme appearance. A subsequent
outside click returned `noWindowsAvailable`, so this follow-up adds visual
evidence only, not fresh dismissal/keyboard interaction acceptance.

The history context-menu adapter now clips the dependency's hard-coded
`shadow_lg` at the menu surface and adds a local shadow with 4 px blur, 2 px
vertical offset, zero spread and 12% opacity. Borders, surface colors and
PopupMenu item/confirmation behavior remain unchanged. This is scoped to the
three history context-menu call sites, not a global popover/theme change.
The adapter preserves the gpui-component 0.5.1 context-menu implementation;
upstream provenance is in its header and Apache-2.0 text is under `licenses/`.

GUI tests passed (83 passed, 2 ignored); strict GUI all-target Clippy, Release
build and macOS packaging passed. The new isolated instance (PID 32682) was
verified against the test database using lsof. Native Runs-list expansion was
observed, but both right-click attempts failed with `noWindowsAvailable`,
including a window-Raise retry. The new shadow's rendered appearance remains
unverified; earlier screenshots below describe the previous shadow version.

### Native acceptance follow-up

The newly packaged app was launched explicitly against
`/private/tmp/gnsscope-ui-workflow-20260906-a/runs.sqlite3`; `lsof` verified
the database before native interaction (desktop PID 31178, then narrow PID 31666).
Native screenshots and AX observations confirmed:

- At 1440×900, right-clicking the unselected first row switched the selected
  border, workbench title and snapshot UUID to `9a477335`. The compact two-item
  menu rendered with aligned icons, separator and danger-colored delete text.
- Opening Delete showed a native confirmation with full target UUID
  `9a477335-7f12-4bfd-995e-6f20b087f06b` and the correct 1200-second source path.
  Clicking the AX Cancel button closed it and retained the record.
- Right-clicking the bottom visible `fixture-097.dat` selected that row and
  displayed both menu items within the window. Clicking outside dismissed it.
- At 800×720, opening the compact Runs list and right-clicking the second row
  switched selection without collapsing the list. A menu opened near the right
  edge shifted left and remained fully visible.
- Read-only SQL after testing confirmed 106 records, unchanged from before the
  test, and the confirmation target still completed. No permanent deletion was
  executed and the ordinary library was not targeted.

The requested mouse-selection and dark-theme menu-layout acceptance now passes
at both sizes. Keyboard acceptance is still open: two native Escape attempts did
not visibly dismiss the desktop menu; later coordinate actions intermittently
failed with `noWindowsAvailable`, and a Down-key observation captured a transient
window image. These observations do not establish whether keyboard delivery or
application focus/handling is responsible. Do not claim all keyboard behavior or
the broader Phase 4 workflow accepted. No speculative keyboard fix was applied.

Right-click now selects the captured history UUID before opening the menu in all
three history renderers. The compact Runs overlay remains expanded so the selected
target stays visible. The shared menu content uses 26 px rows, 14 px icons, 12 px
interface text, a bounded 208–240 px width, and a separator before the danger-colored
delete action. Existing themed popover borders, corners, shadows, keyboard handling
and screen-edge positioning are retained; there is no global theme override.
The styling borrows compact icon/text alignment from Zed and Comet menu sources,
using GNSScope's existing semantic palette rather than copying a new color system.

A GPUI regression exercises the actual menu builder and confirms target selection,
preservation of the narrow overlay, and no deletion on menu opening. GUI tests:
82 passed, 2 ignored. Strict GUI all-target Clippy passed. These checks cover menu
construction and state transitions, not native right-button delivery or rendered
visual acceptance; those remain unverified after the earlier native tool failures.

## Context-menu and identity-bound deletion correction

Following user confirmation, history management actions moved out of selected rows
into right-click context menus in the Runs dock and both other history renderers.
Left-click remains selection only. Menus offer source-file reveal and Delete run;
the latter requests a native warning prompt with source path/full run UUID and an
explicit statement that source IQ is retained. Cancel is the first/default answer.
The asynchronous confirmation captures the original UUID, never the current
selection or row index. Active/queued runs are rejected by the deletion guard.
Context menus reuse the existing themed GPUI component; the confirmation uses the
platform prompt behavior rather than introducing a new product modal style.

The approved deletion-flow regression was written first (initial compile failed
because the request API did not yet exist), then passed after implementation.
It verifies request/cancel keep both records, and confirmation still deletes only
the original target after selection and history order change. This exercises the
real request callback with GPUI's simulated native prompt answers, not native mouse
delivery. GUI tests: 81 passed/2 ignored; strict workspace all-target Clippy, Release
build, macOS packaging and diff check passed. Missing locked dependencies were
downloaded after the initial sandbox/offline attempts failed.

Native inspection of the newly packaged isolated instance confirmed no inline
reveal/delete controls on the selected history row. Two right-click attempts then
returned `noWindowsAvailable`; menu display and native prompt cancellation remain
unverified. No record was deleted during this correction's native inspection.

## 2026-09-08 native continuation

### Authorized long-source native attempt

After explicit user approval for the 19.2 GB logical sparse input, native Start
created A `9a477335-7f12-4bfd-995e-6f20b087f06b`. The GUI showed A processing while
the original fixture-100 replay remained readable (one synthetic epoch). Reprocess
created queued B (visible snapshot prefix `2dff2887`) while A was at 79%; B correctly
showed an empty 0/0 workbench rather than the original historical epoch.

The next coordinate click targeted the queue cancel position, but the subsequent
view had no active/queued job and B was absent from SQL, not persisted cancelled.
This is consistent with the completed-row delete control replacing the cancel
target as A/B finished before the click. Treat this as an unintended deletion of
the disposable B record, NOT successful cancellation or deletion acceptance. The
record has no normal UI restore path. Further coordinate mutations were stopped.

Read-only SQL confirmed A completed 4,800,000,000/4,800,000,000 samples with zero PVT
epochs. Original `bbd08455-1221-433c-9c2c-9257def5226b` remained completed with one
epoch; its source remained 4,000,000 bytes and the long source 19,200,000,000 bytes.
The ordinary user library was not targeted. Native concurrent cancellation remains
open. Next fix/validation should prevent stale-coordinate actions from becoming
destructive, e.g. an identity-bound confirmation before historical deletion.

The fixture example now accepts an optional second argument of 1..=1200 seconds
(default 120), validated before directory creation; existing fixtures are not
extended or overwritten. Release example build and formatting passed.

### Unattended real-worker scheduling evidence

`cargo test -p gnss-gui --lib cancelling_queued_copy_does_not_stop_real_worker`
passes. Unlike the earlier synthetic-active-state test, this starts the production
worker through `enqueue`, consumes its real events through `apply_worker_event`,
and checks the thread is unfinished both before and after queued-copy cancellation.
A advances after cancellation and completes all 4,000 samples. B persists as
cancelled with zero processed samples and never starts. Deleting B preserves the
original completed record/epoch and the source's exact zero bytes.

The fixture is 8,000 bytes of intentionally undersampled zero IQ (1 kHz Int8,
2 ms chunks/output epochs), not a valid RF capture or positioning test. Its 2,000
progress epochs exceed the production bounded channel capacity, so A cannot finish
while the test has consumed only its first progress event. No artificial worker
delay or alternate scheduler is used. GPUI's test context and in-memory SQLite
replace native rendering and disk persistence; their native/disk gates remain
separate. On assertion failure the test disconnects the receiver before joining
the worker, avoiding a blocked-sender cleanup hang.

GUI suite: 80 passed/2 ignored; strict all-target workspace Clippy passed. The new
worker thread-state accessor is test-only; production receiver behavior is unchanged.
During the control investigation, automated review rejected rebinding without a
PID/database proof. The follow-up process check found no GNSScope process and the
launcher had returned exit 0; this does not establish why native clicks previously
failed. No new native click success or root cause is claimed.

With no old GNSScope process running, the workflow launcher opened the marked
20260906-a library. Native inspection confirmed its 103 test records. Read-only
`lsof` also confirmed the sole process opened that exact database, not the default
library. The long-source draft was loaded successfully through the native picker.

The fixture generator now additionally creates `synthetic-zero-iq-120s.dat` with
`create_new` and `set_len`: 120 seconds of 4 Msps Int16 interleaved zero IQ,
1.92 GB logical sparse length. It does not extend the hard-linked short fixtures or
allocate that size in memory. Generated under
`/private/tmp/gnsscope-ui-workflow-20260908-a`; the running app still uses the older
isolated database. This source is for scheduling checks, not positioning proof.
The example Release build and formatting check passed.

Start clicks repeatedly returned `noWindowsAvailable`; the observed draft remained
unchanged and SQL still showed the previous completed runs. No successful start,
concurrent cancellation, or deletion is claimed. A rebind was rejected by automated
safety review because it could launch a default instance; subsequent checks used
the existing binding and explicit process/database identity only. Native acceptance
remains open; no original records or IQ files were deleted.

## Post-checkpoint queue selection correction

### Deterministic app-command regression

`enqueue_and_cancel_preserve_active_and_completed_history` now runs with GPUI's
`TestAppContext` and an injected in-memory RunLibrary. It calls the production app
methods `enqueue`, `cancel_queued`, and `delete_historical_run`, rather than a duplicate
workflow implementation. It seeds active state plus completed historical replay, then
asserts a distinct queued ID, empty selected-run replay, retained active ID/two epochs,
persisted cancellation/empty queue order, and deletion of only the new copy. The
original completed run and stored epoch survive. This is state/command coverage with
a synthetic active run, not native click or concurrently executing receiver evidence.

Constructor extraction `with_library` lets tests supply storage without reading or
mutating the default run database. GPUI `test-support` and `runtime_shaders` are
dev-dependency features only; the latter avoids requiring the missing offline Xcode
Metal Toolchain for test/lint builds. No toolchain download or production rendering
dependency change was made. Cargo.lock includes the test-support transitive packages.

Verification: GUI tests 79 passed/2 ignored; strict all-target workspace Clippy,
formatting, Release build, macOS packaging and strict signature verification pass.
The first Clippy attempt failed on missing Metal Toolchain, including outside the
sandbox; after enabling GPUI's supported dev-only runtime shader mode it passed.

Checkpoint commit: `7a8bd65` (shell extraction, isolated workflow tooling, and prior
evidence). Subsequent source review found that enqueueing while another run is active
selects the new queued ID but retained the previously inspected historical buffers.
The queue branch now clears historical epochs/channels/events and selected epoch,
and exits live-follow. Active-run buffers and worker ownership remain unchanged.
GUI regression tests pass (78 passed, 2 ignored), as do strict workspace Clippy,
formatting and diff checks. This correction has not yet had native concurrent-queue
acceptance or a new packaged-build inspection; the earlier screenshots predate it.

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
