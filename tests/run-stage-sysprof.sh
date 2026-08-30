#!/bin/sh

set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 OUTPUT.syscap [PRESENTATION]" >&2
  exit 2
fi

output=$1
presentation=${2:-/app/share/pinpoint/introduction/introduction.pin}
app_id=com.nedrichards.pinpoint
profiler_pid=
keepalive_pid_file=$(mktemp)

stop_profiler ()
{
  if [ -s "$keepalive_pid_file" ]; then
    keepalive_pid=$(sed -n '1p' "$keepalive_pid_file")
    if kill -0 "$keepalive_pid" 2>/dev/null; then
      kill -TERM "$keepalive_pid"
    fi
  fi
  if [ -n "$profiler_pid" ] && kill -0 "$profiler_pid" 2>/dev/null; then
    wait "$profiler_pid" || true
  fi
  rm -f "$keepalive_pid_file"
}

mkdir -p "$(dirname -- "$output")"
if [ -e "$output" ]; then
  echo "refusing to overwrite existing capture: $output" >&2
  rm -f "$keepalive_pid_file"
  exit 1
fi

if ! command -v eglinfo >/dev/null 2>&1 || ! command -v glxinfo >/dev/null 2>&1; then
  echo "PINPOINT ADVISORY Sysprof may report missing optional eglinfo/glxinfo helpers" >&2
fi

trap stop_profiler EXIT INT TERM
sysprof-cli --force "$output" \
  --buffer-size=16384 --no-disk --no-network --gnome-shell --rapl \
  --scheduler --speedtrack --power-profile=balanced --no-debuginfod \
  --no-decode \
  -- /bin/sh -c 'trap "exit 0" TERM INT; echo "$$" > "$1"; while :; do /usr/bin/sleep 1; done' sh \
  "$keepalive_pid_file" &
profiler_pid=$!

sleep 1
flatpak run --user --device=dri --socket=wayland --socket=fallback-x11 \
  "$app_id" --validate-stage "$presentation"

stop_profiler
profiler_pid=
trap - EXIT INT TERM
