#!/usr/bin/env python3
"""Reject development-only permissions and payloads in the production manifest."""

import argparse
import json
import sys
from pathlib import Path


def forbidden_permissions(manifest: dict) -> list[str]:
    finish_args = manifest.get("finish-args", [])
    if not isinstance(finish_args, list):
        return ["finish-args must be a JSON array"]
    forbidden = []
    for argument in finish_args:
        if not isinstance(argument, str):
            forbidden.append(f"non-string finish argument: {argument!r}")
        elif argument == "--share=network":
            forbidden.append(argument)
        elif argument.startswith(("--filesystem=host", "--filesystem=home")):
            forbidden.append(argument)
        elif argument.startswith(
            ("--talk-name=org.freedesktop.Flatpak", "--system-talk-name=org.freedesktop.Flatpak")
        ):
            forbidden.append(argument)
    return forbidden


def forbidden_payloads(manifest: dict) -> list[str]:
    """Keep regression fixtures and experiment artifacts out of /app."""
    forbidden = []
    for module in manifest.get("modules", []):
        if not isinstance(module, dict):
            continue
        for command in module.get("build-commands", []):
            if not isinstance(command, str):
                continue
            if "install" not in command and "cp " not in command:
                continue
            if "tests/" in command or any(
                marker in command
                for marker in (
                    "/sample.jpg",
                    "/sample.svg",
                    "/sample.mp4",
                    "-validation.pin",
                    "/stage-smoke.pin",
                    "/pixel-reference.pin",
                    "/multi-monitor.pin",
                )
            ):
                forbidden.append(command)
    return forbidden


def check(path: Path) -> list[str]:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        return [f"cannot read production manifest: {error}"]
    errors = []
    if manifest.get("app-id") != "com.nedrichards.pinpoint":
        errors.append("production app-id must be com.nedrichards.pinpoint")
    errors.extend(forbidden_permissions(manifest))
    errors.extend(f"development payload: {item}" for item in forbidden_payloads(manifest))
    return errors


def self_test() -> None:
    safe = {"finish-args": ["--socket=wayland", "--device=dri"]}
    assert forbidden_permissions(safe) == []
    for permission in (
        "--talk-name=org.freedesktop.Flatpak",
        "--system-talk-name=org.freedesktop.Flatpak",
        "--filesystem=host",
        "--filesystem=host-os:ro",
        "--filesystem=home:ro",
        "--share=network",
    ):
        assert forbidden_permissions({"finish-args": [permission]}) == [permission]
    assert forbidden_payloads({"modules": [{"build-commands": ["install app /app/bin/app"]}]}) == []
    fixture_command = "install -Dm644 tests/fixtures/demo.pin /app/share/pinpoint/demo.pin"
    assert forbidden_payloads({"modules": [{"build-commands": [fixture_command]}]}) == [
        fixture_command
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "manifest",
        nargs="?",
        type=Path,
        default=Path("com.nedrichards.pinpoint.json"),
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    errors = check(args.manifest)
    if errors:
        for error in errors:
            print(f"production manifest permission gate: {error}", file=sys.stderr)
        return 1
    print(f"production manifest permission gate: PASS ({args.manifest})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
