#!/usr/bin/env python3
"""Generate Flatpak Builder sources for every crates.io Cargo.lock package."""

import argparse
import json
from pathlib import Path
import tomllib
from urllib.parse import quote


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("lockfile", type=Path, nargs="?", default=Path("Cargo.lock"))
    parser.add_argument("-o", "--output", type=Path, default=Path("flatpak/cargo-sources.json"))
    args = parser.parse_args()

    with args.lockfile.open("rb") as stream:
        lock = tomllib.load(stream)

    sources: list[dict[str, object]] = [
        {
            "type": "inline",
            "contents": (
                "[source.crates-io]\n"
                'replace-with = "vendored-sources"\n\n'
                "[source.vendored-sources]\n"
                'directory = "/app/share/pinpoint/cargo/vendor"\n'
            ),
            "dest": "cargo",
            "dest-filename": "config.toml",
        }
    ]
    for package in sorted(lock["package"], key=lambda item: (item["name"], item["version"])):
        source = package.get("source")
        if source is None:
            continue
        if source != "registry+https://github.com/rust-lang/crates.io-index":
            raise SystemExit(f"unsupported Cargo source for {package['name']}: {source}")
        checksum = package.get("checksum")
        if not checksum:
            raise SystemExit(f"missing checksum for {package['name']} {package['version']}")
        name = package["name"]
        version = package["version"]
        destination = f"cargo/vendor/{name}-{version}"
        escaped_name = quote(name, safe="")
        sources.extend(
            [
                {
                    "type": "archive",
                    "archive-type": "tar-gzip",
                    "url": (
                        f"https://static.crates.io/crates/{escaped_name}/"
                        f"{escaped_name}-{version}.crate"
                    ),
                    "sha256": checksum,
                    "dest": destination,
                },
                {
                    "type": "inline",
                    "contents": json.dumps({"files": {}, "package": checksum}),
                    "dest": destination,
                    "dest-filename": ".cargo-checksum.json",
                },
            ]
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(sources, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
