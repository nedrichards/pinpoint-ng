#!/bin/sh
set -eu

cargo_bin=$1
source_root=$2
profile=$3
output=$4
case "$output" in
  /*) ;;
  *) output="$PWD/$output" ;;
esac

# The caller supplies the same Meson-owned context used by every check.
: "${CARGO_TARGET_DIR:?Meson must supply CARGO_TARGET_DIR}"
cd "$source_root"
"$cargo_bin" build --locked --profile "$profile" -p pinpoint
case "$profile" in
  dev) artifact_profile=debug ;;
  *) artifact_profile=$profile ;;
esac
install -m 0755 "$CARGO_TARGET_DIR/$artifact_profile/pinpoint" "$output"
