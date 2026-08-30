#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
set +e
output=$(flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
  --filesystem="$root":ro \
  --env=PINPOINT_VALIDATION_MEDIA="$root/tests/fixtures/media-formats/pinpoint-h264-high.mp4" \
  com.nedrichards.pinpoint --validate-media 2>&1)
status=$?
set -e
printf '%s\n' "$output"
test "$status" -eq 0
line=$(printf '%s\n' "$output" | grep '^PINPOINT MEDIA PASS ')

printf '%s\n' "$line" | awk '
  {
    for (i = 1; i <= NF; i++) {
      split($i, pair, "=")
      value[pair[1]] = pair[2]
    }
    if (value["hwm_mib"] + 0 >= 1024.0)
      print "PINPOINT ADVISORY media peak RSS is " value["hwm_mib"] " MiB"
  }
'
