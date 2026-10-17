# Local builds and updates

## Local Flatpak installation

Run the canonical build from the repository root:

```sh
./scripts/local-flatpak.sh
```

This builds and tests the production manifest, exports to `flatpak-repo/`,
then installs or updates the user application. Quit and reopen the app after
updating. Account settings and application data are preserved.

For the separate development application, use:

```sh
./scripts/local-flatpak.sh development
```

Both profiles share one staging directory, `.flatpak-builder/staging`, and
one Builder state directory, `.flatpak-builder/`. The script takes a checkout
lock, uses `--force-clean --delete-build-dirs`, and removes module working
directories and staging on success, failure, or an ordinary interrupt. SIGKILL
or power loss can leave working files; the next run reuses the same paths and
cleans them on exit. Do not run GNOME Builder or another build in this checkout
at the same time. Downloads and Builder's reusable cache remain available;
this is not a storage quota or a limit on cache growth. Builder caches completed
module results, not the removed Cargo target directory: source changes that
invalidate the application module require fresh compilation. GNOME Builder
development retains its separate Meson target directory for incremental work.

The build log is `.flatpak-builder/local-build.log` and is replaced on each
run. Run diagnostic commands and installed-app checks against `flatpak run`
after installation; staging is deliberately disposable. GNOME Builder can
still use the `.Devel` manifest for interactive development. Prefer the script
for automated builds and validation; do not create extra validation trees.

`flatpak-repo/` is the durable local update repository, outside disposable
build state. Keep it when cleaning `.flatpak-builder/`. The production remote
is `pinpoint-origin`; development uses `pinpoint-ng-devel-local`. The script
registers or repoints these remotes only after the build, tests and export succeed. If the app was
installed from another origin, the script reinstalls that app from the intended
remote, preserving its application data without repointing the previous remote.
Source manifests exclude the repository and generated build directories from
directory copies.

Never export an installed `export/` directory with `flatpak build-export`:
its desktop entry already includes the host Flatpak wrapper. Let the normal
manifest-driven build finish and export the staging tree.
