#!/bin/sh

set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 OUTPUT.syscap [SLIDES]" >&2
  exit 2
fi

capture=$1
requested_slides=${2:-}
app_id=com.nedrichards.pinpoint
profiler_pid=
run_tmp=$(mktemp -d -t pinpoint-pdf-sysprof-XXXXXX)
deck=$run_tmp/profile.pin
pdf=$run_tmp/profile.pdf
export_log=$run_tmp/export.log
export_status_file=$run_tmp/export.status
profiled_command_pid_file=$run_tmp/profiled-command.pid

cp data/introduction/bg.jpg "$run_tmp/sample.jpg"
printf '%s\n' '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="450"><rect width="800" height="450" fill="#3584e4"/></svg>' >"$run_tmp/sample.svg"

stop_profiler ()
{
  if [ -s "$profiled_command_pid_file" ]; then
    profiled_command_pid=$(sed -n '1p' "$profiled_command_pid_file")
    if kill -0 "$profiled_command_pid" 2>/dev/null; then
      kill -TERM "$profiled_command_pid" 2>/dev/null || true
    fi
  fi
  if [ -n "$profiler_pid" ] && kill -0 "$profiler_pid" 2>/dev/null; then
    attempts=0
    while kill -0 "$profiler_pid" 2>/dev/null && [ "$attempts" -lt 20 ]; do
      sleep 0.25
      attempts=$((attempts + 1))
    done
    for forced_interrupt in 1 2 3; do
      if kill -0 "$profiler_pid" 2>/dev/null; then
        kill -INT "$profiler_pid" 2>/dev/null || true
        sleep 0.5
      fi
    done
    if kill -0 "$profiler_pid" 2>/dev/null; then
      kill -TERM "$profiler_pid" 2>/dev/null || true
      sleep 0.5
    fi
    if kill -0 "$profiler_pid" 2>/dev/null; then
      kill -KILL "$profiler_pid" 2>/dev/null || true
    fi
  fi
  if [ -n "$profiler_pid" ]; then
    wait "$profiler_pid" || true
  fi
}

cleanup ()
{
  stop_profiler
  rm -rf "$run_tmp"
}

trap cleanup EXIT INT TERM

mkdir -p "$(dirname -- "$capture")"
if [ -e "$capture" ]; then
  echo "refusing to overwrite existing capture: $capture" >&2
  exit 1
fi

if [ -n "$requested_slides" ]; then
  case $requested_slides in
    *[!0-9]*|'')
      echo "SLIDES must be a positive integer" >&2
      exit 2
      ;;
  esac
  if [ "$requested_slides" -lt 1 ]; then
    echo "SLIDES must be a positive integer" >&2
    exit 2
  fi
  if [ "$requested_slides" -gt 500 ]; then
    echo "SLIDES must not exceed the 500-slide real-world ceiling" >&2
    exit 2
  fi
  slides=$requested_slides
else
  mem_mib=$(awk '/MemAvailable:/ { print int($2 / 1024) }' /proc/meminfo)
  cpu_count=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)
  memory_slides=$((mem_mib / 64))
  cpu_slides=$((cpu_count * 48))
  slides=$memory_slides
  test "$slides" -le "$cpu_slides" || slides=$cpu_slides
  test "$slides" -ge 240 || slides=240
  test "$slides" -le 500 || slides=500
fi

awk -v slides="$slides" 'BEGIN {
  print "[stage-color=#182030]"
  print "[text-color=white]"
  print "[font=Sans 42px]"
  for (i = 1; i <= slides; i++) {
    asset = i % 2 ? "sample.jpg" : "sample.svg"
    printf "-- [%s] [fit]\nPDF Sysprof slide %d of %d\n", asset, i, slides
    if (i % 4 == 0)
      printf "#Speaker notes exercise wrapped Pango output on slide %d.\n", i
  }
}' >"$deck"

if ! command -v eglinfo >/dev/null 2>&1 || ! command -v glxinfo >/dev/null 2>&1; then
  echo "PINPOINT ADVISORY Sysprof may report missing optional eglinfo/glxinfo helpers" >&2
fi

echo "PINPOINT PDF SYSPROF starting slides=$slides capture=$capture" >&2
sysprof-cli --force "$capture" \
  --buffer-size=32768 --no-network --rapl --scheduler --speedtrack \
  --power-profile=performance --no-debuginfod --no-decode \
  -- /bin/sh -c '
    printf "%s\n" "$$" >"$8"
    /usr/bin/time -v flatpak run --user --filesystem="$1" "$2" \
      --output="$3" "$4" >"$5" 2>"$6"
    export_status=$?
    printf "%s\n" "$export_status" >"$7"
    trap "exit 0" TERM INT
    while :; do /usr/bin/sleep 1; done
  ' sh "$run_tmp" "$app_id" "$pdf" "$deck" "$run_tmp/export.out" "$export_log" \
  "$export_status_file" "$profiled_command_pid_file" &
profiler_pid=$!
started=$(date +%s)
heartbeat_at=5
while [ ! -s "$export_status_file" ]; do
  if ! kill -0 "$profiler_pid" 2>/dev/null; then
    echo "PINPOINT PDF SYSPROF FAIL profiler exited before export completed" >&2
    wait "$profiler_pid" || true
    profiler_pid=
    exit 1
  fi
  sleep 1
  elapsed_now=$(($(date +%s) - started))
  if [ "$elapsed_now" -ge "$heartbeat_at" ]; then
    elapsed_now=$(($(date +%s) - started))
    working_kib=$(du -sk "$run_tmp" | awk '{ print $1 }')
    echo "PINPOINT PDF SYSPROF running elapsed_s=$elapsed_now working_kib=$working_kib" >&2
    heartbeat_at=$((heartbeat_at + 5))
  fi
done
export_status=$(sed -n '1p' "$export_status_file")
stop_profiler
profiler_pid=
if [ "$export_status" -ne 0 ]; then
  echo "PINPOINT PDF SYSPROF FAIL exporter status=$export_status" >&2
  tail -20 "$export_log" >&2
  exit "$export_status"
fi
test -s "$capture"
grep -q "PINPOINT PDF PASS slides=$slides" "$export_log"
rss_kib=$(awk -F: '/Maximum resident set size/ { gsub(/^[[:space:]]+/, "", $2); print $2 }' "$export_log")
elapsed=$(awk '/Elapsed \(wall clock\) time/ { sub(/^.*\): /, ""); print }' "$export_log")
pdf_bytes=$(stat -c %s "$pdf")

trap - EXIT INT TERM
rm -rf "$run_tmp"
echo "PINPOINT PDF SYSPROF PASS slides=$slides rss_mib=$(awk -v kib="$rss_kib" 'BEGIN { printf "%.2f", kib / 1024 }') elapsed=$elapsed pdf_bytes=$pdf_bytes capture=$capture"
