#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
c_root=${PINPOINT_C_SOURCE:-"$root/../fun-pinpoint"}
app_id=com.nedrichards.pinpoint
capture_dir=$(mktemp -d /tmp/pinpoint-real-deck-capture.XXXXXX)
if [ "${KEEP_CAPTURE_ARTIFACTS:-0}" = 1 ]; then
  trap 'printf "REALDECK CAPTURE ARTIFACTS directory=%s\n" "$capture_dir"' EXIT HUP INT TERM
else
  trap 'rm -rf "$capture_dir"' EXIT HUP INT TERM
fi

if [ "$#" -eq 0 ]; then
  set -- \
    "$root/guadec-2018/product-management-open-source.pin" \
    "$root/desktop-summit/desktop-summit.pin"
fi

if [ ! -x "$c_root/_build-codex/src/pinpoint" ] && [ ! -x "$c_root/_build/src/pinpoint" ]; then
  echo "build the C implementation before running real-deck captures" >&2
  exit 2
fi

c_build="$c_root/_build-codex"
if [ ! -d "$c_build/src" ]; then
  c_build="$c_root/_build"
fi
overall_status=0

flatpak run --user --filesystem="$root" --filesystem="$capture_dir" \
  --command=sh org.gnome.Sdk//50 -c '
    cc -std=c17 -Wall -Wextra -Werror \
      -I "$1/_build-codex" -I "$1/src" \
      "$2/tests/capture-c-real-slide.c" \
      -L "$3/src" -Wl,-rpath,"$3/src" \
      -lpinpoint-editor "$3/src/libpinpoint-core.a" \
      $(pkg-config --cflags --libs gtk4 gio-2.0 gstreamer-1.0) -lm \
      -o "$4/capture-c-real-slide"
    cc -std=c17 -Wall -Wextra -Werror \
      "$2/tests/compare-page-curl-pixels.c" \
      $(pkg-config --cflags --libs gtk4) \
      -o "$4/compare-real-deck-pixels"
  ' sh "$c_root" "$root" "$c_build" "$capture_dir"

for presentation in "$@"; do
  name=$(basename "$presentation" .pin)
  indices=$(awk '
    BEGIN { slide = 0 }
    /^\[transition=/ { print slide }
    /^-/ {
      slide++
      if ($0 ~ /\[transition=/) print slide
    }
  ' "$presentation" | sort -nu)
  for slide in $indices; do
    c_capture="$capture_dir/$name-slide-$slide-c.png"
    rust_capture="$capture_dir/$name-slide-$slide-rust.png"
    slide_asset=$(awk -v target="$slide" '
      BEGIN { current = 0 }
      /^-/ {
        if (current == target) {
          for (field = 1; field <= NF; field++) {
            asset = $field
            gsub(/^\[/, "", asset)
            gsub(/\]$/, "", asset)
            if (asset !~ /=/ && asset ~ /\.(png|jpe?g|gif|svg)$/) {
              print asset
              exit
            }
          }
        }
        current++
      }
    ' "$presentation")
    icc_asset=""
    if [ -n "$slide_asset" ] && [ -f "$(dirname "$presentation")/$slide_asset" ] &&
       grep -a -q 'ICC_PROFILE' "$(dirname "$presentation")/$slide_asset"; then
      icc_asset="$slide_asset"
    fi
    flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
      --filesystem="$root" --filesystem="$capture_dir" --command=sh \
      org.gnome.Sdk//50 -c \
      'LD_LIBRARY_PATH="$1/src" "$2/capture-c-real-slide" "$3" "$4" "$5"' \
      sh "$c_build" "$capture_dir" "$presentation" "$slide" "$c_capture"
    rust_output=$(flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
      --filesystem="$root" --filesystem="$capture_dir" \
      "$app_id" --capture-real-slide="$rust_capture" --capture-slide="$slide" \
      --capture-size=1280x720 "$presentation" 2>&1)
    printf '%s\n' "$rust_output"
    printf '%s\n' "$rust_output" | grep '^PINPOINT REAL CAPTURE PASS '
    set +e
    diff_output=$(flatpak run --user --filesystem="$capture_dir" \
      --command="$capture_dir/compare-real-deck-pixels" org.gnome.Sdk//50 \
      "$c_capture" "$rust_capture" 2>&1)
    diff_status=$?
    set -e
    printf '%s\n' "$diff_output"
    printf '%s\n' "$diff_output" | grep '^CURL DIFF '
    if [ "$diff_status" -eq 0 ]; then
      printf 'REALDECK DIFF PASS deck=%s slide=%s\n' "$name" "$slide"
    elif [ -n "$icc_asset" ]; then
      printf 'REALDECK DIFF ICC-EXPECTED deck=%s slide=%s asset=%s\n' \
        "$name" "$slide" "$icc_asset"
    else
      printf 'REALDECK DIFF FAIL deck=%s slide=%s\n' "$name" "$slide"
      overall_status=1
    fi
    printf 'REALDECK CAPTURE PASS deck=%s slide=%s c=%s rust=%s\n' \
      "$name" "$slide" "$c_capture" "$rust_capture"
  done
done

printf 'REALDECK CAPTURE ARTIFACTS directory=%s\n' "$capture_dir"
exit "$overall_status"
