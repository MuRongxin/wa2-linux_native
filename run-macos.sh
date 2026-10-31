#!/usr/bin/env bash
# Experimental macOS launcher: local source only, never download or install.
set -euo pipefail

usage() {
  printf '%s\n' \
    'Usage: ./run-macos.sh [title | SCENE | --check | --build-only | --smoke]' \
    '  (default), title  Check local dependencies/data, compile, then open the title screen.' \
    '  SCENE             Check, compile, then run a numeric scene (for example 1002).' \
    '  --check           Read-only, offline dependency/data checks; no build or launch.' \
    '  --build-only      Check build tools and compile; no game data/runtime libs required.' \
    '  --smoke           Check build tools/SDL2, compile, run the brief fmtinfo diagnostic.' \
    '                    Opens a small native window; no game data required.' \
    '  --help            Show this help.' \
    '' \
    'No packages, AppImage, or game resources are downloaded or installed.' \
    'WA2_DATA defaults to ./data. Relative paths are relative to this checkout.' \
    'Existing WA2_*, SDL_*, PATH and Cargo settings are preserved. See MACOS.md.'
}

if [ "$#" -gt 1 ]; then
  usage >&2
  exit 2
fi
mode=play
scene=
case "${1-}" in
  --help|-h) usage; exit 0 ;;
  --check) mode=check ;;
  --build-only) mode=build ;;
  --smoke) mode=smoke ;;
  ''|title) ;;
  *)
    if [[ "$1" =~ ^[0-9]+$ ]]; then
      scene=$1
    else
      printf 'Unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
    fi
    ;;
esac

if [ "$(uname -s)" != Darwin ]; then
  printf '%s\n' 'This experimental launcher requires macOS (Darwin); it never runs Linux AppImages.' >&2
  exit 1
fi
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if ! command -v python3 >/dev/null 2>&1; then
  printf '%s\n' 'Python 3.8+ is required for local checks. If missing, consider: brew install python' >&2
  exit 1
fi
# The local migration can keep case-colliding resources in a case-sensitive
# sparsebundle next to this checkout. Only a normal launch mounts it; --check
# remains read-only and explicit WA2_DATA always takes precedence.
if [ "$mode" = play ] && [ "${WA2_DATA+x}" != x ] && [ ! -d data/script ] \
    && [ -L data ] && [ -d ../WA2Data.sparsebundle ]; then
    printf '%s\n' 'Mounting the local case-sensitive WA2 resource image...'
    hdiutil attach ../WA2Data.sparsebundle -mountpoint "$(dirname "$PWD")/wa2-data-volume" -nobrowse
fi
# -B prevents even importing the existing data predicate from creating __pycache__.
case "$mode" in
  build) python3 -B launcher/check_macos.py --build-only ;;
  smoke) python3 -B launcher/check_macos.py --smoke ;;
  *) python3 -B launcher/check_macos.py ;;
esac
[ "$mode" != check ] || exit 0

# The source discovers bundled and macOS fonts. Do not force WA2_FONT or override
# role-specific choices, saved profiles, SDL drivers, PATH, or Cargo settings.
# Cargo chooses its output location, honoring CARGO_TARGET_DIR / .cargo/config.
# This crate has no registry dependencies. --offline --locked forbids Cargo fetches.
if [ "$mode" = build ]; then
  exec cargo build --offline --locked --release --bin wa2-native
elif [ "$mode" = smoke ]; then
  printf '%s\n' 'SDL diagnostic only; this does not validate fonts, audio, video, or gameplay.'
  exec cargo run --offline --locked --release --bin wa2-native -- fmtinfo
elif [ -n "$scene" ]; then
  exec cargo run --offline --locked --release --bin wa2-native -- run "$scene"
else
  exec cargo run --offline --locked --release --bin wa2-native -- title
fi
