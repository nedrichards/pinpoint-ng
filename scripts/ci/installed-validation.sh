#!/usr/bin/env bash
set -euo pipefail
app="$APP_ID"
args=(--user --socket=x11 --nosocket=wayland --env=GDK_BACKEND=x11 --env=GSK_RENDERER=cairo --env=GSETTINGS_BACKEND=memory --filesystem="$PWD:ro" --filesystem="$PWD/.ci-results")
cat > .ci-results/fixture.pin <<'PIN'
[stage-color=#101820]
[font=Sans 36px]
[text-color=white]
-- [#3050d0] [center]
Invented native CI fixture
-- [#c03040] [center]
Second native slide
PIN
flatpak run "${args[@]}" "$app" --version
flatpak run "${args[@]}" "$app" --check "$PWD/.ci-results/fixture.pin"
flatpak run "${args[@]}" "$app" --output "$PWD/.ci-results/fixture.pdf" "$PWD/.ci-results/fixture.pin"
python3 -c 'from pathlib import Path; data=Path(".ci-results/fixture.pdf").read_bytes(); assert data.startswith(b"%PDF-") and len(data)>1000'
flatpak run "${args[@]}" "$app" --validate-stage "$PWD/.ci-results/fixture.pin"
flatpak run "${args[@]}" "$app" --validate-pixels "$PWD/.ci-results/fixture.pin"
