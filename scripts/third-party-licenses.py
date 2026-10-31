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

The crates are the ones `cargo tree -p openbrick -e normal --target all`
lists: the normal (not dev or build) dependencies of the `openbrick` package
for every platform, with features resolved for that package alone, as
`dist build` compiles it (`cargo build --package=openbrick`, precise-builds in
dist-workspace.toml). `cargo metadata` would resolve features for the whole
workspace instead and add crates that only xtask or the WebAssembly facade
switch on. For each crate outside the workspace the script copies the licence
files of its published package (LICENSE*, LICENCE*, COPYING*, NOTICE*,
UNLICENSE*, COPYRIGHT*, and `license-file`), found through `cargo metadata`.
A crate whose package has none is listed with its SPDX expression and its
repository, and the script says so on stderr. Both cargo commands run with
`--locked`, so they need network only if the crates are not downloaded yet.

Usage: python3 scripts/third-party-licenses.py OUT.txt
Exit codes: 0 written; 1 a cargo command failed, or a crate of the tree was
not found in the metadata.
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


def cargo(*args: str) -> str:
    return subprocess.run(
        ["cargo", *args, "--locked"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout


def tree_crates() -> set[tuple[str, str]]:
    """(name, version) of every package `cargo tree` lists for the CLI."""
    # One package per line, as "name vX.Y.Z", followed by " (proc-macro)",
    # " (<path or source>)" or " (*)" for a package already listed.
    lines = cargo("tree", "-p", PACKAGE, "-e", "normal", "--target", "all", "--prefix", "none", "--format", "{p}")
    found = set()
    for line in lines.splitlines():
        words = line.split()
        if len(words) >= 2 and words[1].startswith("v"):
            found.add((words[0], words[1][1:]))
    return found


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().split("\n\n")[-2], file=sys.stderr)
        return 1
    out = pathlib.Path(sys.argv[1])
    try:
        meta = json.loads(cargo("metadata", "--format-version", "1"))
        wanted = tree_crates()
    except subprocess.CalledProcessError as e:
        print(f"third-party-licenses.py: {' '.join(e.cmd)} failed:\n{e.stderr}", file=sys.stderr)
        return 1
    workspace = set(meta["workspace_members"])
    if not any(p["name"] == PACKAGE and p["id"] in workspace for p in meta["packages"]):
        print(f"third-party-licenses.py: no workspace package {PACKAGE!r}", file=sys.stderr)
        return 1

    by_key: dict[tuple[str, str], list[dict]] = {}
    for p in meta["packages"]:
        if p["id"] not in workspace:
            by_key.setdefault((p["name"], p["version"]), []).append(p)
    workspace_keys = {(p["name"], p["version"]) for p in meta["packages"] if p["id"] in workspace}
    crates = []
    for key in sorted(wanted - workspace_keys):
        candidates = by_key.get(key, [])
        if len(candidates) != 1:
            # Not in the metadata, or the same name and version from two
            # sources (crates.io and git): the tree line cannot tell which.
            print(
                f"third-party-licenses.py: {key[0]} {key[1]} matches {len(candidates)} packages of cargo metadata",
                file=sys.stderr,
            )
            return 1
        crates.append(candidates[0])

    parts = [
        "Third-party licences of the openbrick CLI\n",
        "=========================================\n\n",
        "OpenBrick3D's own code is under the MIT licence in LICENSE. The openbrick\n",
        f"binary also contains code from the {len(crates)} crates below. They are its\n",
        "dependencies for every platform, so a few are not in your build, and they\n",
        "include the procedural macros that only run while it compiles. Each entry\n",
        "gives the crate, its version, its SPDX licence expression and the licence\n",
        "files its published package ships, unchanged.\n",
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
