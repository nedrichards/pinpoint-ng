# Release checklist

Pinpoint uses Flathub's reproducible-build and packaging conventions as a
quality benchmark while the project matures. Publishing on Flathub is an
aspiration, not a release requirement, and is currently out of scope. GitHub
releases must stand on their own; do not delay one solely for Flathub readiness
or prepare a Flathub submission without a separate explicit decision.

1. Regenerate `flatpak/cargo-sources.json` and require a clean locked build.
2. Run workspace tests, strict Clippy, rustfmt, AppStream, desktop, MIME, XML,
   Markdown-link, source-path, Cargo policy, advisory, and bounded fuzz checks.
3. Build the production Flatpak and run the setup, shell, stage, transition,
   lifecycle, media, speaker, rehearsal, editor, and PDF gates.
4. Manually review setup, editor, export, About, keyboard focus, accessibility,
   camera permission handling, and representative one- and two-display talks.
5. Confirm `COPYING`, `docs/licensing.md`, introduction provenance, and the
   generated third-party license inventory remain accurate.
6. Confirm ignored build outputs, profiler captures, vendored crates, local
   talks, workstation paths, signing material, and credentials are absent, and
   run `scripts/check-production-manifest.py` to reject test fixtures in the
   installed payload.
7. Confirm neither Flatpak manifest grants `org.freedesktop.Flatpak` access and
   review every manifest permission against the documented feature that needs it.
8. Commit and tag only after the release commit builds from a fresh checkout.
