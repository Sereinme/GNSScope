# GNSScope UI Architecture

Date: 2026-08-29

This document defines the product-facing structure of the GNSScope desktop app. It
complements ADR-0003 (run persistence) and ADR-0005 (GNSScope-owned visual system).
The receiver pipeline and persisted run model remain authoritative for domain
behavior.

## Product Model

GNSScope is an offline receiver workstation, not a generic dashboard. Its primary
workflow is:

1. **Create an analysis**: choose a local IQ recording, review detected metadata,
   configure the receiver, validate the draft, and start or queue a run.
2. **Monitor processing**: keep the active run visible while allowing the user to
   inspect another completed run. Cancellation affects only the active run.
3. **Inspect results**: move between overview, channel, position, and navigation
   workbenches over one shared epoch timeline.
4. **Reopen a run**: search the immutable run library, manage the waiting queue, and
   replay persisted results. Reprocessing creates a new run.

The UI must always distinguish these three objects:

- **Next-run draft**: editable configuration opened from the Runs-dock `+` action.
- **Active run**: background processing state and progress.
- **Selected run**: the run currently shown in the central workbench.

## Information Architecture

GNSScope has no global activity bar or collection of top-level domain pages. The
application shell has four persistent regions:

1. **Runs dock**: Active, Queue, and History sections with search and contextual run
   actions. A small `+` in its header creates a New Analysis draft. A small button at
   the bottom-left collapses or restores the dock.
2. **Selected-run workbench**: the central surface with Overview, Channels, Position,
   Navigation, and Candidates tabs. These tabs always describe the selected run, not
   the application globally.
3. **Epoch/PRN inspector**: an optional right pane for the selected epoch and linked
   signal, tracking, geometry, PVT, and navigation details.
4. **Epoch and status dock**: a persistent selected-run timeline above a global status
   strip that keeps active processing visible even while history is inspected. The
   timeline spans beneath both the analysis panes and the right inspector; the
   inspector must not terminate it early.

Global Settings is the only application-level destination. Receiver configuration is
run-local and distinct from interface preferences. Events, channels, navigation
frames, candidates, and configuration snapshots are children of a run.

New Analysis temporarily occupies the central workbench. Starting it creates an
immutable run snapshot and turns the same working context into the run workspace;
when another run is active, it enters the queue instead.

## Visual System

- Use the platform UI font for navigation and prose, and the platform monospace font
  for measurements, epochs, identifiers, and progress values.
- Use cyan-teal as the single interaction accent. Green, amber, and red are reserved for
  healthy, attention, and failure states. Status must also include text or shape.
- Prefer flat graphite panes separated by one-pixel dividers and 0-4 px radii. Avoid
  decorative gradients, glass effects, raised dashboard cards, and excessive shadow.
- Keep control labels visible. Icon-only controls require tooltips and are limited to
  compact navigation or conventional actions.
- Use 8 px as the base spacing rhythm, with 4 px for tightly related controls and
  12-16 px inside cards.
- Keep charts native to GPUI. A plot must have a title, units or a clear subtitle,
  empty/loading states, accessible status cues, and a table or textual diagnostic
  counterpart where useful.
- C/N0 is a per-run, per-epoch time series rather than a PRN bar chart. Show a bounded
  set of linked PRN lines, current values, gaps for missing lock, and the shared epoch
  cursor from persisted channel snapshots.

## Responsive Behavior

- At desktop width, use a roughly 272 px Runs dock, flexible workbench, and roughly
  278 px inspector when space permits. The analysis canvas receives the saved space;
  measurements stay dense rather than shrinking into dashboard cards.
- At narrower widths, collapse the inspector first and then the Runs dock. Do not
  stack the run library above plots or introduce a replacement activity bar.
- Configuration pages become vertically stacked at compact widths.
- A full-width drawer may temporarily replace the workspace on narrow windows; it must
  never render underneath or on top of unreadable workspace content.

## Interaction and State Rules

- New Analysis appears once as the Runs-header `+`; it is not a full-width permanent
  row competing with Active and History.
- Channels, Position, Navigation, Candidates, Event Log, and Run Configuration always
  resolve against the selected run. Global Settings never changes a run snapshot.
- Validation errors appear next to their fields and are summarized at the top of the
  receiver configuration area.
- A cancelled file picker leaves the user on a valid empty New Analysis page.
- Starting a valid draft switches the center to its run workspace; starting while
  another run is active queues a new immutable run snapshot.
- Selecting a historical run does not stop or replace the active run.
- Moving the epoch slider updates every run-local view, exits live-follow, and keeps
  its selection across tab changes. Follow Live returns to the latest available epoch.
- Hovered or pinned PRN identity links plots, channel rows, and the inspector.
- Empty pages explain what belongs there and provide one clear next action.

## Verification Matrix

Required visual checks cover 1440x900, 1100x800, and 800x720 in light and dark themes,
with both Chinese and English navigation. State coverage includes empty draft, valid
draft, validation errors, queued, processing, completed, failed, cancelled, and
interrupted runs. Formatting, GUI unit tests, strict Clippy, and the macOS package
build remain required before release.
