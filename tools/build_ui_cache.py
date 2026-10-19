#!/usr/bin/env python3
"""Pre-render logical-size UI sheets from the scaled WebP trees.

UI code (title, settings, save/load, command bar, music mode) draws in the
1280x720 logical coordinate space, so a high-resolution sheet such as
`sys_00000` (9984x1664) is decoded and then immediately reduced to 1248x208.
This tool renders that logical image once into `data/cache/ui/<tree>/<stem>.tga`;
`codec::image::decode_ui_file` prefers the cached TGA and falls back to
decoding the WebP when the file is missing.

Both the 8x master tree and the 4x tree are covered: the engine reads the 4x
tree by default, so it needs a cache under `cache/ui/bg_4x` / `bg_cn_4x`.

Usage:
    python3 tools/build_ui_cache.py            # build missing sheets
    python3 tools/build_ui_cache.py --force    # rebuild all
"""

from __future__ import annotations

import argparse
import struct
import sys
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent / "data"
TREES = ("bg", "bg_cn")
# Scale trees to pre-render, highest first. Missing trees are skipped.
SCALES = (8, 4)


def is_ui_asset(stem: str) -> bool:
    lower = stem.lower()
    return lower.startswith("sys_") or lower.startswith("t") or lower == "weather"


def image_dims(path: Path) -> tuple[int, int] | None:
    try:
        with open(path, "rb") as file:
            head = file.read(64)
    except OSError:
        return None
    suffix = path.suffix.lower()
    if suffix == ".tga" and len(head) >= 18:
        return struct.unpack_from("<HH", head, 12)
    if suffix == ".bmp" and len(head) >= 26:
        width, height = struct.unpack_from("<ii", head, 18)
        return width, abs(height)
    if suffix == ".webp" and len(head) >= 30 and head[:4] == b"RIFF" and head[8:12] == b"WEBP":
        kind = head[12:16]
        if kind == b"VP8X":
            width = 1 + int.from_bytes(head[24:27], "little")
            height = 1 + int.from_bytes(head[27:30], "little")
            return width, height
        if kind == b"VP8L" and head[20] == 0x2F:
            bits = int.from_bytes(head[21:25], "little")
            return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1
    return None


def render(task: tuple[Path, Path, str, int]) -> tuple[str, str]:
    src, dst, base_stem, tree_scale = task
    if dst.is_file() and dst.stat().st_size > 0:
        return base_stem, "skip"
    try:
        base = None
        for suffix in (".tga", ".TGA", ".bmp", ".BMP"):
            candidate = ROOT / task_tree(src) / f"{src.stem}{suffix}"
            if candidate.is_file():
                base = candidate
                break
        if base is None:
            return base_stem, "FAIL no original asset"
        original = image_dims(base)
        actual = image_dims(src)
        if original is None or actual is None:
            return base_stem, "FAIL unreadable dimensions"
        scale = round(actual[0] / original[0]) if original[0] else tree_scale
        logical = max(1, actual[0] // scale), max(1, actual[1] // scale)
        with Image.open(src) as image:
            image = image.convert("RGBA")
            if image.size != logical:
                image = image.resize(logical, Image.LANCZOS)
        dst.parent.mkdir(parents=True, exist_ok=True)
        image.save(dst, compression="tga_rle")
        return base_stem, "ok"
    except Exception as exc:  # noqa: BLE001 - report and keep going
        return base_stem, f"FAIL {type(exc).__name__}: {exc}"


def task_tree(path: Path) -> str:
    name = path.parent.name.lower()
    for suffix in ("_8x", "_4x", "_2x"):
        if name.endswith(suffix):
            return name[: -len(suffix)]
    return name


def collect(force: bool) -> list[tuple[Path, Path, str, int]]:
    tasks: list[tuple[Path, Path, str, int]] = []
    for tree in TREES:
        for tree_scale in SCALES:
            hires = ROOT / f"{tree}_{tree_scale}x"
            if not hires.is_dir():
                continue
            for path in sorted(hires.glob("*.webp")):
                if not is_ui_asset(path.stem):
                    continue
                dst = ROOT / "cache" / "ui" / f"{tree}_{tree_scale}x" / f"{path.stem}.tga"
                if force:
                    dst.unlink(missing_ok=True)
                tasks.append((path, dst, path.stem, tree_scale))
    return tasks


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    tasks = collect(args.force)
    if not tasks:
        print("no UI sheets found")
        return 0
    print(f"rendering {len(tasks)} logical-size UI sheets")
    started = time.time()
    built = skipped = failed = 0
    failures: list[str] = []
    with ProcessPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(render, task) for task in tasks]
        for index, future in enumerate(as_completed(futures), 1):
            stem, status = future.result()
            if status == "skip":
                skipped += 1
            elif status == "ok":
                built += 1
            else:
                failed += 1
                failures.append(f"{stem}: {status}")
            if index % 40 == 0 or index == len(tasks):
                print(f"  [{index}/{len(tasks)}] {time.time() - started:.0f}s built={built} skipped={skipped} failed={failed}")
    print(f"finished in {time.time() - started:.1f}s: {built} built, {skipped} skipped, {failed} failed")
    if failures:
        for line in failures[:20]:
            print(" ", line)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
