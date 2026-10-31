#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 OpenBrick3D contributors
#
# OrcaSlicer slice smoke test (backlog item 18 in docs/plan/14, docs/plan/05
# section 7, Q18 in docs/spikes/q18-slicer-multimaterial-import.md). The
# slicer-smoke workflow runs it weekly and on demand; it is never a required
# pull request check.
#
#   1. Build the inputs with the openbrick CLI, as the `cli` CI job does: the
#      2x4 logo tile as emboss, as a multi-material (parts) inlay, and as that
#      inlay without colours, the Neuraldeep.net brand tile as a parts inlay,
#      and the fdm-0.4 calibration coupon, each with the CLI's --json
#      description. Check that every input exists and is a ZIP package.
#   2. Download the OrcaSlicer AppImage pinned below from its GitHub release
#      and check its SHA-256 (taken from the release page); a mismatch stops
#      the run before anything from the file is executed.
#   3. Extract it with --appimage-extract and resolve the pinned printer,
#      process and filament profiles from its bundled system profiles
#      (scripts/slicer-smoke.py flatten: the CLI does not follow `inherits`).
#   4. Slice every input headless (xvfb-run) with the OrcaSlicer command line:
#      --slice 0 --arrange 1 --orient 0 --export-3mf, two filaments loaded.
#   5. Report, per file, whether it sliced, the objects and parts OrcaSlicer
#      made and the filament each part got, and what that means for Q18
#      (scripts/slicer-smoke.py report): WORK/summary.md (also appended to
#      $GITHUB_STEP_SUMMARY) and WORK/results.json. Exit 1 when a file did not
#      slice.
#
# OrcaSlicer is AGPL-3.0. It is run as a separate program only, never linked,
# vendored or shipped (LICENSE-POLICY.md); the downloaded AppImage stays in
# the work directory.
#
# Usage:
#   scripts/slicer-smoke.sh [--dry-run] [--openbrick PATH] [--work DIR] [--download-dir DIR]
#
#   --dry-run        build and check the inputs and write the summary, but do
#                    not download, extract or run OrcaSlicer
#   --openbrick      the openbrick binary (default: target/release/openbrick)
#   --work           output directory (default: target/slicer-smoke); emptied first
#   --download-dir   where the AppImage is kept between runs (default: WORK/download)
#   --orca-root DIR  use an OrcaSlicer you extracted yourself (the squashfs-root
#                    of an AppImage, with AppRun) instead of downloading the
#                    pinned one; for trying another version locally. The
#                    summary then names DIR instead of the pinned checksum.
#
# Run it from the repository root.

set -euo pipefail

# The pinned slicer. SHA-256 of the asset as listed on
# https://github.com/OrcaSlicer/OrcaSlicer/releases/tag/v2.4.2 (read 2026-10-31).
ORCA_VERSION="2.4.2"
ORCA_APPIMAGE="OrcaSlicer_Linux_AppImage_Ubuntu2404_V2.4.2.AppImage"
ORCA_SHA256="d12fb8c8eac1aecd2dfb6377acd48f994f8fa439ed5292fa532dd82880f029fd"
ORCA_URL="https://github.com/OrcaSlicer/OrcaSlicer/releases/download/v${ORCA_VERSION}/${ORCA_APPIMAGE}"

# The printer of Q18's multi-material case: a Bambu Lab A1 with the AMS lite,
# two filaments of the same PLA in two colours (body grey, accent blue, the
# openbrick default role colours).
ORCA_VENDOR="BBL"
ORCA_MACHINE="Bambu Lab A1 0.4 nozzle"
ORCA_PROCESS="0.20mm Standard @BBL A1"
ORCA_FILAMENT="Bambu PLA Basic @BBL A1"
ORCA_COLOURS="#E6E6E6,#1E5AA8"
# Seconds one slice may take before it counts as failed.
SLICE_TIMEOUT=600

dry_run=0
openbrick="target/release/openbrick"
work="target/slicer-smoke"
download_dir=""
orca_root=""

while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry_run=1 ;;
    --openbrick) openbrick="${2:?--openbrick needs a path}"; shift ;;
    --work) work="${2:?--work needs a directory}"; shift ;;
    --download-dir) download_dir="${2:?--download-dir needs a directory}"; shift ;;
    --orca-root) orca_root="${2:?--orca-root needs a directory}"; shift ;;
    -h|--help) sed -n '2,46p' "$0"; exit 0 ;;
    *) echo "slicer-smoke: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

