# Format intelligence

Pinpoint's authoring assistance is deliberately conservative. It knows the
`.pin` grammar, nearby assets and parser diagnostics; it does not invent slide
prose, rewrite audience text, rearrange a deck or save a file.

## In the composition editor

Press **Ctrl+Space** to request completion. Completion is manual so normal
writing stays quiet. The list has focus while it is open: use Up and Down to
choose, Enter or Tab to insert, and Escape to return to the source buffer.

Candidates depend on the insertion point:

- inside a setting bracket, Pinpoint offers valid setting names, then only the
  permitted values for enumerated settings;
- an unfinished background bracket also offers regular non-`.pin` files next
  to the presentation;
- after `<`, it offers supported Pango tags and places the cursor inside the
  generated closing tag;
- on a note line it offers `#@alt:`; and
- elsewhere it offers a slide separator, speaker note or visual description.

The parser continues to underline malformed or unknown syntax and the safe
preview remains paused rather than executing commands, audio or cameras while
the source is incomplete.

## Terminal/editor protocol

Pinpoint provides a stable read-only JSON protocol for editor plug-ins,
terminal workflows and CI:

```sh
flatpak run --user com.nedrichards.pinpoint --format-assist=diagnostics talk.pin
flatpak run --user com.nedrichards.pinpoint --format-assist=symbols talk.pin
flatpak run --user com.nedrichards.pinpoint --format-assist=assets talk.pin
flatpak run --user com.nedrichards.pinpoint --format-assist=completions \
  --complete-position=12:18 talk.pin
```

`--complete-position` is a one-based `LINE:COLUMN` byte position. It is
required for `completions` and rejected for the other kinds. The command cannot
be combined with presentation, rehearsal, PDF export or `--check`.

Every response is one JSON object with `version: 1` and `kind`. Diagnostic and
symbol offsets are zero-based UTF-8 byte offsets. Completion records contain
`label`, `detail`, `insert_text`, `replace_start`, and `cursor_back`.
`replace_start` is a zero-based byte offset; `cursor_back` is counted in UTF-8
characters from the end of the inserted text. This lets a plug-in apply paired
markup snippets and partially typed settings without recreating the grammar.
`assets` lists regular non-`.pin` siblings in sorted order. The command reads
the deck and directory only; it never writes, executes embedded commands or
opens media/camera portals.

## External editors

GtkSourceView highlighting remains the most direct integration for GNOME Text
Editor and Builder; see [External editor support](external-editors.md). The
repository also carries small optional [editor integrations](../editor-integrations/)
for VS Code, Vim and Emacs. They deliberately provide highlighting and
snippets only; richer plug-ins can call the protocol above without reimplementing
Pinpoint's parser.

An LSP is intentionally deferred. A separate long-running server is useful
only if this simple command contract proves insufficient; until then it would
duplicate transport and lifecycle complexity without improving format knowledge.
