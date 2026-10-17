# Development workflow

Pinpoint targets the GNOME 51 SDK and requires Rust 1.97 or newer. Generate the locked Cargo
sources and build the development Flatpak for display-backed testing:

```sh
python3 scripts/generate-cargo-sources.py
./scripts/local-flatpak.sh development
```

Use `com.nedrichards.pinpoint.Devel.json` as GNOME Builder's build
configuration. Its `.Devel` identity tells Glycin that an uninstalled Builder
run is a development environment, so image decoding works without trying to
launch a nonexistent installed production app as its nested sandbox. The
production-shaped `com.nedrichards.pinpoint.json` remains the release and CI
manifest.

Build the production manifest before release:

```sh
./scripts/local-flatpak.sh
flatpak run --user com.nedrichards.pinpoint --version
```

The Flatpak source list makes Cargo builds offline and reproducible. Regenerate
`flatpak/cargo-sources.json` whenever `Cargo.lock` changes.

The manifest prepares `cargo-sources.json` in a build-only dependency module.
Meson orchestrates the Cargo build, checks and installation. Every Cargo command
uses `CARGO_TARGET_DIR=<meson-build-directory>/target` and inherits the same
Cargo home and SDK environment. The application build, application tests and
Clippy use the same Cargo profile: release for Meson `buildtype=release`, dev
otherwise. Core tests always run in release mode for their performance gate.
Run `meson compile -C <build-directory>` and
`meson test -C <build-directory> --print-errorlogs` inside the configured SDK
environment; avoid separate bare Cargo invocations from the checkout. Meson
delegates source freshness checks to Cargo, including embedded assets.

GNOME Builder stops Flatpak preparation before the primary application module
and then builds the live checkout directly, so the shared Cargo home in `/app`
must already contain the offline registry at that point. The path is removed
from the finished application by the manifest's cleanup rules.

GNOME Builder keeps its own Meson build directory and incremental Cargo cache;
the manifest-driven release build has a separate context. These caches should
not be merged across SDKs or host toolchains. The local Flatpak script retains
module-result caching and downloads but removes module working directories,
including compiled Cargo artifacts, after a build. An unchanged module can be
restored from cache; a changed application module compiles afresh. See
[local build storage](local-flatpak.md).

The CLI keeps internal `--validate-*` and `--capture-*` options for automated
installed-app tests, but omits them from user help. The scripts in `tests/`
exercise setup, stage rendering, transitions, media pressure, lifecycle,
speaker view, rehearsal, editor behavior, PDF output, and application-shell
contracts. Their fixtures stay in `tests/` and are mounted read-only by the
test scripts; they are not part of the installed production payload.
`run-page-curl-diff.sh` also uses the installed production app; set
`PINPOINT_APP_ID=com.nedrichards.pinpoint.Devel` to compare the installed
development app instead. It never builds Rust in the checkout. Hardware
scripts additionally cover two-display placement and hot-plug behavior.

Sysprof captures are disposable evidence, not source artifacts. Keep the
profiling scripts and record durable conclusions in documentation, but write
captures outside the checkout or delete them after review.

The optional C/Rust differential oracle expects a built sibling checkout at
`../fun-pinpoint`; it is a development tool and is not part of the application
or release package.

## Defensive testing

Parser fuzz targets live in `fuzz/` and use synthetic local input only. Run
them with nightly Rust and `cargo-fuzz`; see [`fuzz/README.md`](../fuzz/README.md).
CI performs short bounded runs, while a longer local campaign is appropriate
before a release. Dependency advisories are checked on dependency changes and
on a weekly schedule. Cargo source, license, and duplicate-version policy is
defined in [`deny.toml`](../deny.toml).

The production and development manifests deliberately omit access to
`org.freedesktop.Flatpak`. Embedded commands can use programs and files already
available inside Pinpoint's sandbox, but cannot use `flatpak-spawn --host`.
The temporary `--talk-name=org.freedesktop.Flatpak` override documented in the
format guide is only for a trusted, deliberate developer demonstration.