if [ ! -f Cargo.toml ] || [ ! -f scripts/slicer-smoke.py ]; then
  echo "slicer-smoke: run from the repository root" >&2
  exit 2
fi

need() {
  command -v "$1" > /dev/null 2>&1 || { echo "slicer-smoke: $1 is required ($2)" >&2; exit 2; }
}
need python3 "for scripts/slicer-smoke.py"
if [ "$dry_run" -eq 0 ]; then
  if [ -z "$orca_root" ]; then
    need curl "to download the AppImage"
    need sha256sum "to check the AppImage"
  elif [ ! -x "$orca_root/AppRun" ]; then
    echo "slicer-smoke: --orca-root $orca_root has no executable AppRun" >&2
    exit 2
  fi
  need xvfb-run "to run OrcaSlicer headless (apt-get install xvfb xauth)"
  need timeout "to bound each slice"
fi
if [ ! -x "$openbrick" ]; then
  echo "slicer-smoke: no openbrick binary at $openbrick (cargo build --release -p openbrick, or pass --openbrick)" >&2
  exit 2
fi

# The work directory is emptied, so it must be new, empty, or one this script
# made before (it leaves a marker file there).
marker=".slicer-smoke-work"
if [ -d "$work" ] && [ ! -f "$work/$marker" ] && [ -n "$(ls -A "$work")" ]; then
  echo "slicer-smoke: $work is not empty and was not made by this script; choose a new directory with --work" >&2
  exit 2
fi
rm -rf "$work"
mkdir -p "$work/inputs" "$work/sliced" "$work/profiles"
: > "$work/$marker"
work="$(cd "$work" && pwd)"
download_dir="${download_dir:-$work/download}"
mkdir -p "$download_dir"
download_dir="$(cd "$download_dir" && pwd)"
inputs="$work/inputs"

# 1. Inputs, with the CLI's description of each (the report compares the parts).
echo "slicer-smoke: building the inputs with $openbrick"
build() {
  local name="$1"; shift
  "$openbrick" "$@" --out "$inputs/$name.3mf" --json > "$inputs/$name.json" 2> "$inputs/$name.log" || {
    echo "slicer-smoke: openbrick failed for $name (see $inputs/$name.log):" >&2
    cat "$inputs/$name.log" >&2
    exit 1
  }
}
build logo-tile-emboss build parts/ndn.logo-tile.obpart.json --fit example --param relief=emboss
build logo-tile-inlay-parts build parts/ndn.logo-tile.obpart.json --fit example --params relief=inlay inlayStyle=parts
build logo-tile-inlay-parts-no-colour build parts/ndn.logo-tile.obpart.json --fit example --params relief=inlay inlayStyle=parts --no-colour
build brand-tile build parts/ndn.brand-tile.obpart.json --fit example --params relief=inlay inlayStyle=parts
build coupon-fdm-0.4-cross calibrate coupon --process fdm-0.4

names="logo-tile-emboss logo-tile-inlay-parts logo-tile-inlay-parts-no-colour brand-tile coupon-fdm-0.4-cross"
for name in $names; do
  file="$inputs/$name.3mf"
  if [ ! -s "$file" ]; then
    echo "slicer-smoke: input $file is missing or empty" >&2
    exit 1
  fi
  if [ "$(head -c 2 "$file")" != "PK" ]; then
    echo "slicer-smoke: input $file is not a ZIP package (3MF)" >&2
    exit 1
  fi
  echo "slicer-smoke: input $name.3mf ($(wc -c < "$file") bytes)"
done

report() {
  local status=0 pin=(--orca "$ORCA_VERSION" --appimage "$ORCA_APPIMAGE" --sha256 "$ORCA_SHA256")
  if [ -n "$orca_root" ]; then
    pin=(--orca "local build" --appimage "$orca_root" --sha256 "not checked (--orca-root)")
  fi
  python3 -I scripts/slicer-smoke.py report --work "$work" "$@" "${pin[@]}" || status=$?
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ] && [ -f "$work/summary.md" ]; then
    cat "$work/summary.md" >> "$GITHUB_STEP_SUMMARY"
  fi
  return "$status"
}

if [ "$dry_run" -eq 1 ]; then
  echo "slicer-smoke: dry run, OrcaSlicer is not downloaded or run"
  report --dry-run > /dev/null
  echo "slicer-smoke: dry run ok; summary in $work/summary.md"
  exit 0
