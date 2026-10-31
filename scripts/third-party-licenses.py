#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Write the licence texts of the crates compiled into the openbrick CLI.

Backlog item 15 (CLI releases), LICENSE-POLICY.md. The release archives carry
the CLI's own LICENSE; most of the crates linked into the binary are MIT,
Apache-2.0 or BSD, whose licences ask that their notice travel with every copy,
binaries included. This script collects those notices into one text file,
which dist puts into every archive (dist-workspace.toml `include`; the release
workflow runs this script before `dist build`, see .github/dist-build-setup.yml).

It reads `cargo metadata --locked` (so it needs network only if the crates
are not downloaded yet), follows the normal (not dev or build) dependencies of
the `openbrick` package for every platform, and for each crate outside the
workspace copies the licence files of its published package (LICENSE*,
LICENCE*, COPYING*, NOTICE*, UNLICENSE*, COPYRIGHT*, and `license-file`). A
crate whose package has none is listed with its SPDX expression and its
repository, and the script says so on stderr.

Usage: python3 scripts/third-party-licenses.py OUT.txt
Exit codes: 0 written; 1 cargo metadata failed or the package was not found.
"""

import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = "openbrick"
PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT")


def licence_files(pkg: dict) -> list[pathlib.Path]:
    folder = pathlib.Path(pkg["manifest_path"]).parent
    found = {p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(PREFIXES)}
    if pkg.get("license_file"):
        named = folder / pkg["license_file"]
        if named.is_file():
            found.add(named)
    return sorted(found, key=lambda p: p.name)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().split("\n\n")[-2], file=sys.stderr)
        return 1
    out = pathlib.Path(sys.argv[1])
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    workspace = set(meta["workspace_members"])
    roots = [i for i in workspace if packages[i]["name"] == PACKAGE]
    if len(roots) != 1:
        print(f"third-party-licenses.py: no workspace package {PACKAGE!r}", file=sys.stderr)
        return 1

    seen, todo = set(), roots
    while todo:
        current = todo.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    crates = sorted(
        (packages[i] for i in seen - workspace),
        key=lambda p: (p["name"], p["version"]),
    )

    parts = [
        "Third-party licences of the openbrick CLI\n",
        "=========================================\n\n",
        "OpenBrick3D's own code is under the MIT licence in LICENSE. The openbrick\n",
        f"binary also contains code from the {len(crates)} crates below (every platform's\n",
        "dependencies are listed, so a few are not in your build). Each entry gives\n",
        "the crate, its version, its SPDX licence expression and the licence files\n",
        "its published package ships, unchanged.\n",
    ]
    missing = []
    first_with_text = {}  # a licence text -> the first entry that printed it
    for pkg in crates:
        name, version = pkg["name"], pkg["version"]
        header = f"{name} {version}"
        parts.append(f"\n\n{'=' * 78}\n{header}\nLicence: {pkg.get('license') or 'not stated'}\n")
        if pkg.get("repository"):
            parts.append(f"Source: {pkg['repository']}\n")
        files = licence_files(pkg)
        if not files:
            missing.append(header)
            parts.append("Its package ships no licence file; the licence is the SPDX expression above.\n")
        for f in files:
            text = f.read_text(encoding="utf-8", errors="replace").rstrip() + "\n"
            if text in first_with_text:
                # Many crates ship the same Apache-2.0 text word for word.
                parts.append(f"\n--- {f.name}: the same text as under {first_with_text[text]} above ---\n")
            else:
                first_with_text[text] = header
                parts.append(f"\n--- {f.name} ---\n\n{text}")

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("".join(parts), encoding="utf-8", newline="\n")
    print(f"wrote {out} ({len(crates)} crates)")
    for header in missing:
        print(f"note: {header} ships no licence file; listed with its SPDX expression", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
