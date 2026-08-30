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

Use the pinned GNOME 50 SDK and the Rust stable SDK extension. The release
Flatpak is the authoritative build. GNOME Builder and display-backed
development use the sibling `.Devel` manifest so Glycin can identify
uninstalled Builder runs correctly:

```sh
python3 scripts/generate-cargo-sources.py
flatpak-builder --user --force-clean flatpak-build com.nedrichards.pinpoint.json
flatpak-builder --run flatpak-build com.nedrichards.pinpoint.json pinpoint --version
flatpak-builder --user --force-clean flatpak-build-devel \
  com.nedrichards.pinpoint.Devel.json
```

Run `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets
--locked -- -D warnings`, and `cargo fmt --all -- --check` in that SDK
environment. Run the relevant installed-app script in `tests/` for changes to
rendering, media, lifecycle, PDF, setup, editor, or display handling.

## Style and changes

Use standard rustfmt output and keep Clippy warning-free. Prefer safe Rust;
keep the small OpenGL FFI boundary isolated in the page-curl implementation.
Add focused tests for behavioural changes and update the user documentation
when a CLI, format, accessibility, or editor contract changes. Use concise
Conventional Commit subjects and keep release metadata free of local paths.
