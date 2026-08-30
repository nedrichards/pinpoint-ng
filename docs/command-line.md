# Command-line interface

Pinpoint is Flatpak-first. Invoke the installed application through its app ID;
the inner `pinpoint` options retain the original 0.1.8 presentation options and
add scriptable checking and PDF controls:

```sh
flatpak run --user com.nedrichards.pinpoint \
  [--maximized] [--fullscreen] [--speakermode] [--rehearse] \
  [--ignore-comments] presentation.pin
```

Exactly one presentation may be supplied. Running without one opens the GTK
setup screen; supplying more than one is an error instead of silently ignoring
the additional paths.

Assets are confined to the presentation folder by default. A deliberate CLI
presentation, check, or PDF export may add `--allow-external-assets` for legacy
decks containing absolute paths, `file://` URLs, or parent-directory
references. The flag is session-only and should be used only with a trusted
deck.

Open an existing deck in composition mode, or start an untitled deck, with:

```sh
flatpak run --user com.nedrichards.pinpoint --edit presentation.pin
flatpak run --user com.nedrichards.pinpoint --edit
```

Composition mode cannot be combined with checking, PDF output, rehearsal,
speaker mode, or presentation fullscreen options. It uses the same parser,
asset store, and renderer as presentation mode, while keeping commands, audio,
and camera access disabled in its safe preview.

Append `--version` for the installed version and `--help` for the complete
option list. A native installation may invoke its installed `pinpoint` binary
directly, but documentation and integrations should prefer the Flatpak form.

## Camera backgrounds

`[camera]` backgrounds use the desktop **Camera portal**. When an active,
visible presentation reaches a camera slide, Pinpoint requests access and uses
the PipeWire stream supplied by the portal. No raw `/dev/video*` access or
device-specific Flatpak permission is required.

`--camera=DEVICE` and `-c DEVICE` were historical command-line options, but
they never selected a camera in the portal implementation. They are now
rejected with a migration diagnostic. Remove the option and approve the portal
request; select a preferred camera through the desktop's camera or portal
settings when that environment offers a choice.

## Checking a presentation

`--check` provides a non-interactive compatibility check suitable for editor,
CI, and packaging workflows:

```sh
flatpak run --user com.nedrichards.pinpoint --check presentation.pin
```

It verifies that the source is readable UTF-8, parses it with the same
compatibility parser used for presenting, checks referenced relative image,
SVG, and video backgrounds, and loads each custom legacy transition JSON. It
does not execute embedded commands, open a camera, or decode media. Built-in
transition names retain their normal runtime handling. Outside-folder assets
are rejected unless the invocation explicitly uses `--allow-external-assets`.

A successful check prints the slide count and exits 0. A read, parse, missing
asset, or custom-transition error is written to standard error and exits 1.
`--check` cannot be combined with `--output`.

## Format assistance for editors

`--format-assist=diagnostics`, `symbols`, `assets` or `completions` emits a
read-only JSON object for external editor integrations. `completions` also
requires `--complete-position=LINE:COLUMN`. It never presents, saves, executes
commands or activates media. See [Format intelligence](format-intelligence.md)
for the stable response schema and examples.

## PDF export

PDF export is non-interactive when `--output` is present:

```sh
flatpak run --user com.nedrichards.pinpoint --output=talk.pdf --pdf-page-size=a4 \
  --pdf-orientation=landscape --pdf-no-speaker-notes talk.pin
```

Paper size accepts `a4` or `letter`; orientation accepts `landscape` or
`portrait`. `--ignore-comments` excludes comment notes. These PDF options also
set the initial choices in the graphical exporter when `--output` is omitted.

The PDF is rendered to a temporary file beside the destination and moved into
place only after every page succeeds. A failed or cancelled export therefore
preserves an existing destination. Pinpoint also refuses an output that is the
presentation source, including a different path resolving to that same file.

Progress is written to standard error only when it is a terminal, leaving
redirected and scripted exports quiet on success.

## Interruption and exit status

- Normal completion or an ordinary graphical close exits 0.
- Invalid options, ambiguous arguments, failed checks, and failed exports exit
  1 with a `pinpoint:` diagnostic on standard error.
- Ctrl+C cancels work, preserves source and destination files, and exits 130.
- SIGTERM performs the same orderly cancellation and exits 143.
- A second termination signal forces an immediate exit if a decoder, file
  system, or other dependency does not respond to cancellation.

Ctrl+C during rehearsal deliberately discards the in-progress timing changes,
matching the original command-line contract. Timings are written only after a
rehearsal reaches its normal completion path.
