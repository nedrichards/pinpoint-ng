# Accessibility

Pinpoint keeps its setup workflow in native GTK and libadwaita controls so
keyboard focus, control roles, switch state, menu structure, and platform text
scaling come from the desktop toolkit. Icon-only setup actions also have
explicit accessible names rather than relying on their icons or tooltips.

The presentation stage is custom-rendered, so Pinpoint supplies the semantics
which GTK cannot infer from child widgets. The focused stage exposes:

- its role as a grouped presentation surface;
- the current position, such as “Presentation slide 2 of 12”;
- the visible audience text with Pango markup removed;
- the blank-screen state; and
- the non-standard presentation keyboard controls as shortcut and help text.

Those published controls match presentation mode exactly. In particular, Tab
is an editor-completion key and is not advertised as a command-editing action
while presenting; Enter reviews or runs the current slide command.

Slide changes update those properties while focus remains on the stage. The
speaker view identifies its three rendered previews as previous, current, and
next slides. Speaker notes, remaining time, and the visual timing indicator
have contextual names. Every toolbar operation remains a focusable text button;
the display-swap and fullscreen buttons also publish their `S` and `F11`
shortcuts.

The stage accepts a deliberate horizontal swipe: one finger on a touchscreen
or two fingers on a touchpad moves left for next and right for previous. The
gesture needs at least 96 logical pixels and must be predominantly horizontal,
so ordinary scrolling does not advance a slide. Pinpoint intentionally does
nothing with three-finger gestures, leaving GNOME workspace navigation alone.

Pinpoint follows GTK 4.22's `gtk-interface-reduced-motion` preference as well as
the older `gtk-enable-animations` setting. When either requests less motion,
slide transitions complete immediately rather than running a reduced but still
moving effect. Pinpoint prints one informational message per process when this
path is first used so an intentional accessibility response is not mistaken for
a broken transition.

## Author responsibility

Audience text and `#@alt:` visual descriptions are available to assistive
technology. Use one or more `#@alt:` lines for information conveyed by an
image, video, SVG, or camera background that the audience text does not already
say. Pinpoint joins those lines into the stage description after the visible
audience text; they do not appear in the speaker-note pane.

The directive deliberately uses the existing first-column comment syntax.
Unknown bracketed settings mean “background” in the historical parser, so an
`[alt=…]` setting would make a legacy Pinpoint render the same file incorrectly.
Pinpoint 0.1.8 instead treats `#@alt:` as an ordinary speaker note and leaves
the audience output intact. Authors should still avoid relying on an
uncaptioned visual as the only source of essential information.

Colour contrast inside a slide is likewise selected by the presentation
author. Pinpoint retains the historical shading controls and defaults to a
dark, partially opaque text backdrop, but it does not rewrite authored colours.

## Regression coverage

`tests/test-lifecycle.c` checks the stage role, initial and changing slide
labels, audience text, blank state, contextual preview naming, and the
reduced-motion path. The normal GTK lifecycle test remains display-backed so
these assertions use GTK's real accessible context rather than a parallel model.
