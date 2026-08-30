#!/usr/bin/env python3
"""Create a reviewable dependency-license inventory from Cargo.lock and vendor/."""

import argparse
from collections import defaultdict
from pathlib import Path
import tomllib


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--lockfile", type=Path, default=Path("Cargo.lock"))
    parser.add_argument("--vendor", type=Path, default=Path("vendor"))
    parser.add_argument("-o", "--output", type=Path, default=Path("THIRD_PARTY_LICENSES.md"))
    args = parser.parse_args()

    with args.lockfile.open("rb") as stream:
        locked = tomllib.load(stream)["package"]
    wanted = {
        (package["name"], str(package["version"]))
        for package in locked
        if package.get("source") is not None
    }

    found: dict[tuple[str, str], str] = {}
    for directory in args.vendor.iterdir():
        manifest = directory / "Cargo.toml"
        if not manifest.is_file():
            continue
        with manifest.open("rb") as stream:
            package = tomllib.load(stream)["package"]
        key = (package["name"], str(package["version"]))
        if key not in wanted:
            continue
        license_expression = package.get("license")
        if not license_expression:
            license_file = package.get("license-file")
            if not license_file:
                raise SystemExit(f"{key[0]} {key[1]} declares no license")
            license_expression = f"LicenseRef-{license_file}"
        found[key] = license_expression

    missing = sorted(wanted - found.keys())
    if missing:
        details = ", ".join(f"{name} {version}" for name, version in missing)
        raise SystemExit(f"vendored metadata is missing for: {details}")

    groups: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for package, expression in found.items():
        groups[expression].append(package)

    lines = [
        "# Third-party Rust dependency licenses",
        "",
        "This inventory is generated from the release `Cargo.lock` and each crate's",
        "published `Cargo.toml`. It records declared SPDX expressions; upstream license",
        "texts remain in the corresponding crate source archives fetched by the Flatpak",
        "build. Regenerate with `python3 scripts/generate-license-inventory.py` after",
        "running `cargo vendor vendor` whenever the dependency lock changes.",
        "",
        f"Packages inventoried: {len(found)}",
        "",
    ]
    for expression in sorted(groups, key=str.casefold):
        lines.extend([f"## {expression}", ""])
        for name, version in sorted(groups[expression]):
            lines.append(f"- `{name}` {version}")
        lines.append("")
    args.output.write_text("\n".join(lines), encoding="utf-8")


if __name__ == "__main__":
    main()
