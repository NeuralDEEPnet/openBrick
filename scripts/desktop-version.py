#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Check that the desktop app carries the workspace version, and check the
desktop release workflow.

docs/plan/09 sections 3 and 7, apps/desktop/README.md, docs/guides/install.md.

The desktop app has one version: `[workspace.package] version` in the root
Cargo.toml. The crate openbrick-desktop inherits it (`version.workspace =
true`), and tauri.conf.json has no `version` key, so the Tauri CLI takes the
crate's version for the bundles, the installers' file names, the Windows file
properties and the macOS Info.plist (https://v2.tauri.app/reference/config/
#version: "By default version is read from Cargo.toml"). apps/desktop's
package.json repeats it only so that nobody reads a second number there.

The installers add their own limit: an MSI ProductVersion is
major.minor.build with major and minor at most 255 and build at most 65535
(https://learn.microsoft.com/en-us/windows/win32/msi/productversion), and the
Tauri bundler refuses a pre-release suffix that is not numeric. Desktop
releases therefore need a plain X.Y.Z.

Usage, from anywhere in the repository:
    python3 -I scripts/desktop-version.py               print the version
    python3 -I scripts/desktop-version.py --check       exit 1 when a file disagrees
    python3 -I scripts/desktop-version.py --check --tag v0.2.0
                                                        also compare a release tag
    python3 -I scripts/desktop-version.py --check-workflow
                                                        check desktop-release.yml
    python3 -I scripts/desktop-version.py --self-test   test the checks themselves

CI runs --self-test, --check and --check-workflow in the `release` job of
ci.yml; .github/workflows/desktop-release.yml runs --check --tag on every tag.
"""

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ".github/workflows/desktop-release.yml"

# MSI ProductVersion limits (see the module docstring for the source).
MSI_MAX = (255, 255, 65535)


def workspace_version(cargo_toml: str) -> str:
    return tomllib.loads(cargo_toml)["workspace"]["package"]["version"]


def problems(cargo_toml: str, desktop_cargo: str, tauri_conf: str, package_json: str, tag: str | None) -> list[str]:
    """Every way the desktop app's version can differ from the workspace's.
    Each message says what was found, what was expected and how to fix it."""
    out = []
    version = workspace_version(cargo_toml)
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not m:
        out.append(
            f"DESKTOP.VERSION.FORMAT: the workspace version is {version!r}, but desktop "
            "installers need a plain X.Y.Z (an MSI allows no pre-release suffix). "
            "Use X.Y.Z in Cargo.toml for a desktop release."
        )
    else:
        for name, part, limit in zip(("major", "minor", "patch"), m.groups(), MSI_MAX):
            if int(part) > limit:
                out.append(
                    f"DESKTOP.VERSION.MSI: {name} is {part} in {version}, above the MSI limit of {limit}. "
                    "Pick a smaller number before tagging a desktop release."
                )
    desktop = tomllib.loads(desktop_cargo)["package"]
    if desktop.get("version") != {"workspace": True}:
        out.append(
            f"DESKTOP.VERSION.CRATE: apps/desktop/src-tauri/Cargo.toml has version {desktop.get('version')!r}; "
            "expected `version.workspace = true` so the app follows the workspace."
        )
    conf = json.loads(tauri_conf)
    if "version" in conf:
        out.append(
            f"DESKTOP.VERSION.TAURI: tauri.conf.json sets version {conf['version']!r}; expected no `version` "
            f"key, so Tauri reads {version} from Cargo.toml. Remove the key."
        )
    pkg = json.loads(package_json).get("version")
    if pkg != version:
        out.append(
            f"DESKTOP.VERSION.NPM: apps/desktop/package.json has version {pkg!r}; expected {version!r}, "
            "the workspace version. Set it to the same number."
        )
    if tag is not None and tag != f"v{version}":
        out.append(
            f"DESKTOP.VERSION.TAG: the tag is {tag!r}; expected 'v{version}', the workspace version. "
            "Tag the commit whose Cargo.toml has the version you release."
        )
    return out


def workflow_problems(text: str) -> list[str]:
    """desktop-release.yml keeps the promises in apps/desktop/README.md: actions
    pinned by commit, no AppImage, write access only where it uploads, and
    never on a dry run. Text checks, so CI needs no YAML library."""
    out = []
    for line in text.splitlines():
        m = re.match(r"\s*(?:-\s+)?uses:\s*([^\s#]+)", line)
        if m and not re.fullmatch(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}", m.group(1)):
            out.append(f"WORKFLOW.PIN: `{m.group(1)}` is not pinned by a full commit SHA; pin it (tags can move).")
    for m in re.finditer(r"--bundles\s+(\S+)", text):
        if "appimage" in m.group(1).lower():
            out.append("WORKFLOW.APPIMAGE: a row builds an AppImage, which waits for a licence decision (apps/desktop/README.md).")
    if re.search(r"(?mi)^\s*bundles:.*appimage", text):
        out.append("WORKFLOW.APPIMAGE: the matrix names an AppImage, which waits for a licence decision.")
    if not re.search(r"(?m)^permissions:\s*\n\s+contents:\s*read\s*$", text):
        out.append("WORKFLOW.PERMISSIONS: the workflow's default permissions must be `contents: read`.")
    writes = re.findall(r"(?m)^\s+contents:\s*write\s*$", text)
    if len(writes) != 1:
        out.append(f"WORKFLOW.PERMISSIONS: {len(writes)} jobs ask for `contents: write`; only the publish job may.")
    if "startsWith(github.ref, 'refs/tags/v')" not in text:
        out.append("WORKFLOW.DRYRUN: the publish job must run on tag pushes only, so a manual run uploads nothing.")
    if "desktop-version.py --check --tag" not in text:
        out.append("WORKFLOW.VERSION: the workflow must check the tag against the workspace version.")
    return out


def self_test() -> None:
    cargo = '[workspace.package]\nversion = "0.2.0"\n'
    crate = '[package]\nname = "openbrick-desktop"\nversion.workspace = true\n'
    conf = '{"productName": "OpenBrick3D"}'
    pkg = '{"version": "0.2.0"}'
    assert problems(cargo, crate, conf, pkg, "v0.2.0") == []
    assert problems(cargo, crate, conf, pkg, None) == []

    def ids(found):
        return [p.split(":")[0] for p in found]

    assert ids(problems(cargo, crate, conf, pkg, "v0.1.9")) == ["DESKTOP.VERSION.TAG"]
    assert ids(problems(cargo, crate, '{"version": "0.2.0"}', pkg, None)) == ["DESKTOP.VERSION.TAURI"]
    assert ids(problems(cargo, crate, conf, '{"version": "0.0.1"}', None)) == ["DESKTOP.VERSION.NPM"]
    assert ids(problems(cargo, crate.replace("version.workspace = true", 'version = "0.2.0"'), conf, pkg, None)) == [
        "DESKTOP.VERSION.CRATE"
    ]
    rc = cargo.replace("0.2.0", "0.2.0-rc.1")
    assert ids(problems(rc, crate, conf, '{"version": "0.2.0-rc.1"}', "v0.2.0-rc.1")) == ["DESKTOP.VERSION.FORMAT"]
    big = cargo.replace("0.2.0", "256.0.0")
    assert ids(problems(big, crate, conf, '{"version": "256.0.0"}', None)) == ["DESKTOP.VERSION.MSI"]
    # Messages carry what was found, what was expected and a fix.
    msg = problems(cargo, crate, conf, pkg, "v0.1.9")[0]
    assert "'v0.1.9'" in msg and "'v0.2.0'" in msg and "Tag the commit" in msg

    good = (
        "permissions:\n  contents: read\njobs:\n  a:\n    steps:\n"
        "      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0\n"
        "      - run: python3 -I scripts/desktop-version.py --check --tag x\n"
        "      - run: pnpm tauri build --bundles deb,rpm\n"
        "  publish:\n    if: startsWith(github.ref, 'refs/tags/v')\n    permissions:\n      contents: write\n"
    )
    assert workflow_problems(good) == [], workflow_problems(good)
    assert ids(workflow_problems(good.replace("@d23441a48e516b6c34aea4fa41551a30e30af803", "@v6"))) == ["WORKFLOW.PIN"]
    assert ids(workflow_problems(good.replace("deb,rpm", "deb,rpm,appimage"))) == ["WORKFLOW.APPIMAGE"]
    assert ids(workflow_problems(good.replace("  contents: read\n", "  contents: write\n", 1))) == [
        "WORKFLOW.PERMISSIONS",
        "WORKFLOW.PERMISSIONS",
    ]
    assert ids(workflow_problems(good.replace("startsWith(github.ref, 'refs/tags/v')", "true"))) == ["WORKFLOW.DRYRUN"]
    print("desktop-version: self-test ok")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="exit 1 when a file disagrees with the workspace version")
    parser.add_argument("--tag", help="a release tag (vX.Y.Z) that must name the workspace version")
    parser.add_argument("--check-workflow", action="store_true", help=f"check {WORKFLOW}")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    read = lambda p: (ROOT / p).read_text(encoding="utf-8")  # noqa: E731
    found = []
    if args.check or args.tag is not None:
        found += problems(
            read("Cargo.toml"),
            read("apps/desktop/src-tauri/Cargo.toml"),
            read("apps/desktop/src-tauri/tauri.conf.json"),
            read("apps/desktop/package.json"),
            args.tag,
        )
    if args.check_workflow:
        found += workflow_problems(read(WORKFLOW))
    for p in found:
        print(p, file=sys.stderr)
    if found:
        return 1
    if args.check or args.tag is not None or args.check_workflow:
        print(f"desktop-version: ok ({workspace_version(read('Cargo.toml'))})", file=sys.stderr)
    else:
        print(workspace_version(read("Cargo.toml")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
