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


def code_lines(text: str) -> list[str]:
    """The workflow's lines without the comment-only ones."""
    return [line for line in text.splitlines() if not line.lstrip().startswith("#")]


def jobs(lines: list[str]) -> dict[str, list[str]]:
    """Each job's lines (its body under `jobs:`), by job id. The workflow
    indents by two spaces, as every workflow in .github does."""
    out: dict[str, list[str]] = {}
    current = None
    inside = False
    for line in lines:
        if re.match(r"jobs:\s*$", line):
            inside = True
            continue
        if not inside:
            continue
        if line and not line[0].isspace():
            break
        m = re.match(r"  ([\w-]+):\s*$", line)
        if m:
            current = m.group(1)
            out[current] = []
        elif current is not None:
            out[current].append(line)
    return out


def permissions(lines: list[str], indent: str) -> dict[str, str] | None:
    """The `permissions:` at this indent: {scope: access}, {"*": "write-all"}
    for the short forms, or None when there is none."""
    for i, line in enumerate(lines):
        m = re.match(rf"{indent}permissions:\s*(\S*)\s*$", line)
        if not m:
            continue
        if m.group(1):
            return {"*": m.group(1)}
        scopes = {}
        for body in lines[i + 1 :]:
            n = re.match(rf"{indent}\s+([\w-]+):\s*(\S+)", body)
            if not n:
                break
            scopes[n.group(1)] = n.group(2)
        return scopes
    return None


