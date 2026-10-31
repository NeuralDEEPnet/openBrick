#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
"""Generate or check .github/workflows/release.yml, the CLI release workflow.

Backlog item 15, decision D15 in docs/plan/03, docs/plan/09 sections 7 and 8,
docs/guides/install.md. dist (cargo-dist) writes the workflow from
dist-workspace.toml; this script runs it and then makes one edit, the `on:`
block, which dist has no setting for:

  - releases start only from tags vX.Y.Z (dist's own pattern accepts any tag
    with a version in it, such as openbrick-wasm/1.0.0);
  - pull requests run the workflow only when they touch the release
    configuration (TRIGGER_PATHS), and then only `dist plan`: dist publishes
    nothing for a pull request, from a fork or not.

Because of that edit dist-workspace.toml sets `allow-dirty = ["ci"]`, so dist
itself no longer checks release.yml; `--check` does instead (CI job
release-plan).

Usage, from anywhere in the repository, with the dist version that
dist-workspace.toml pins (cargo-dist-version) on PATH:
    python3 scripts/release-workflow.py            write release.yml
    python3 scripts/release-workflow.py --check    exit 1 if release.yml differs
    python3 scripts/release-workflow.py --dist-version
                                                   print the pinned dist version

dist only writes into the working tree, so the script removes the
`allow-dirty` line from dist-workspace.toml while dist runs and puts the file
back afterwards, whatever happens. With --check, release.yml is put back as
well.
"""

import argparse
import difflib
import os
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CONFIG = ROOT / "dist-workspace.toml"
WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"
ALLOW_DIRTY = 'allow-dirty = ["ci"]\n'

# The `on:` block dist 0.33 writes for pr-run-mode = "plan" and tag pushes.
DIST_TRIGGERS = """on:
  pull_request:
  push:
    tags:
      - '**[0-9]+.[0-9]+.[0-9]+*'
"""

# Files whose change can break a release; a pull request that touches none of
# them cannot change what `dist plan` says.
TRIGGER_PATHS = [
    "dist-workspace.toml",
    "Cargo.toml",
    "crates/openbrick/Cargo.toml",
    "rust-toolchain.toml",
    ".github/dist-build-setup.yml",
    ".github/workflows/release.yml",
    "scripts/release-workflow.py",
    "scripts/third-party-licenses.py",
]

OUR_TRIGGERS = (
    """on:
  # Edited by scripts/release-workflow.py, not by dist (see
  # dist-workspace.toml): releases start only from tags vX.Y.Z, and pull
  # requests run `dist plan` only when they touch the release configuration.
  pull_request:
    paths:
"""
    + "".join(f'      - "{p}"\n' for p in TRIGGER_PATHS)
    + """  push:
    tags:
      - 'v[0-9]+.[0-9]+.[0-9]+*'
"""
)


def pinned_dist_version() -> str:
    with CONFIG.open("rb") as f:
        return tomllib.load(f)["dist"]["cargo-dist-version"]


def patch_triggers(generated: str) -> str:
    if generated.count(DIST_TRIGGERS) != 1:
        sys.exit(
            "release-workflow.py: dist wrote a different `on:` block than this "
            "script expects (DIST_TRIGGERS); a new dist version? Update "
            "DIST_TRIGGERS and OUR_TRIGGERS together."
        )
    return generated.replace(DIST_TRIGGERS, OUR_TRIGGERS)


def generate(dist: str) -> str:
    """Run `dist generate --mode ci` and return the patched release.yml."""
    config = CONFIG.read_text(encoding="utf-8")
    if config.count(ALLOW_DIRTY) != 1:
        sys.exit(f"release-workflow.py: expected one line {ALLOW_DIRTY.strip()!r} in {CONFIG.name}")
    try:
        CONFIG.write_text(config.replace(ALLOW_DIRTY, ""), encoding="utf-8")
        subprocess.run([dist, "generate", "--mode", "ci"], cwd=ROOT, check=True)
        generated = WORKFLOW.read_text(encoding="utf-8")
    finally:
        CONFIG.write_text(config, encoding="utf-8")
    return patch_triggers(generated)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="compare instead of writing")
    ap.add_argument("--dist-version", action="store_true", help="print the pinned dist version")
    ap.add_argument("--dist", default=os.environ.get("DIST", "dist"), help="dist executable (default: dist, or $DIST)")
    args = ap.parse_args()

    if args.dist_version:
        print(pinned_dist_version())
        return 0

    before = WORKFLOW.read_bytes() if WORKFLOW.exists() else None
    try:
        expected = generate(args.dist)
    except BaseException:
        if before is not None:
            WORKFLOW.write_bytes(before)
        raise
    if not args.check:
        WORKFLOW.write_text(expected, encoding="utf-8")
        print(f"wrote {WORKFLOW.relative_to(ROOT)}")
        return 0

    if before is None:
        WORKFLOW.unlink(missing_ok=True)
        print(f"{WORKFLOW.relative_to(ROOT)} is missing; run python3 scripts/release-workflow.py", file=sys.stderr)
        return 1
    WORKFLOW.write_bytes(before)
    actual = before.decode("utf-8")
    if actual == expected:
        print(f"{WORKFLOW.relative_to(ROOT)} is what dist {pinned_dist_version()} generates (with the trigger edit)")
        return 0
    sys.stderr.writelines(
        difflib.unified_diff(
            actual.splitlines(keepends=True),
            expected.splitlines(keepends=True),
            "release.yml (committed)",
            "release.yml (generated)",
        )
    )
    print(
        "\nrelease.yml differs from what dist generates; change dist-workspace.toml, "
        "not release.yml, and run python3 scripts/release-workflow.py",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
