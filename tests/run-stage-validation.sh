#!/bin/sh
set -eu

app_id=com.nedrichards.pinpoint
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
smoke=$root/tests/fixtures/stage-smoke.pin
pixels=$root/tests/fixtures/pixel-reference.pin

flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 --filesystem="$root":ro \
  "$app_id" --check "$smoke"

stage_output=$(flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 --filesystem="$root":ro \
  "$app_id" --validate-stage "$smoke" 2>&1)
printf '%s\n' "$stage_output"
stage_line=$(printf '%s\n' "$stage_output" | grep '^PINPOINT STAGE PASS ')

printf '%s\n' "$stage_line" | awk '
  {
    for (i = 1; i <= NF; i++) {
      split($i, pair, "=")
      value[pair[1]] = pair[2]
    }
    if (value["hwm_mib"] + 0 >= 512.0)
      print "PINPOINT ADVISORY peak RSS is " value["hwm_mib"] " MiB"
    if (value["frame_p95_ms"] + 0 >= 40.0)
      print "PINPOINT ADVISORY p95 frame interval is " value["frame_p95_ms"] " ms"
    if (value["frame_max_ms"] + 0 >= 750.0)
      print "PINPOINT ADVISORY worst frame interval is " value["frame_max_ms"] " ms"
    if (value["texture_mib"] + 0 > value["texture_budget_mib"] + 0)
      print "PINPOINT ADVISORY one retained texture exceeds the adaptive cache budget"
  }
'

pixel_output=$(flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 --filesystem="$root":ro \
  "$app_id" --validate-pixels "$pixels" 2>&1)
printf '%s\n' "$pixel_output"
printf '%s\n' "$pixel_output" | grep '^PINPOINT PIXELS PASS '
printf '%s\n' "$pixel_output" | grep '^PINPOINT STAGE PASS '
