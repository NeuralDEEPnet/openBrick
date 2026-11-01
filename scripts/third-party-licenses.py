#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Write the licence texts of the crates compiled into the openbrick CLI, or
into the desktop app (--desktop).

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

With --desktop, the file is for the desktop app's installers instead
(.github/workflows/desktop-release.yml ships it in every bundle through
apps/desktop/src-tauri/tauri.release.conf.json). The app binary is
`cargo tree -p openbrick-desktop --features app -e normal --target all`, as
`tauri build` compiles it (tauri.conf.json build.features); its webview runs
the playground, which carries the core compiled to WebAssembly
(`cargo tree -p openbrick-wasm -e normal --target wasm32-unknown-unknown`)
and the npm packages apps/web depends on at run time (three.js; their
dependencies too, if they gain any). The npm packages' licence files come
from node_modules, so run `pnpm install` first.

Usage: python3 scripts/third-party-licenses.py [--desktop] OUT.txt
Exit codes: 0 written; 1 a cargo command failed, a crate of the tree was not
found in the metadata, or an npm package is not installed.
"""

import json
import os
import pathlib
import subprocess
import sys
import textwrap

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = "openbrick"
# (package, extra cargo tree arguments, target) per binary of each product.
TREES = {
    "cli": [("openbrick", [], "all")],
    "desktop": [
        ("openbrick-desktop", ["--features", "app"], "all"),
        ("openbrick-wasm", [], "wasm32-unknown-unknown"),
    ],
}
# The npm packages bundled into the desktop app's webview: the run-time
# dependencies of these package.json files, recursively.
NPM_ROOTS = {"cli": [], "desktop": ["apps/web"]}
TITLES = {"cli": "the openbrick CLI", "desktop": "the OpenBrick3D desktop app"}
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


def tree_crates(product: str) -> set[tuple[str, str]]:
    """(name, version) of every package `cargo tree` lists for the product."""
    found = set()
    for package, extra, target in TREES[product]:
        # One package per line, as "name vX.Y.Z", followed by " (proc-macro)",
        # " (<path or source>)" or " (*)" for a package already listed.
        lines = cargo(
            "tree", "-p", package, *extra, "-e", "normal", "--target", target, "--prefix", "none", "--format", "{p}"
        )
        for line in lines.splitlines():
            words = line.split()
            if len(words) >= 2 and words[1].startswith("v"):
                found.add((words[0], words[1][1:]))
    return found


def npm_packages(product: str) -> list[dict]:
    """The npm packages bundled at run time: name, version, license, folder.
    Workspace packages (private, ours) are followed but not listed."""
    found: dict[tuple[str, str], dict] = {}
    todo = [(ROOT / r).resolve() for r in NPM_ROOTS[product]]
    seen = set()
    while todo:
        folder = todo.pop()
        if folder in seen:
            continue
        seen.add(folder)
        manifest = json.loads((folder / "package.json").read_text(encoding="utf-8"))
        if not manifest.get("private"):
            key = (manifest["name"], manifest["version"])
            found[key] = {"name": key[0], "version": key[1], "license": manifest.get("license"), "folder": folder}
        for dep in manifest.get("dependencies", {}):
            # pnpm links a package's dependencies into node_modules next to it;
            # look upwards as Node's resolver does.
            for base in [folder, *folder.parents]:
                cand = base / "node_modules" / dep
                if (cand / "package.json").is_file():
                    todo.append(pathlib.Path(os.path.realpath(cand)))
                    break
            else:
                raise FileNotFoundError(f"npm package {dep!r} (a dependency of {manifest['name']}) is not installed; run `pnpm install`")
    return [found[k] for k in sorted(found)]


def main() -> int:
    args = sys.argv[1:]
    product = "cli"
    if args[:1] == ["--desktop"]:
        product, args = "desktop", args[1:]
    if len(args) != 1:
        print(__doc__.strip().split("\n\n")[-2], file=sys.stderr)
        return 1
    out = pathlib.Path(args[0])
    try:
        # The metadata is only where the licence files are looked up; the
        # desktop app's crates sit behind its `app` feature, so switch it on.
        extra = ["--all-features"] if product == "desktop" else []
        meta = json.loads(cargo("metadata", "--format-version", "1", *extra))
        wanted = tree_crates(product)
        npm = npm_packages(product)
    except subprocess.CalledProcessError as e:
        print(f"third-party-licenses.py: {' '.join(e.cmd)} failed:\n{e.stderr}", file=sys.stderr)
        return 1
    except FileNotFoundError as e:
        print(f"third-party-licenses.py: {e}", file=sys.stderr)
        return 1
    workspace = set(meta["workspace_members"])
    for package, _, _ in TREES[product]:
        if not any(p["name"] == package and p["id"] in workspace for p in meta["packages"]):
            print(f"third-party-licenses.py: no workspace package {package!r}", file=sys.stderr)
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

    title = f"Third-party licences of {TITLES[product]}"
    if product == "cli":
        intro = [
            "OpenBrick3D's own code is under the MIT licence in LICENSE. The openbrick\n",
            f"binary also contains code from the {len(crates)} crates below. They are its\n",
            "dependencies for every platform, so a few are not in your build, and they\n",
            "include the procedural macros that only run while it compiles. Each entry\n",
            "gives the crate, its version, its SPDX licence expression and the licence\n",
            "files its published package ships, unchanged.\n",
        ]
    else:
        intro = [
            textwrap.fill(
                "OpenBrick3D's own code is under the MIT licence in LICENSE. The desktop app (its "
                "binary, and the WebAssembly core and the playground in its window) also contains "
                f"code from the {len(crates)} crates and the {len(npm)} npm package(s) below. The crates "
                "are its dependencies for every platform, so some are not in your build, and they "
                "include the procedural macros that only run while it compiles. Each entry gives "
                "the package, its version, its SPDX licence expression and the licence files its "
                "published package ships, unchanged. Where a licence asks for the source code to "
                "be available (MPL-2.0), the entry's Source line says where it is.",
                width=78,
            )
            + "\n"
        ]
    parts = [f"{title}\n", f"{'=' * len(title)}\n\n", *intro]
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

    for pkg in npm:
        header = f"npm {pkg['name']} {pkg['version']}"
        parts.append(f"\n\n{'=' * 78}\n{header}\nLicence: {pkg['license'] or 'not stated'}\n")
        files = sorted(
            (f for f in pkg["folder"].iterdir() if f.is_file() and f.name.upper().startswith(PREFIXES)),
            key=lambda f: f.name,
        )
        if not files:
            missing.append(header)
            parts.append("Its package ships no licence file; the licence is the SPDX expression above.\n")
        for f in files:
            text = f.read_text(encoding="utf-8", errors="replace").rstrip() + "\n"
            if text in first_with_text:
                parts.append(f"\n--- {f.name}: the same text as under {first_with_text[text]} above ---\n")
            else:
                first_with_text[text] = header
                parts.append(f"\n--- {f.name} ---\n\n{text}")

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("".join(parts), encoding="utf-8", newline="\n")
    print(f"wrote {out} ({len(crates)} crates" + (f", {len(npm)} npm packages)" if npm else ")"))
    for header in missing:
        print(f"note: {header} ships no licence file; listed with its SPDX expression", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
