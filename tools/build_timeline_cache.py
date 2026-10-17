#!/usr/bin/env python3
"""Pre-render timeline thumbnails from the high-resolution asset trees.

The timeline chart draws one 104x58 thumbnail per scene and a 208x117
preview. Decoding the full 8x WebP for each of the ~379 scenes takes minutes
(a lossless 10240x5760 background costs ~1.2 s to decode), so the chart would
feel broken. This tool renders small 320x180 TGA thumbnails once; the engine
prefers them in `data/cache/timeline/<stem>.tga` and falls back to decoding the
full asset only when a cached file is missing.

Usage:
    python3 tools/build_timeline_cache.py            # build missing thumbs
    python3 tools/build_timeline_cache.py --force    # rebuild all
    python3 tools/build_timeline_cache.py --size 512 288
"""

from __future__ import annotations

import argparse
import re
import sys
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent / "data"
PICTURE = re.compile(r"^(?:tv|b|v)\d{6}$")


def collect(size: tuple[int, int], force: bool) -> list[tuple[Path, Path]]:
    tasks: list[tuple[Path, Path]] = []
    for tree in sorted(ROOT.glob("*_8x")):
        if not tree.is_dir():
            continue
        out_dir = ROOT / "cache" / "timeline"
        for path in tree.glob("*.webp"):
            if not PICTURE.match(path.stem):
                continue
            out = out_dir / f"{path.stem}.tga"
            if force:
                out.unlink(missing_ok=True)
            tasks.append((path, out))
    return tasks


def render(task: tuple[Path, Path]) -> tuple[str, str]:
    src, dst = task
    if dst.is_file() and dst.stat().st_size > 0:
        return src.stem, "skip"
    try:
        with Image.open(src) as image:
            image = image.convert("RGBA")
            # Downscaling a 10240x5760 frame straight to 320x180 is dominated
            # by sampling 59 M source pixels; box-reduce first for a ~3x win.
            image = image.resize(SIZE, Image.LANCZOS, reducing_gap=3.0)
        dst.parent.mkdir(parents=True, exist_ok=True)
        image.save(dst, compression="tga_rle")
        return src.stem, "ok"
    except Exception as exc:  # noqa: BLE001 - report at the end and keep going
        return src.stem, f"FAIL {type(exc).__name__}: {exc}"


SIZE = (320, 180)


def main() -> int:
    global SIZE
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--size", nargs=2, type=int, default=list(SIZE))
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    SIZE = (args.size[0], args.size[1])
    tasks = collect(SIZE, args.force)
    if not tasks:
        print("no picture files found")
        return 0
    print(f"rendering {len(tasks)} timeline thumbnails at {SIZE[0]}x{SIZE[1]}")
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
            if index % 100 == 0 or index == len(tasks):
                elapsed = time.time() - started
                print(f"  [{index}/{len(tasks)}] {elapsed:.0f}s built={built} skipped={skipped} failed={failed}")
    elapsed = time.time() - started
    print(f"finished in {elapsed / 60:.1f} min: {built} built, {skipped} skipped, {failed} failed")
    if failures:
        for line in failures[:20]:
            print(" ", line)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
