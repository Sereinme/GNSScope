# macOS Application Bundle

GNSScope can be assembled as a native macOS application bundle without installing an additional packaging tool:

```bash
./scripts/package-macos.sh
```

The release bundle is written to `target/release/bundle/osx/GNSScope.app`. Use `--debug` for a debug bundle or `--skip-build` when the matching binary is already current.

The bundle identifier is `io.gnsscope.desktop`. The script reads the application version from `crates/gnss-gui/Cargo.toml`, installs the matching binary into `Contents/MacOS`, renders the tracked `Info.plist` template, validates the result with `plutil`, and applies a local ad-hoc signature so LaunchServices recognizes the assembled bundle.

The ad-hoc signature has no distribution identity. Distribution outside the development machine still requires the normal Apple Developer ID signing, hardened-runtime, and notarization workflow. Bundle assembly does not change receiver behavior or the Windows/Linux executable build.

## Deterministic GUI Preview

Launch a bundled preview state with:

```bash
./scripts/run-gui-preview-macos.sh overview
./scripts/run-gui-preview-macos.sh settings-tracking
./scripts/run-gui-preview-macos.sh narrow-settings
```

Supported layouts are `overview`, `channels`, `position`, `navigation`, `settings`, `settings-tracking`, `comparison`, `interface-settings`, `narrow`, and `narrow-settings`. Set `GNSSCOPE_SKIP_BUILD=1` to reuse an already current release binary.

Preview mode uses bounded, deterministic PVT, channel, and receiver-event fixtures. It does not expose the local run library or persisted receiver events. The stable bundle identifier lets macOS accessibility and screenshot tooling target the GNSScope window without relying on an unbundled process name.

The preview script launches the bundle through macOS LaunchServices and waits for the application to close. It must not execute `Contents/MacOS/gnss-gui` directly: restricted automation parent processes can otherwise abort inside HIServices before GPUI initialization.
