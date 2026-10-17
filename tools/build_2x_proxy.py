#!/usr/bin/env python3
"""Build fast-loading `*_2x` TGA proxy trees from the `*_8x` WebP trees.

Background: the engine decodes game art through ffmpeg for WebP.  A lossless
8x background is 10240x5760 (59 MP), which costs ~4 s and ~750 MB to decode.
The renderer only ever needs roughly the window resolution, so for daily play
we keep a 2x-of-logical TGA proxy tree in the data tree:

    data/bg_8x/<stem>.webp       -> data/bg_2x/<stem>.tga      (RLE, 2x logical)
    data/sprite_8x/<stem>.webp   -> data/sprite_2x/<stem>.tga  (RLE, 2x logical)
    data/bg_cn_8x/<stem>.webp    -> data/bg_cn_2x/<stem>.tga   (RLE, 2x logical)

TGA is decoded in-process by the engine (no ffmpeg subprocess), RLE keeps the
files around half the raw size, and the 2x size matches a 2560x1440 window
(the current display is 1920x1080).  The 8x trees stay untouched and remain
selectable with WA2_ART_SCALE=8.

Usage:
    python3 tools/build_2x_proxy.py             # build everything, skip existing
    python3 tools/build_2x_proxy.py --force     # rebuild
    python3 tools/build_2x_proxy.py --trees bg  # only one tree
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
TREES = ("bg", "sprite", "bg_cn")


def source_dims(path: Path) -> tuple[int, int]:
    """Logical (1x) dimensions from the original TGA/BMP header."""
    suffix = path.suffix.lower()
    with open(path, "rb") as f:
        head = f.read(64)
    if suffix == ".tga":
        if len(head) < 18:
            raise ValueError("TGA too short")
        return struct.unpack_from("<HH", head, 12)
    if suffix == ".bmp":
        if len(head) < 26:
            raise ValueError("BMP too short")
        w, h = struct.unpack_from("<ii", head, 18)
        return w, abs(h)
    raise ValueError(f"unsupported source {path}")


def find_source(tree: str, stem: str) -> Path | None:
    for suffix in (".tga", ".TGA", ".bmp", ".BMP"):
        candidate = ROOT / tree / f"{stem}{suffix}"
        if candidate.is_file():
            return candidate
    return None


def convert(task: tuple[str, str, str]) -> tuple[str, str]:
    tree, stem, scale = task
    seconds = int(scale)
    src_hires = ROOT / f"{tree}_8x" / f"{stem}.webp"  # source is always the 8x tree
    src = find_source(tree, stem)
    if src is None:
        return stem, f"SKIP missing original source for {tree}/{stem}"
    out = ROOT / f"{tree}_{seconds}x" / f"{stem}.tga"
    try:
        if out.is_file() and out.stat().st_size > 0:
            return stem, "skip"
        width, height = source_dims(src)
        t = time.perf_counter()
        with Image.open(src_hires) as image:
            image = image.convert("RGBA")
            resized = image.resize((width * seconds, height * seconds), Image.LANCZOS)
        out.parent.mkdir(parents=True, exist_ok=True)
        resized.save(out, compression="tga_rle")
        return stem, f"{time.perf_counter() - t:.2f}s {out.stat().st_size / 1e6:.1f}MB"
    except Exception as exc:  # noqa: BLE001 - keep going, report at the end
        return stem, f"FAIL {type(exc).__name__}: {exc}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--trees", default=",".join(TREES), help="comma separated trees")
    parser.add_argument("--scale", type=int, default=2, choices=(2, 4), help="proxy scale (default 2)")
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    tasks: list[tuple[str, str, str]] = []
    for tree in [t.strip() for t in args.trees.split(",") if t.strip()]:
        hires_dir = ROOT / f"{tree}_8x"  # source tree (8x or mixed 4x/8x)
        if not hires_dir.is_dir():
            print(f"skip {tree}: {hires_dir} does not exist")
            continue
        for path in sorted(hires_dir.glob("*.webp")):
            tasks.append((tree, path.stem, str(args.scale)))

    if args.force:
        for tree, stem, scale in tasks:
            out = ROOT / f"{tree}_{scale}x" / f"{stem}.tga"
            out.unlink(missing_ok=True)

    if not tasks:
        print("nothing to do")
        return 0

    print(f"building {len(tasks)} proxies -> *_2x TGA (RLE), workers={args.workers}")
    t0 = time.perf_counter()
    done = skipped = failed = 0
    failures: list[str] = []
    with ProcessPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(convert, task) for task in tasks]
        for index, future in enumerate(as_completed(futures), 1):
            stem, status = future.result()
            if status == "skip":
                skipped += 1
            elif status.startswith("FAIL") or status.startswith("SKIP"):
                failed += 1
                failures.append(f"{stem}: {status}")
                print(f"  ! {stem}: {status}")
            else:
                done += 1
            if index % 200 == 0 or index == len(tasks):
                elapsed = time.perf_counter() - t0
                print(f"  [{index}/{len(tasks)}] {elapsed:.0f}s done={done} skipped={skipped} failed={failed}")
    elapsed = time.perf_counter() - t0
    print(f"finished in {elapsed/60:.1f} min: {done} built, {skipped} skipped, {failed} failed")
    if failures:
        print("failures:")
        for line in failures[:20]:
            print(" ", line)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
