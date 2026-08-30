#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
  --filesystem="$root":ro com.nedrichards.pinpoint --validate-editor \
  "$root/tests/fixtures/editor-validation.pin"
