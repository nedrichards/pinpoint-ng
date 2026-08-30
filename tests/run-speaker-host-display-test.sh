#!/bin/sh
set -eu

app_id=com.nedrichards.pinpoint
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
audience_suffix='— Pinpoint Audience'
speaker_title='Pinpoint Speaker View'

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
    JSON.stringify(global.get_window_actors()
      .map(actor => actor.get_meta_window())
      .filter(window =>
        window.get_title() === "Pinpoint Speaker View" ||
        window.get_title().endsWith("— Pinpoint Audience"))
      .map(window => ({
        title: window.get_title(),
        monitor: window.get_monitor(),
        fullscreen: window.is_fullscreen()
      })))'
}

wait_for_condition ()
{
  expression=$1
  attempts=0
  while [ "$attempts" -lt 100 ]
  do
    state=$(window_state || printf '[]')
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

focus_speaker ()
{
  shell_eval '
    const window = global.get_window_actors()
      .map(actor => actor.get_meta_window())
      .find(candidate =>
        candidate.get_title() === "Pinpoint Speaker View");
    if (!window)
      throw new Error("speaker window not found");
    window.activate(global.get_current_time());
    "focused"' >/dev/null
}

send_key ()
{
  key=$1
  shell_eval "
    const Clutter = imports.gi.Clutter;
    const GLib = imports.gi.GLib;
    const device = Clutter.get_default_backend()
      .get_default_seat()
      .create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
    const time = GLib.get_monotonic_time();
    device.notify_keyval(time, Clutter.KEY_${key}, Clutter.KeyState.PRESSED);
    device.notify_keyval(time + 1000,
                         Clutter.KEY_${key},
                         Clutter.KeyState.RELEASED);
    'sent'" >/dev/null
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
monitor_count=$(shell_eval 'String(global.display.get_n_monitors())')
if [ "$monitor_count" -lt 2 ]
then
  echo "Pinpoint host display automation requires two connected monitors." >&2
  exit 77
fi

run_tmp=$(mktemp -d -t pinpoint-display-XXXXXX)
pinpoint_pid=
cleanup ()
{
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

initial=$(wait_for_condition '
  length == 2 and
  all(.[]; .fullscreen == true) and
  ([.[].monitor] | unique | length) == 2') || {
    echo "Pinpoint did not expose distinct fullscreen presentation windows." >&2
    cat "$run_tmp/stderr" >&2
    exit 1
  }
audience_before=$(printf '%s' "$initial" | jq -r ".[] | select(.title | endswith(\"$audience_suffix\")) | .monitor")
speaker_before=$(printf '%s' "$initial" | jq -r ".[] | select(.title == \"$speaker_title\") | .monitor")

focus_speaker
send_key s
if ! wait_for_condition \
  "(.[] | select(.title | endswith(\"$audience_suffix\")) | .monitor) == $speaker_before and (.[] | select(.title == \"$speaker_title\") | .monitor) == $audience_before" \
  >/dev/null
then
    echo "Pinpoint did not swap its two display assignments." >&2
    echo "Initial compositor state:" >&2
    printf '%s\n' "$initial" >&2
    echo "Compositor state after S:" >&2
    window_state >&2 || true
    cat "$run_tmp/stderr" >&2
    exit 1
fi

focus_speaker
send_key F11
wait_for_condition 'all(.[]; .fullscreen == false)' >/dev/null || {
  echo "F11 did not leave fullscreen on both Pinpoint windows." >&2
  exit 1
}
focus_speaker
send_key F11
final=$(wait_for_condition '
  all(.[]; .fullscreen == true) and
  ([.[].monitor] | unique | length) == 2') || {
    echo "F11 did not restore two-display Pinpoint fullscreen." >&2
    exit 1
  }

printf '%s\n' "$final" | jq .
echo "Pinpoint host display automation passed: distinct fullscreen windows, display swap, and fullscreen restore."
