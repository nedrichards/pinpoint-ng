#!/bin/sh
set -eu

app_id=com.nedrichards.pinpoint
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
run_tmp=$(mktemp -d -t pinpoint-setup-XXXXXX)
trap 'rm -rf "$run_tmp"' EXIT HUP INT TERM

output=$(flatpak run --user "$app_id" --validate-setup 2>&1)
printf '%s\n' "$output"
printf '%s\n' "$output" | grep -q \
  '^PINPOINT SETUP PASS presentation_files=2 sorted=true ignored_files=1 ignored_directories=1 preflight_parse=valid-and-invalid summary=parser-derived comments=refresh explicit_cli=authoritative normal_launch=reopens_nothing session_restore=deferred-gtk-4.24$'

flatpak run --user "$app_id" --help >"$run_tmp/help"
grep -q '^--check ' "$run_tmp/help"

flatpak run --user --filesystem="$root":ro "$app_id" --check \
  "$root/tests/fixtures/stage-smoke.pin" >"$run_tmp/check.out"
grep -q ': 4 slides$' "$run_tmp/check.out"

if flatpak run --user --filesystem="$root":ro "$app_id" \
  "$root/tests/fixtures/stage-smoke.pin" \
  "$root/tests/fixtures/pixel-reference.pin" \
  >"$run_tmp/multiple.out" 2>"$run_tmp/multiple.err"
then
  echo 'PINPOINT SETUP FAIL multiple presentations were accepted' >&2
  exit 1
fi
grep -q 'exactly one presentation may be supplied' "$run_tmp/multiple.err"

permissions=$(flatpak info --user --show-permissions "$app_id")
filesystems=$(printf '%s\n' "$permissions" | sed -n 's/^filesystems=//p')
case ";$filesystems" in
  *';home'*|*';host'*)
    echo 'PINPOINT SETUP FAIL broad host filesystem permission is present' >&2
    exit 1
    ;;
esac
printf '%s\n' "$permissions" | grep -q '^shared=ipc;'
printf '%s\n' "$permissions" | grep -q '^devices=dri;'

schema_value=$(flatpak run --user --command=gsettings "$app_id" \
  get "$app_id" welcome-complete)
case $schema_value in
  true|false) ;;
  *)
    echo "PINPOINT SETUP FAIL unreadable settings schema: $schema_value" >&2
    exit 1
    ;;
esac

echo 'PINPOINT SETUP INSTALLED PASS cli=checked settings=available filesystem=portal-only metadata=installed'
