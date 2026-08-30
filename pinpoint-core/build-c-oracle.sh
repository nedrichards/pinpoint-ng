#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
c_root=${PINPOINT_C_SOURCE:-"$project_root/../fun-pinpoint"}

if [ ! -f "$c_root/_build/src/libpinpoint-core.a" ]; then
  echo "build the C implementation at $c_root before generating the oracle" >&2
  exit 2
fi

cc \
  -std=c17 -O2 -Wall -Wextra -Werror \
  -I"$c_root/src" -I"$c_root/_build" -I"$c_root/_build/src" \
  "$project_root/pinpoint-core/c-oracle.c" \
  "$c_root/src/pp-transition.c" \
  "$c_root/_build/src/libpinpoint-core.a" \
  $(pkg-config --cflags --libs gtk4 gio-unix-2.0 json-glib-1.0) \
  -lm \
  -o "$project_root/pinpoint-core/pinpoint-c-oracle"
