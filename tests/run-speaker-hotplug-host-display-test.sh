#!/bin/sh
set -eu

app_id=com.nedrichards.pinpoint
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
audience_suffix='— Pinpoint Audience'
speaker_title='Pinpoint Speaker View'
phase_file=${PINPOINT_HOTPLUG_PHASE_FILE:-/tmp/pinpoint-hotplug-phase}
state_file="$phase_file.state"

shell_eval ()
{
  reply=$(gdbus call --session \
    --dest org.gnome.Shell \
    --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval "$1")
  case "$reply" in
    "(true, '"*"')")
      value=${reply#"(true, '"}
      value=${value%"')"}
      printf '%b\n' "$value" | jq -r .
      ;;
    *) return 1 ;;
  esac
}

window_state ()
{
  shell_eval '
    JSON.stringify({
      monitors: global.display.get_n_monitors(),
      windows: global.get_window_actors()
        .map(actor => actor.get_meta_window())
        .filter(window =>
          window.get_title() === "Pinpoint Speaker View" ||
          window.get_title().endsWith("— Pinpoint Audience"))
        .map(window => ({
          title: window.get_title(),
          monitor: window.get_monitor(),
          fullscreen: window.is_fullscreen()
        }))
    })'
}

wait_for_condition ()
{
  expression=$1
  attempts=0
  while [ "$attempts" -lt 900 ]
  do
    state=$(window_state || printf '{}')
    printf '%s\n' "$state" > "$state_file"
    if printf '%s' "$state" | jq -e "$expression" >/dev/null
    then
      printf '%s\n' "$state"
      return 0
    fi
    attempts=$((attempts + 1))
    sleep 0.1
  done
  return 1
}

write_phase ()
{
  printf '%s\n' "$1" > "$phase_file"
  echo "Pinpoint hotplug host test: $1" >&2
}

if ! flatpak info --user "$app_id" >/dev/null 2>&1
then
  echo "Install the Pinpoint Flatpak before running the host display test." >&2
  exit 1
fi
if ! shell_eval 'String(1 + 1)' >/dev/null
then
  echo "GNOME Shell Eval is disabled." >&2
  echo "Open Looking Glass (Alt+F2, then lg) and run:" >&2
  echo "  global.context.unsafe_mode = true" >&2
  exit 77
fi
if [ "$(shell_eval 'String(global.display.get_n_monitors())')" -lt 2 ]
then
  echo "Connect the second display before starting the hotplug test." >&2
  exit 77
fi

run_tmp=$(mktemp -d -t pinpoint-hotplug-XXXXXX)
pinpoint_pid=
cleanup ()
{
  rm -f -- "$phase_file" "$state_file"
  flatpak kill --user "$app_id" >/dev/null 2>&1 || true
  if [ -n "$pinpoint_pid" ] && kill -0 "$pinpoint_pid" 2>/dev/null
  then
    kill "$pinpoint_pid" 2>/dev/null || true
    wait "$pinpoint_pid" 2>/dev/null || true
  fi
  rm -f -- "$run_tmp/stdout" "$run_tmp/stderr"
  rmdir -- "$run_tmp"
}
trap cleanup EXIT HUP INT TERM

flatpak run --user --filesystem="$root":ro "$app_id" \
  --speakermode --fullscreen \
  "$root/tests/fixtures/speaker-validation.pin" \
  >"$run_tmp/stdout" 2>"$run_tmp/stderr" &
pinpoint_pid=$!

write_phase waiting-for-initial-state
initial=$(wait_for_condition '
  .monitors >= 2 and
  (.windows | length == 2) and
  (.windows | all(.[]; .fullscreen == true)) and
  ([.windows[].monitor] | unique | length) == 2') || {
    echo "Pinpoint did not expose distinct fullscreen presentation windows." >&2
    cat "$run_tmp/stderr" >&2
    exit 1
  }
write_phase ready-to-unplug

disconnected=$(wait_for_condition '
  .monitors == 1 and
  (.windows | length == 2) and
  (.windows[] | select(.title | endswith("— Pinpoint Audience")) | .fullscreen) and
  ((.windows[] | select(.title == "Pinpoint Speaker View") | .fullscreen) == false)') || {
    echo "Pinpoint did not keep the audience fullscreen after monitor removal." >&2
    echo "Initial compositor state:" >&2
    printf '%s\n' "$initial" >&2
    echo "Final compositor state:" >&2
    window_state >&2 || true
    cat "$run_tmp/stderr" >&2
    exit 1
  }
write_phase ready-to-replug

restored=$(wait_for_condition '
  .monitors >= 2 and
  (.windows | length == 2) and
  (.windows | all(.[]; .fullscreen == true)) and
  ([.windows[].monitor] | unique | length) == 2') || {
    echo "Pinpoint did not restore distinct fullscreen presentation windows." >&2
    echo "Removed-monitor compositor state:" >&2
    printf '%s\n' "$disconnected" >&2
    echo "Final compositor state:" >&2
    window_state >&2 || true
    cat "$run_tmp/stderr" >&2
    exit 1
  }

printf '%s\n' "$restored" | jq .
echo "Pinpoint hotplug host automation passed: audience continuity and two-display restoration."
