# External editor support

Pinpoint presentations remain ordinary UTF-8 text files. Pinpoint monitors the
open `.pin` file, waits briefly for an editor to finish saving, then reloads the
presentation at the first slide whose source changed. The editor and presenter
can therefore stay open side by side without an editor becoming part of the
presentation runtime. Monitoring follows the presentation's directory so live
reload also survives editors that save safely by atomically replacing the
original file. When a sandboxed presentation is reached through the document
portal, where host-side file events are not forwarded reliably, Pinpoint checks
the file's revision twice a second instead.

Pinpoint also provides an optional built-in
[composition mode](composition-editor.md). It uses the same plain-text file,
language definition, parser, and renderer; this page remains the supported path
for hackers who prefer Text Editor, Builder, Vim, Emacs, or another tool.

## GtkSourceView 5 highlighting

Pinpoint ships `data/pinpoint.lang`, a GtkSourceView 5 language definition. It
recognises the `application/x-pinpoint` MIME type and the `*.pin` glob, and
highlights:

- presentation-default lines and slide separators;
- slide settings and embedded commands;
- audience text and first-column speaker notes;
- supported Pango markup tags, attributes, values, and entities; and
- escaped first-column characters.

### Native GtkSourceView applications

A native installation places the definition in
`$PREFIX/share/gtksourceview-5/language-specs`, where GtkSourceView 5 editors
can discover it automatically. For a per-user installation without installing
Pinpoint itself, use:

```sh
install -Dm644 data/pinpoint.lang \
  "$HOME/.local/share/gtksourceview-5/language-specs/pinpoint.lang"
```

Restart an editor after installing a new language definition.

### Flatpak applications

GtkSourceView searches each application's XDG data directories. Flatpak keeps
one application's `/app` separate from another, so the Pinpoint Flatpak cannot
silently add a language definition to an editor Flatpak. Install the definition
in the editor's own XDG data directory instead.

For GNOME Text Editor:

```sh
install -Dm644 data/pinpoint.lang \
  "$HOME/.var/app/org.gnome.TextEditor/data/gtksourceview-5/language-specs/pinpoint.lang"
```

For GNOME Builder:

```sh
install -Dm644 data/pinpoint.lang \
  "$HOME/.var/app/org.gnome.Builder/data/gtksourceview-5/language-specs/pinpoint.lang"
```

Install this file into Builder's application data as shown above, not into the
project's GNOME SDK, Flatpak build directory, or Pinpoint sandbox. Builder's
editor runs in the Builder application and loads its language manager there.
After restarting Builder, opening a `.pin` file in the project should select
Pinpoint highlighting from the filename and MIME metadata. An already-open
buffer may need to be closed and reopened, or switched to **Pinpoint** with
Builder's language selector.

Close and reopen the relevant application after installing the file so it
creates a fresh language manager. There is no need to restart Pinpoint.

### Other GtkSourceView editors

The same pattern works for another Flatpak that uses GtkSourceView 5. Find its
application ID with:

```sh
flatpak list --app
```

Then install `pinpoint.lang` under:

```text
~/.var/app/APP_ID/data/gtksourceview-5/language-specs/pinpoint.lang
```

The GtkSourceView language-file format is versioned separately from the
library. Version 2 definitions are understood by GtkSourceView 2 through 5,
but Pinpoint currently builds and tests this definition only with GtkSourceView
5. For an older editor, first confirm which GtkSourceView API it uses, then try
the corresponding `gtksourceview-VERSION/language-specs` directory rather than
assuming the version.

If highlighting does not appear:

1. Confirm that the file still ends in `.pin`.
2. Restart the editor so it reloads its language manager.
3. Look for **Pinpoint** in the editor's language selector.
4. Check for an older `pinpoint.lang` in another XDG data directory; the
   per-user directory takes priority over system definitions.

The durable distribution path is to contribute the definition to
GtkSourceView itself, so applications receive it from their runtime rather than
through cross-sandbox modification.

## Validation

The installed editor validation loads the definition through
`GtkSourceLanguageManager`, checks MIME and filename discovery, and exercises
the expected contexts against `tests/fixtures/editor-highlighting.pin`,
including the `#@alt:` visual-description directive. Editor tests also load
the bundled light and dark Pinpoint style schemes.

The complete syntax and live-editing behaviour are documented in the
[presentation format reference](presentation-format.md).
