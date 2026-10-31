#!/usr/bin/env python3
"""Read-only, offline checks for the experimental macOS source launcher."""

import argparse
import ctypes
import os
from pathlib import Path
import platform
import shutil
import sys

# Keep this true even when the checker is run directly without python3 -B.
sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parent.parent
# Keep these candidates aligned with src/sdl2.rs and src/ft2.rs. Do not use
# ctypes.util.find_library: an SDK or brew result need not match engine lookup.
LIBRARIES = {
    "SDL2": (
        "libSDL2-2.0.0.dylib", "libSDL2.dylib",
        "/opt/homebrew/lib/libSDL2-2.0.0.dylib", "/opt/homebrew/lib/libSDL2.dylib",
        "/opt/homebrew/opt/sdl2/lib/libSDL2.dylib",
        "/usr/local/lib/libSDL2-2.0.0.dylib", "/usr/local/lib/libSDL2.dylib",
        "/usr/local/opt/sdl2/lib/libSDL2.dylib",
        "SDL2.framework/SDL2", "/Library/Frameworks/SDL2.framework/SDL2",
    ),
    "FreeType": (
        "libfreetype.6.dylib", "libfreetype.dylib",
        "/opt/homebrew/lib/libfreetype.6.dylib", "/opt/homebrew/lib/libfreetype.dylib",
        "/opt/homebrew/opt/freetype/lib/libfreetype.6.dylib",
        "/usr/local/lib/libfreetype.6.dylib", "/usr/local/lib/libfreetype.dylib",
        "/usr/local/opt/freetype/lib/libfreetype.6.dylib",
    ),
}


def library_available(label):
    failures = []
    for name in LIBRARIES[label]:
        try:
            ctypes.CDLL(name)
            print("  {}: loadable ({})".format(label, name))
            return True
        except OSError as exc:
            failures.append(str(exc))
    print("  {}: NOT LOADABLE by {} Python".format(label, platform.machine()))
    print("    First loader error: {}".format(failures[0]))
    return False


def check_data(data):
    # Import only the existing local predicate, never its downloader main():
    # --dry-run fetches online metadata; --check-only can extract/write files.
    from fetch_data import runtime_ready

    print("  Data: {}".format(data.absolute()))
    if data.is_symlink() and not data.exists():
        print("    Data symlink target is unavailable. The case-sensitive resource image may be unmounted.")
        print("    A normal ./run-macos.sh launch mounts the local image; --check never mounts it.")
    try:
        ready = runtime_ready(data)
    except OSError as exc:
        print("    Cannot inspect data: {}".format(exc))
        return False
    if not ready:
        print("    MISSING or incomplete according to the existing runtime_ready check.")
        print("    Requires script/1001.bnr and 1001.txt; nonempty voice, bgm, se,")
        print("    movie and font directories; a matching bg/sprite art pair at")
        print("    4x, 8x, 2x or original scale. If script_cn exists, it also needs")
        print("    both 1001 files and matching bg_cn art. An .wa2-installing.json")
        print("    marker means incomplete installation. No data was downloaded.")
        print("    Set WA2_DATA to an existing complete data tree; see MACOS.md.")
        return False
    lang = os.environ.get("WA2_LANG", "").lower()
    if lang not in ("", "cn", "zh", "chs", "zh-cn", "jp", "ja", "jpn"):
        print("    Invalid WA2_LANG={!r}; use cn or jp.".format(lang))
        return False
    if lang in ("cn", "zh", "chs", "zh-cn") and not (data / "script_cn").is_dir():
        print("    WA2_LANG requests Chinese but script_cn is missing.")
        return False
    print("    Local layout OK (not a complete inventory/hash or gameplay validation).")
    return True


def report_fonts():
    for key in ("WA2_FONT", "WA2_FONT_HOME", "WA2_FONT_HISTORY", "WA2_FONT_GAME"):
        if key in os.environ:
            print("  {}={!r} (preserved; engine validates font/face)".format(key, os.environ[key]))
    fonts = ROOT / "fonts"
    bundled = [p.name for p in fonts.glob("*") if p.suffix.lower() in (".ttc", ".ttf", ".otf")]
    if bundled:
        print("  Bundled font files: {} (engine validates manifest and glyphs)".format(", ".join(sorted(bundled))))
    else:
        print("  No bundled fonts found. Engine searches native macOS font locations.")
        print("    If necessary set WA2_FONT='/absolute/path/to/CJK font.ttc#face'.")
    print("  Font lookup/selection belongs to the engine; no fonts installed or overrides added.")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--build-only", action="store_true", help="only check build tools")
    mode.add_argument("--smoke", action="store_true", help="check build tools and SDL2, without game data")
    args = parser.parse_args(argv)
    if sys.platform != "darwin":
        print("This checker requires macOS; no Linux launcher/AppImage is used.", file=sys.stderr)
        return 1
    if sys.version_info < (3, 8):
        print("Python 3.8+ is required. If missing: brew install python", file=sys.stderr)
        return 1
    print("macOS local-source checks (offline; no build, install, or data writes):")
    print("  Python: {} ({})".format(sys.version.split()[0], platform.machine()))
    missing = []
    formulas = set()
    for name in ("cargo", "rustc", "cc"):
        path = shutil.which(name)
        print("  {}: {}".format(name, path or "NOT FOUND on PATH"))
        if path is None:
            missing.append(name)
            if name in ("cargo", "rustc"):
                formulas.add("rust")
    print("  Build needs installed Rust 1.90+ and Xcode Command Line Tools.")
    print("    Tool paths only: version/shim activation is not run by this read-only check.")
    if not args.build_only:
        for label in (("SDL2",) if args.smoke else ("SDL2", "FreeType")):
            if not library_available(label):
                missing.append(label)
                formulas.add("sdl2" if label == "SDL2" else "freetype")
        if not args.smoke:
            for name in ("ffmpeg", "ffprobe"):
                path = shutil.which(name)
                print("  {}: {}".format(name, path or "NOT FOUND on PATH"))
                if path is None:
                    missing.append(name)
                    formulas.add("ffmpeg")
            print("  dwebp (optional): {}".format(shutil.which("dwebp") or "not found; ffmpeg fallback"))
            report_fonts()
            if not check_data(Path(os.environ.get("WA2_DATA", "data"))):
                missing.append("game data")
        for name in ("SDL_VIDEODRIVER", "SDL_AUDIODRIVER", "WA2_SOFTWARE"):
            if name in os.environ:
                print("  {}={!r} (explicit user setting preserved)".format(name, os.environ[name]))
        print("  No SDL driver is forced; a real macOS desktop session is needed for a window.")
    if missing:
        print("\nNot ready: {}.".format(", ".join(missing)), file=sys.stderr)
        if formulas:
            print("If these dependencies are actually absent, install only the missing ones manually:", file=sys.stderr)
            print("  brew install {}".format(" ".join(sorted(formulas))), file=sys.stderr)
            print("Existing installations may instead need PATH/library lookup or architecture fixes.", file=sys.stderr)
        if "cc" in missing:
            print("For missing Apple compiler tools, manually run: xcode-select --install", file=sys.stderr)
        print("No package manager or downloader was invoked.", file=sys.stderr)
        return 1
    print("Checks passed for this mode; this is not a full runtime/gameplay test.")
    return 0


if __name__ == "__main__":
    # Match run-macos.sh even if invoked directly from a different directory.
    os.chdir(ROOT)
    raise SystemExit(main())