def workflow_problems(text: str) -> list[str]:
    """desktop-release.yml keeps the promises in apps/desktop/README.md: actions
    pinned by commit, no AppImage, the licence notices in every bundle, write
    access only in the publish job, and publish never on a dry run. Comment
    lines do not count. The checks read the YAML as text, scoped to each job,
    so CI needs no YAML library."""
    out = []
    lines = code_lines(text)
    code = "\n".join(lines)
    for line in lines:
        m = re.match(r"\s*(?:-\s+)?uses:\s*([^\s#]+)", line)
        if m and not re.fullmatch(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}", m.group(1)):
            out.append(
                f"WORKFLOW.PIN: `{m.group(1)}` is not pinned; expected owner/repo@<40-hex commit>. "
                "Pin it by full commit SHA with the tag in a comment (tags can move)."
            )
    for m in re.finditer(r"--bundles\s+(\S+)", code):
        if "appimage" in m.group(1).lower():
            out.append(
                f"WORKFLOW.APPIMAGE: a row builds `--bundles {m.group(1)}`; expected no AppImage, which waits for "
                "a licence decision (apps/desktop/README.md). Remove appimage from the list."
            )
    for m in re.finditer(r"(?mi)^\s*(?:-\s+)?bundles:\s*(.*appimage.*)$", code):
        out.append(
            f"WORKFLOW.APPIMAGE: the matrix names `bundles: {m.group(1).strip()}`; expected no AppImage, which "
            "waits for a licence decision (apps/desktop/README.md). Remove appimage from the row."
        )
    top = permissions(lines, "")
    if top != {"contents": "read"}:
        out.append(
            f"WORKFLOW.PERMISSIONS: the workflow's default permissions are {top}; expected {{'contents': 'read'}}. "
            "Set `permissions: contents: read` at the top and grant more only in the publish job."
        )
    by_job = jobs(lines)
    for job, body in by_job.items():
        scopes = permissions(body, "    ") or {}
        writes = sorted(k for k, v in scopes.items() if v in ("write", "write-all"))
        if job != "publish" and writes:
            out.append(
                f"WORKFLOW.PERMISSIONS: job `{job}` asks for write access ({', '.join(writes)}); expected read only. "
                "Only the publish job may write; move the step there."
            )
        if scopes.get("*") == "write-all":
            out.append(
                f"WORKFLOW.PERMISSIONS: job `{job}` has `permissions: write-all`; expected named scopes. "
                "List only contents, attestations and id-token."
            )
    publish = by_job.get("publish")
    if publish is None:
        out.append("WORKFLOW.DRYRUN: there is no job `publish`; expected the one job that uploads. Name it publish.")
    else:
        scopes = permissions(publish, "    ") or {}
        if scopes.get("contents") != "write":
            out.append(
                f"WORKFLOW.PERMISSIONS: job `publish` has contents: {scopes.get('contents')}; expected write, "
                "to upload to the release. Grant `contents: write` there."
            )
        cond = next((m.group(1) for line in publish if (m := re.match(r"    if:\s*(.*)$", line))), None)
        if not cond or "github.event_name == 'push'" not in cond or "startsWith(github.ref, 'refs/tags/v')" not in cond:
            out.append(
                f"WORKFLOW.DRYRUN: the publish job's `if:` is {cond!r}; expected github.event_name == 'push' && "
                "startsWith(github.ref, 'refs/tags/v'), so a manual run uploads nothing. Restore that condition."
            )
    if "desktop-version.py --check --tag" not in code:
        out.append(
            "WORKFLOW.VERSION: no step runs `desktop-version.py --check --tag`; expected one before the builds, "
            "so a tag that differs from the workspace version stops the release. Add it to the version job."
        )
    if "third-party-licenses.py --desktop" not in code or "tauri.release.conf.json" not in code:
        out.append(
            "WORKFLOW.NOTICES: the bundles do not carry the licence notices; expected a step running "
            "`third-party-licenses.py --desktop` and `tauri build --config src-tauri/tauri.release.conf.json`. "
            "Add both, so THIRD-PARTY-LICENSES.txt ships in every installer."
        )
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
        "# A comment line: uses: x@v1, bundles: appimage, permissions: write-all.\n"
        "on: push\npermissions:\n  contents: read\njobs:\n"
        "  version:\n    runs-on: ubuntu-24.04\n    steps:\n"
        "      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0\n"
        "      - run: python3 -I scripts/desktop-version.py --check --tag x\n"
        "  bundle:\n    strategy:\n      matrix:\n        include:\n          - bundles: deb,rpm\n    steps:\n"
        "      - run: python3 scripts/third-party-licenses.py --desktop target/desktop-notices/T.txt\n"
        '      - run: pnpm tauri build --bundles "$BUNDLES" --config src-tauri/tauri.release.conf.json\n'
        "  publish:\n    if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')\n"
        "    permissions:\n      attestations: write\n      contents: write\n      id-token: write\n"
    )
    assert workflow_problems(good) == [], workflow_problems(good)

    def bad(old, new, count=1):
        assert old in good, old
        return ids(workflow_problems(good.replace(old, new, count)))

    assert bad("@d23441a48e516b6c34aea4fa41551a30e30af803", "@v6") == ["WORKFLOW.PIN"]
    assert bad('--bundles "$BUNDLES"', "--bundles deb,appimage") == ["WORKFLOW.APPIMAGE"]
    assert bad("bundles: deb,rpm", "bundles: deb,rpm,appimage") == ["WORKFLOW.APPIMAGE"]
    assert bad("  contents: read\n", "  contents: write\n") == ["WORKFLOW.PERMISSIONS"]
    assert bad("permissions:\n  contents: read\n", "permissions: write-all\n") == ["WORKFLOW.PERMISSIONS"]
    assert bad("    runs-on: ubuntu-24.04\n", "    runs-on: ubuntu-24.04\n    permissions:\n      packages: write\n") == [
        "WORKFLOW.PERMISSIONS"
    ]
    assert bad("    permissions:\n      attestations: write\n      contents: write\n      id-token: write\n",
               "    permissions: write-all\n") == ["WORKFLOW.PERMISSIONS", "WORKFLOW.PERMISSIONS"]
    assert bad("      contents: write\n", "      contents: read\n") == ["WORKFLOW.PERMISSIONS"]
    # The condition only in a comment does not count: it must be publish's if.
    assert bad(
        "    if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')\n",
        "    # startsWith(github.ref, 'refs/tags/v')\n    if: always()\n",
    ) == ["WORKFLOW.DRYRUN"]
    assert bad("github.event_name == 'push' && ", "") == ["WORKFLOW.DRYRUN"]
    assert bad("  publish:\n", "  upload:\n") == ["WORKFLOW.PERMISSIONS", "WORKFLOW.DRYRUN"]
    assert bad("desktop-version.py --check --tag x", "desktop-version.py --check") == ["WORKFLOW.VERSION"]
    assert bad(" --config src-tauri/tauri.release.conf.json", "") == ["WORKFLOW.NOTICES"]
    assert bad("third-party-licenses.py --desktop", "third-party-licenses.py") == ["WORKFLOW.NOTICES"]
    msg = workflow_problems(good.replace("  contents: read\n", "  contents: write\n", 1))[0]
    assert "{'contents': 'write'}" in msg and "expected {'contents': 'read'}" in msg and "Set `permissions" in msg
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
