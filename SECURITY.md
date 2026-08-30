# Security policy

Pinpoint opens presentation text and media supplied by users. Please report a
crash, sandbox escape, unintended host access, unsafe command behavior, or
resource-exhaustion issue privately through GitHub's **Report a vulnerability**
form once this repository is published. Until then, share it directly with the
maintainer through an existing private contact rather than filing a public bug.

Include the Pinpoint version, Flatpak runtime version, input type, and the
smallest non-sensitive reproducer you can provide. Do not include private
presentations, credentials, or personal files. We will acknowledge a report,
assess affected versions, and coordinate disclosure after a fix is available.

Embedded `[command=…]` entries are an explicit presentation feature. They run
through `/bin/sh` inside Pinpoint's Flatpak sandbox, are limited to one process
at a time, and are stopped with the presentation. Commands begin disabled;
Pinpoint displays the exact first command and requires confirmation before any
command in that content revision can run. An external source change revokes
that session-only permission and stops a running command. The shipped manifests
do not grant access to the Flatpak service and therefore do not support
`flatpak-spawn --host`. Confirm commands carefully because they can still affect
files deliberately granted to Pinpoint.

CI runs `scripts/check-production-manifest.py` against the release manifest.
The gate rejects Flatpak service access, host or home filesystem grants, and
network sharing; development-only experiments must not add these permissions
to `com.nedrichards.pinpoint.json`.
