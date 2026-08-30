#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
c_root=${PINPOINT_C_SOURCE:-"$root/../fun-pinpoint"}
app_id=com.nedrichards.pinpoint

if [ "$#" -eq 0 ]; then
  set -- \
    "$root/guadec-2018/product-management-open-source.pin" \
    "$root/desktop-summit/desktop-summit.pin"
fi

if [ ! -x "$c_root/_build-codex/src/pinpoint" ] && [ ! -x "$c_root/_build/src/pinpoint" ]; then
  echo "build the C implementation before running real-deck validation" >&2
  exit 2
fi

c_binary="$c_root/_build-codex/src/pinpoint"
if [ ! -x "$c_binary" ]; then
  c_binary="$c_root/_build/src/pinpoint"
fi

for presentation in "$@"; do
  if [ ! -f "$presentation" ]; then
    echo "missing presentation: $presentation" >&2
    exit 2
  fi

  name=$(basename "$presentation" .pin)
  slide_count=$(grep -c '^-\|^--' "$presentation" || true)
  transition_count=$(grep -o '\[transition=[^]]*\]' "$presentation" |
    sort | uniq -c | awk '{$1=$1; print}')
  asset_count=$(grep -Eo '\[[^]=[:space:]]+\.(png|jpe?g|gif|svg|mp4|webm|ogv)\]' "$presentation" |
    wc -l | tr -d ' ')

  printf 'REALDECK INVENTORY name=%s slides=%s assets=%s\n' \
    "$name" "$slide_count" "$asset_count"
  if [ -n "$transition_count" ]; then
    printf '%s\n' "$transition_count" | while read -r count transition; do
      printf 'REALDECK TRANSITIONS name=%s count=%s type=%s\n' \
        "$name" "$count" "$transition"
    done
  else
    printf 'REALDECK TRANSITIONS name=%s count=0 type=none\n' "$name"
  fi

  "$c_binary" --check "$presentation"
  flatpak run --user --filesystem="$root" "$app_id" --check "$presentation"

  transition_output=$(flatpak run --user --device=dri \
    --socket=wayland --socket=fallback-x11 --filesystem="$root" \
    "$app_id" --validate-transitions "$presentation" 2>&1)
  printf '%s\n' "$transition_output"
  printf '%s\n' "$transition_output" | grep '^PINPOINT TRANSITION PASS '

  stage_output=$(flatpak run --user --device=dri \
    --socket=wayland --socket=fallback-x11 --filesystem="$root" \
    "$app_id" --validate-stage "$presentation" 2>&1)
  printf '%s\n' "$stage_output"
  printf '%s\n' "$stage_output" | grep '^PINPOINT STAGE PASS '
  if printf '%s\n%s\n' "$transition_output" "$stage_output" | grep -q \
      'Trying to snapshot GtkGraphicsOffload .* without a current allocation'; then
    printf 'REALDECK FOLLOWUP name=%s issue=gtk-graphics-offload-current-allocation\n' "$name"
  fi
done
