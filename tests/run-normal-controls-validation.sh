#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
flatpak run --user --filesystem="$root":ro com.nedrichards.pinpoint \
  --validate-normal-controls "$root/tests/fixtures/speaker-validation.pin"
