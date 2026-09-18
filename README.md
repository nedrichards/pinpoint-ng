# Pinpoint

Pinpoint is a presentation application for hackers. It
turns concise plain-text source into image-led talks, preserving the beloved
Pinpoint presentation format in a modern Rust implementation.

The 0.3 release is a rust implementation  using GTK 4 and libadwaita and targeted at GNOME.
It includes the full presentation renderer and parser, live
reload, a source editor, speaker view, rehearsal timing, PDF export, MPRIS
remote control, display hot-plug handling, camera portal integration, and the
original page-curl transition.

This work is inspired by and dedicated to my former colleagues at the [Intel
Open Source Technology Center](https://desktopsummit.org/sponsors/intel.html)
in London. We tried to fuse creative technologists and designers together to
build extraordinary software that took ease of use, ease of hacking and easy on
 the eye to the next level. Like many things we were probably more influential
than successful but pinpoint was software we built for us. To share what we
were up to and in frustration that all the software we had available made
creating good presentations so hard, especially for non designers.
Whilst you can quickly create some mediocre slides in pinpoint it does make
it hard to create something truly awful. I've kept using it ever since, but
over time it's been harder to keep up to date as the world has moved on and
the concepts we worked on became mainstream. These projects are about bringing
this tool I use into the modern era, as a different codebase to the
[original 0.1.8 release](https://gitlab.gnome.org/Archive/pinpoint) or
the [0.2.0 C and GTK4 modern rewrite](https://github.com/nedrichards/fun-pinpoint)
which is intended to be a perfect modern reimagining. This expression of
pinpoint is a bit looser and more adapted to modern tastes. The codenames of these
repositories are based on in jokes from the team about terrible names for forks
of open source code and are meant with love.


## Using Pinpoint

Like all pinpoint you can use a full featured CLI. This expression also comes with a UI where
you can open a folder containing a presentation and its relative
assets, start a new deck in the composition editor, or open and copy the
bundled introduction. After selecting a deck, Pinpoint shows a parsed summary
and offers Present, Rehearse, Edit, and PDF export.

Presentation source is UTF-8 text, conventionally stored in a `.pin` file.
Slides are separated by a line beginning with three or more hyphens:

```text
[font=Sans 48px]
[duration=20]

# Welcome

---
[bgcolor=#204060]

This is the second slide.
```

The format supports styled text, images, SVG, video, camera input,
transitions, speaker notes, slide durations, commands, and source-relative
assets. See the [complete format reference](docs/presentation-format.md).

## Presentation controls

| Input | Action |
| --- | --- |
| Right, Down, Page Down, Space | Next slide |
| Left, Up, Backspace, Page Up | Previous slide |
| Home or `H` | First slide |
| `F` or F11 | Toggle fullscreen |
| F1 in speaker mode | Show or hide the speaker window |
| `B` | Toggle screen blanking |
| Return | Review or run the slide command inside Pinpoint's sandbox |
| `S` in speaker view | Swap audience and speaker displays |
| Escape or `Q` | Return to the editor, or quit Pinpoint |

Tab is used for completion in the composition editor; it does not edit or run
a command while presenting.

Touchscreen taps advance, horizontal swipes navigate, and pointer movement
reveals the audience close control. Speaker view shows previous, current, and
next slides alongside notes and timing.

## Command line

Run a deck directly with:

```sh
flatpak run --user com.nedrichards.pinpoint talk.pin
```

Use `--edit [talk.pin]` for composition mode, `--rehearse talk.pin` to record
timings, `--check talk.pin` for a non-interactive compatibility check, or
`--output=talk.pdf talk.pin` for PDF export. See the
[command-line reference](docs/command-line.md) for the complete stable
contract.

## Development

Pinpoint is built reproducibly with the pinned GNOME 51 Flatpak SDK and a
locked Rust dependency graph. Build instructions, internal validation gates,
and the optional C/Rust differential oracle are documented in
[the development guide](docs/development.md).

## License

Pinpoint is distributed under the GNU Lesser General Public License, version
2.1 or later. See [COPYING](COPYING) and the
[licensing and provenance notes](docs/licensing.md). Bundled introduction
media provenance is recorded separately in
[data/introduction/ORIGIN.md](data/introduction/ORIGIN.md).
