# Pinpoint fuzz targets

The fuzz workspace exercises presentation/source parsing and legacy transition
JSON independently of GTK. Run it with a nightly Rust toolchain and
`cargo-fuzz`:

```sh
cargo +nightly fuzz run presentation
cargo +nightly fuzz run legacy_transition
cargo +nightly fuzz run source_analyzer fuzz/seeds/source_analyzer
cargo +nightly fuzz run duration_writeback fuzz/seeds/duration_writeback
```

When running under the GNOME SDK Flatpak, the host's ptrace policy prevents
LeakSanitizer from starting. For that environment only, set
`ASAN_OPTIONS=detect_leaks=0`; address sanitization and libFuzzer coverage stay
enabled. Normal host fuzz runs should keep leak detection enabled.

Keep minimized regression inputs as ordinary unit-test fixtures. Generated
corpus and artifact directories are intentionally ignored; small reviewed seed
inputs live in `fuzz/seeds/`.
