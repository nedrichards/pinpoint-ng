#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
c_root=${PINPOINT_C_SOURCE:-"$root/../fun-pinpoint"}
capture_dir=$(mktemp -d /tmp/pinpoint-curl-diff.XXXXXX)
trap 'rm -rf "$capture_dir"' EXIT HUP INT TERM

if [ ! -f "$c_root/_build/src/libpinpoint-editor.so" ] ||
   [ ! -f "$c_root/_build/src/libpinpoint-core.a" ]; then
  echo "build the C implementation in _build before running the curl diff" >&2
  exit 2
fi

cd "$root"

flatpak run --user --filesystem="$root" --filesystem="$capture_dir" \
  --command=sh org.gnome.Sdk//50 -c '
    cc -std=c17 -Wall -Wextra -Werror \
      -I "$1/_build" -I "$1/src" \
      "$2/tests/capture-c-page-curl.c" \
      -L "$1/_build/src" -Wl,-rpath,"$1/_build/src" \
      -lpinpoint-editor "$1/_build/src/libpinpoint-core.a" \
      $(pkg-config --cflags --libs gtk4 epoxy) -lm \
      -o "$3/capture-c-page-curl"
    cc -std=c17 -Wall -Wextra -Werror \
      "$2/tests/compare-page-curl-pixels.c" \
      $(pkg-config --cflags --libs gtk4) \
      -o "$3/compare-page-curl-pixels"
  ' sh "$c_root" "$root" "$capture_dir"

for scale in 1 125 200; do
  case "$scale" in
    1) width=640 height=360 ;;
    125) width=800 height=450 ;;
    200) width=1280 height=720 ;;
  esac
  for direction in forward backward; do
    c_capture="$capture_dir/c-$direction-$scale.png"
    rust_capture="$capture_dir/rust-$direction-$scale.png"
    flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
      --filesystem="$root" --filesystem="$capture_dir" --command=sh \
      org.gnome.Sdk//50 -c \
      'LD_LIBRARY_PATH="$1/_build/src" "$2/capture-c-page-curl" "$3" "$4" "$5" "$6"' \
      sh "$c_root" "$capture_dir" "$width" "$height" "$direction" "$c_capture"
    rust_args="--capture-page-curl=$rust_capture --capture-size=${width}x${height}"
    if [ "$direction" = backward ]; then
      rust_args="$rust_args --capture-backward"
    fi
    flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
      --filesystem="$root" --filesystem="$capture_dir" \
      --env=PATH=/usr/lib/sdk/rust-stable/bin:/usr/bin --command=sh \
      org.gnome.Sdk//50 -c \
      'root="$1"
       shift
       cd "$root"
       cargo run --offline -p pinpoint -- "$@"' \
      sh "$root" $rust_args
    flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
      --filesystem="$capture_dir" --command="$capture_dir/compare-page-curl-pixels" \
      org.gnome.Sdk//50 "$c_capture" "$rust_capture"
    printf 'PINPOINT CURL DIFF PASS scale=%s direction=%s\n' "$scale" "$direction"
  done
done
