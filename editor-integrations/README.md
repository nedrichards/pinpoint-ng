# Optional editor integrations

These small integrations make a `.pin` buffer pleasant in editors outside the
GTK composition view. They are source artefacts, not bundled into Pinpoint's
Flatpak, because one Flatpak cannot modify another editor's sandbox.

- `vscode/` is an unpacked VS Code language extension. Copy or symlink it into
  `~/.vscode/extensions/pinpoint-language` (or use **Install from VSIX** after
  packaging it with `vsce`).
- `vim/` is a normal Vim/Neovim runtime directory. Add it to `runtimepath`, or
  copy its contents under `~/.vim` or `~/.config/nvim`.
- `emacs/pinpoint-mode.el` is a small major mode. Put its directory on
  `load-path` and add `(require 'pinpoint-mode)` to your Emacs init file.

For live diagnostics, symbols, asset discovery or contextual completion, call
the read-only `flatpak run --user com.nedrichards.pinpoint --format-assist`
protocol documented in [`docs/format-intelligence.md`](../docs/format-intelligence.md).
The protocol is intentionally easier to wrap from an editor than an LSP, and
shares its parser vocabulary with the C and Pinpoint composition editors.
