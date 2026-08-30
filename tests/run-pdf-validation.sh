#!/bin/sh
set -eu

app_id=com.nedrichards.pinpoint
run_tmp=$(mktemp -d -t pinpoint-pdf-XXXXXX)
trap 'rm -rf "$run_tmp"' EXIT HUP TERM

deck=$run_tmp/fidelity.pin
output=$run_tmp/fidelity.pdf
log=$run_tmp/fidelity.log
cp data/introduction/bg.jpg "$run_tmp/sample.jpg"
cp tests/fixtures/svg-quality.svg "$run_tmp/sample.svg"
cp tests/fixtures/media-formats/pinpoint-h264-high.mp4 "$run_tmp/sample.mp4"
cat >"$deck" <<'EOF'
[stage-color=#182030]
[text-color=white]
-- [sample.jpg] [fill]
JPEG first use
#A separate speaker-note page
-- [sample.svg] [fit]
SVG at PDF resolution
-- [sample.jpg] [fill]
JPEG revisited
-- [sample.mp4] [fit]
Representative video thumbnail
EOF

printf 'existing PDF destination' >"$output"
flatpak run --user --filesystem="$run_tmp" "$app_id" \
  --output="$output" "$deck" 2>"$log"
grep -q '^%PDF-' "$output"
grep -q 'PINPOINT PDF PASS slides=4 pages=5 jpeg_uses=2 svg_uses=1 video_thumbnails=1 video_failures=0' "$log"
dct_count=$(LC_ALL=C grep -a -o '/DCTDecode' "$output" | wc -l)
image_count=$(LC_ALL=C grep -a -o '/Subtype /Image' "$output" | wc -l)
test "$dct_count" -eq 1
test "$image_count" -eq 2

if flatpak run --user --filesystem="$run_tmp" "$app_id" \
  --output="$deck" "$deck" >"$run_tmp/source.out" 2>"$run_tmp/source.err"
then
  echo 'PINPOINT PDF FAIL source destination was accepted' >&2
  exit 1
fi
grep -q 'PDF output must not replace the presentation source' "$run_tmp/source.err"
grep -q 'JPEG first use' "$deck"

mkdir "$run_tmp/real"
cp "$deck" "$run_tmp/real/alias.pin"
ln -s real "$run_tmp/link"
if flatpak run --user --filesystem="$run_tmp" "$app_id" \
  --output="$run_tmp/real/alias.pin" "$run_tmp/link/alias.pin" \
  >"$run_tmp/alias.out" 2>"$run_tmp/alias.err"
then
  echo 'PINPOINT PDF FAIL source alias was accepted' >&2
  exit 1
fi
grep -q 'PDF output must not replace the presentation source' "$run_tmp/alias.err"

mkdir "$run_tmp/unwritable"
chmod 500 "$run_tmp/unwritable"
if flatpak run --user --filesystem="$run_tmp" "$app_id" \
  --output="$run_tmp/unwritable/export.pdf" "$deck" \
  >"$run_tmp/unwritable.out" 2>"$run_tmp/unwritable.err"
then
  echo 'PINPOINT PDF FAIL unwritable output was accepted' >&2
  exit 1
fi
grep -q 'cannot create temporary PDF' "$run_tmp/unwritable.err"
chmod 700 "$run_tmp/unwritable"

mem_mib=$(awk '/MemAvailable:/ { print int($2 / 1024) }' /proc/meminfo)
# Stay at the parser's public slide limit and signal only after the temporary
# PDF exists, so cancellation does not depend on the machine's export speed.
cancel_slides=1024
awk -v slides="$cancel_slides" 'BEGIN {
  for (i = 1; i <= slides; i++)
    printf "-- [white] [text-color=black]\nCancellation slide %d\n", i
}' >"$run_tmp/cancel.pin"
for signal_case in INT:130 TERM:143
do
  signal_name=${signal_case%:*}
  expected_status=${signal_case#*:}
  protected="$run_tmp/protected-$signal_name.pdf"
  printf 'existing destination' >"$protected"
  set +e
  flatpak run --user --filesystem="$run_tmp" --command=sh "$app_id" -c '
    pinpoint --output="$1" "$2" &
    export_pid=$!
    while kill -0 "$export_pid" 2>/dev/null; do
      for temporary in "${1%/*}"/.pinpoint-pdf-*; do
        if [ -e "$temporary" ]; then
          kill -"$3" "$export_pid"
          wait "$export_pid"
          exit $?
        fi
      done
    done
    wait "$export_pid"
  ' sh "$protected" "$run_tmp/cancel.pin" "$signal_name" \
    >"$run_tmp/cancel-$signal_name.out" 2>"$run_tmp/cancel-$signal_name.err"
  cancel_status=$?
  set -e
  test "$cancel_status" -eq "$expected_status"
  test "$(cat "$protected")" = 'existing destination'
  test -z "$(find "$run_tmp" -maxdepth 1 -name '.pinpoint-pdf-*' -print -quit)"
done

large_slides=$((mem_mib / 128))
test "$large_slides" -ge 64 || large_slides=64
test "$large_slides" -le 240 || large_slides=240
awk -v slides="$large_slides" 'BEGIN {
  for (i = 1; i <= slides; i++) {
    asset = i % 2 ? "sample.jpg" : "sample.svg"
    printf "-- [%s] [fit]\nLarge deck slide %d\n", asset, i
  }
}' >"$run_tmp/large.pin"
/usr/bin/time -v flatpak run --user --filesystem="$run_tmp" "$app_id" \
  --pdf-no-speaker-notes --output="$run_tmp/large.pdf" "$run_tmp/large.pin" \
  >"$run_tmp/large.out" 2>"$run_tmp/large.log"
grep -q "PINPOINT PDF PASS slides=$large_slides pages=$large_slides" "$run_tmp/large.log"
large_rss_kib=$(awk -F: '/Maximum resident set size/ { gsub(/^[[:space:]]+/, "", $2); print $2 }' "$run_tmp/large.log")
test -n "$large_rss_kib"

echo "PINPOINT PDF VALIDATION PASS pages=5 jpeg_objects=$dct_count video_thumbnails=1 dynamic_cancel_slides=$cancel_slides large_slides=$large_slides large_rss_mib=$(awk -v kib="$large_rss_kib" 'BEGIN { printf "%.2f", kib / 1024 }')"
