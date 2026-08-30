# Licensing and provenance

Pinpoint 0.3 is distributed under the GNU Lesser General Public License,
version 2.1 or later. The complete license text is in [`COPYING`](../COPYING).

This Rust implementation preserves the format and user-visible behaviour of
the original Pinpoint and its GTK 4 C successor. The About dialog credits the
original Pinpoint contributors and records their LGPL-2.1-or-later work as
inspiration. Project-authored Rust, metadata, tests, and documentation are
declared LGPL-2.1-or-later; AppStream metadata is CC0-1.0.

The bundled introduction's images and source come from the official Pinpoint
0.1.8 release under LGPL-2.1-or-later. Its short *Big Buck Bunny* excerpt is
CC BY 3.0. Exact source, attribution, and encoding details are recorded in
[`data/introduction/ORIGIN.md`](../data/introduction/ORIGIN.md) and repeated in
the About dialog.

Rust dependencies retain their own upstream licenses. `Cargo.lock` fixes the
release dependency graph, while the Flatpak build downloads checksum-verified
crate sources. Before publishing, generate and review a machine-readable
third-party inventory with `cargo deny` or an equivalent Cargo license tool;
do not commit the local `vendor/` directory as a substitute for that review.
