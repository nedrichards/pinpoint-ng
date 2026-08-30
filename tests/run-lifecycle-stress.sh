#!/bin/sh
set -eu

flatpak run --user \
  --env=G_DEBUG=fatal-warnings \
  --env=G_SLICE=always-malloc \
  com.nedrichards.pinpoint \
  --validate-lifecycle-stress
