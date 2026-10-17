#!/usr/bin/env bash
# Build and install from the normal manifest, retaining only reusable Builder state.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
case "${1:-production}" in
  production) manifest=com.nedrichards.pinpoint.json; app=com.nedrichards.pinpoint; remote=pinpoint-origin ;;
  development) manifest=com.nedrichards.pinpoint.Devel.json; app=com.nedrichards.pinpoint.Devel; remote=pinpoint-ng-devel-local ;;
  -h|--help) echo "Usage: $0 [production|development]"; exit 0 ;;
  *) echo "Usage: $0 [production|development]" >&2; exit 2 ;;
esac
if (( $# > 1 )); then
  echo "Usage: $0 [production|development]" >&2
  exit 2
fi
state="$root/.flatpak-builder"
staging="$state/staging"
repo="$root/flatpak-repo"
mkdir -p "$state"
exec 9>"$state/local-build.lock"
flock -n 9 || { echo "Another local Flatpak build is running in $root" >&2; exit 1; }
# Builder removes module directories normally; also clean interrupted builds and
# staging on exit. Keep downloads, cache, logs and the separate export repository.
cleanup() {
  local status=$?
  trap - EXIT
  rm -rf -- "$state/build" "$staging" || { (( status != 0 )) || status=1; }
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
flatpak-builder --user --disable-rofiles-fuse --force-clean \
  --delete-build-dirs --install-deps-from=flathub --default-branch=master \
  --state-dir="$state" --repo="$repo" "$staging" "$manifest" \
  > >(tee "$state/local-build.log") 2>&1
# Only change the update source after the build, tests and export succeed.
flatpak remote-add --user --if-not-exists --no-gpg-verify "$remote" "$repo"
flatpak remote-modify --user --no-gpg-verify --url="file://$repo" "$remote"
# --or-update cannot switch an existing app to a different origin. Reinstall
# only that ref when migrating; retain application data without repointing
# another application's remote.
install_args=(--user --noninteractive --or-update)
installed_origin=$(flatpak info --user --show-origin "$app//master" 2>/dev/null || true)
if [[ -n "$installed_origin" && "$installed_origin" != "$remote" ]]; then
  install_args+=(--reinstall)
fi
flatpak install "${install_args[@]}" "$remote" "$app//master"
echo "Installed $app from $repo. Quit and reopen the app to use this build."
