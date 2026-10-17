# Repository Guidelines

## Structure

`pinpoint/` contains the GTK 4/libadwaita application. `pinpoint-core/`
contains deterministic parsing, source intelligence, rendering geometry,
transitions, rehearsal writeback, and command policy. Release assets live in
`data/`, user documentation in `docs/`, regression fixtures and installed-app
gates in `tests/`, and Flatpak packaging in `flatpak/`.

The large `desktop-summit/` and `guadec-2018/` decks are local compatibility
fixtures and are intentionally ignored.

## Build and test

Use the pinned GNOME 51 SDK and the Rust stable SDK extension. The release
Flatpak is the authoritative build. GNOME Builder and display-backed
development use the sibling `.Devel` manifest so Glycin can identify
uninstalled Builder runs correctly:

```sh
python3 scripts/generate-cargo-sources.py
./scripts/local-flatpak.sh
flatpak run --user com.nedrichards.pinpoint --version
./scripts/local-flatpak.sh development
```

Meson orchestrates Cargo builds, tests, Clippy and formatting. Run
`meson compile -C <build-directory>` and
`meson test -C <build-directory> --print-errorlogs` in the configured SDK
environment. Every invocation must use the same Meson-owned
`CARGO_TARGET_DIR=<build-directory>/target` and inherited Cargo home. Application
builds, tests and Clippy share the selected Cargo profile; core tests always
use release mode for their performance gate. Keep GNOME Builder development
and manifest-driven Flatpak contexts separate; do not run bare Cargo commands
that create a checkout `target/` cache. Run the relevant installed-app script in
`tests/` for changes to rendering, media, lifecycle, PDF, setup, editor, or display handling.

## Local Flatpak build storage

Use `./scripts/local-flatpak.sh` for production builds and updates, or
`./scripts/local-flatpak.sh development` for the separate `.Devel` app.
Use the normal manifests and keep their test gates enabled.

- Keep the durable update repository at `flatpak-repo/`. Never place it under
  `.flatpak-builder/` or another disposable build/cache directory.
- Use `.flatpak-builder/` for Builder state and `.flatpak-builder/staging` as
  the single staging location for both profiles. Build sequentially. Do not
  create per-task, per-validation or timestamped build/state/repository trees.
- Use `--delete-build-dirs` and clean module working directories and staging
  after success, failure and ordinary interruption, as the script does. Keep
  the reusable cache and downloads. Do not use `--keep-build-dirs` for routine
  automated builds. Debug retention needs an explicit task-specific reason
  and cleanup when finished.
- Do not run GNOME Builder or raw Builder commands concurrently with the
  script. Interactive GNOME Builder development remains supported via the
  `.Devel` manifest; prefer the script for automated validation and deployment.
- Keep directory-source `skip` lists excluding generated build directories,
  `target`, `.flatpak-builder*` and `flatpak-repo`, so artifacts are not copied
  into subsequent builds.
- Verify the installed app and its update origin after deployment. Never
  re-export an installed `export/` directory.

There is no enforced storage budget. Do not change Rust incremental/debug
settings as part of storage housekeeping without a separate request.

## Style and changes

Use standard rustfmt output and keep Clippy warning-free. Prefer safe Rust;
keep the small OpenGL FFI boundary isolated in the page-curl implementation.
Add focused tests for behavioural changes and update the user documentation
when a CLI, format, accessibility, or editor contract changes. Use concise
Conventional Commit subjects and keep release metadata free of local paths.