fi

if [ -n "$orca_root" ]; then
  root="$(cd "$orca_root" && pwd)"
  echo "slicer-smoke: using the OrcaSlicer in $root (--orca-root; not the pinned AppImage)"
else
  # 2. Download and verify. The checksum is checked before the file is made
  #    executable, so a wrong file never runs.
  appimage="$download_dir/$ORCA_APPIMAGE"
  if [ -f "$appimage" ] && ! echo "$ORCA_SHA256  $appimage" | sha256sum --check --status; then
    echo "slicer-smoke: cached $appimage does not match the pinned SHA-256; downloading again"
    rm -f "$appimage"
  fi
  if [ ! -f "$appimage" ]; then
    echo "slicer-smoke: downloading $ORCA_URL"
    curl --fail --location --retry 3 --silent --show-error --output "$appimage.part" "$ORCA_URL"
    mv "$appimage.part" "$appimage"
  fi
  if ! echo "$ORCA_SHA256  $appimage" | sha256sum --check --status; then
    echo "slicer-smoke: $ORCA_APPIMAGE does not match the pinned SHA-256 $ORCA_SHA256 (got $(sha256sum "$appimage" | cut -d' ' -f1)); not running it" >&2
    exit 1
  fi
  echo "slicer-smoke: $ORCA_APPIMAGE matches SHA-256 $ORCA_SHA256"

  # 3. Extract (no FUSE needed).
  chmod +x "$appimage"
  mkdir -p "$work/orca"
  (cd "$work/orca" && "$appimage" --appimage-extract > /dev/null)
  root="$work/orca/squashfs-root"
fi
app="$root/AppRun"
if [ ! -x "$app" ]; then
  echo "slicer-smoke: $root has no executable AppRun" >&2
  exit 1
fi

# Resolve the profiles from the slicer's own system profiles.
index="$(find "$root" -path "*/profiles/$ORCA_VENDOR.json" -print -quit)"
if [ -z "$index" ]; then
  echo "slicer-smoke: no profiles/$ORCA_VENDOR.json under $root" >&2
  exit 1
fi
# Shared libraries the host lacks, for the log (the AppImage leaves some to the system).
find "$root" -type f -name 'orca-slicer' -exec ldd {} \; 2> /dev/null \
  | grep 'not found' > "$work/missing-libraries.txt" || true
if [ -s "$work/missing-libraries.txt" ]; then
  echo "slicer-smoke: libraries not found on this host:" >&2
  cat "$work/missing-libraries.txt" >&2
fi
python3 -I scripts/slicer-smoke.py flatten --profiles "$(dirname "$index")" --vendor "$ORCA_VENDOR" \
  --machine "$ORCA_MACHINE" --process "$ORCA_PROCESS" --filament "$ORCA_FILAMENT" \
  --colours "$ORCA_COLOURS" --out "$work/profiles"
printf 'Printer `%s`, process `%s`, filaments 1 and 2 `%s` (%s), resolved from the AppImage'"'"'s system profiles.\n' \
  "$ORCA_MACHINE" "$ORCA_PROCESS" "$ORCA_FILAMENT" "$ORCA_COLOURS" > "$work/profiles/settings.txt"
filaments=""
for f in "$work"/profiles/filament-*.json; do
  filaments="${filaments:+$filaments;}$f"
done

# The command line's own help, for the log.
timeout 120 xvfb-run -a "$app" --help > "$work/orca-help.txt" 2>&1 || true

# 4. Slice each input in its own output directory.
for name in $names; do
  out="$work/sliced/$name"
  mkdir -p "$out"
  echo "slicer-smoke: slicing $name.3mf"
  code=0
  timeout "$SLICE_TIMEOUT" xvfb-run -a "$app" \
    --datadir "$work/orca-data" \
    --debug 3 \
    --load-settings "$work/profiles/machine.json;$work/profiles/process.json" \
    --load-filaments "$filaments" \
    --arrange 1 --orient 0 \
    --slice 0 \
    --outputdir "$out" \
    --export-3mf "$name.sliced.3mf" \
    "$inputs/$name.3mf" > "$out/orca.log" 2>&1 || code=$?
  echo "$code" > "$out/exit-code"
  echo "slicer-smoke: $name.3mf: exit $code"
done

# 5. Report.
report
