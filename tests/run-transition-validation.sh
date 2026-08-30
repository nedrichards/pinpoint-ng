#!/bin/sh
set -eu

app_id=com.nedrichards.pinpoint
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
presentation=${1:-$root/tests/fixtures/page-curl-validation.pin}

flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
  --filesystem="$root":ro "$app_id" --validate-transitions "$presentation"
