#!/usr/bin/env python3
"""Pre-decode the high-resolution art trees into the runtime art cache.

The 8x WebP trees are the only high-quality source, but decoding a 59 MP
lossless WebP costs about a second, which is far too slow to do while the
player is waiting for a scene change. The engine decodes each picture once and
writes the already-scaled result to

    data/cache/art/<scale>x/<tree>/<stem>.tga

so every later load is a plain file read. This tool builds that cache in bulk,
which removes the one-off wait on the first visit to each picture - the prefetch
thread inside the engine can only hide the ones it sees coming.

Sizes to build (one per common window size):

    1x  1280x720    ~3.7 MB per background
    2x  1920x1080   ~8.3 MB per background
    4x  2560x1440   ~15 MB per background

Usage:
    python3 tools/build_art_cache.py                 # 1x, whatever is missing
    python3 tools/build_art_cache.py --scale 2
    python3 tools/build_art_cache.py --scale 1 --scale 2
    python3 tools/build_art_cache.py --limit 200     # stop after 200 files
"""

from __future__ import annotations

import argparse
import struct
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "data"
TREES = ("bg_8x", "sprite_8x", "bg_cn_8x")


def image_dims(path: Path) -> tuple[int, int] | None:
    """Width and height from a TGA / BMP / WebP header - never decodes pixels."""
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
            return 1 + int.from_bytes(head[24:27], "little"), 1 + int.from_bytes(head[27:30], "little")
        if kind == b"VP8L" and head[20] == 0x2F:
            bits = int.from_bytes(head[21:25], "little")
            return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1
    return None


def parse_pam(data: bytes) -> tuple[int, int, bytes] | None:
    """Split a PAM RGBA frame written by dwebp into (width, height, pixels)."""
    if not data.startswith(b"P7\n"):
        return None
    end = data.find(b"ENDHDR\n")
    if end < 0:
        return None
    header = data[: end + 7].decode("latin-1")
    end += 7
    width = height = depth = None
    for line in header.splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue
        if parts[0] == "WIDTH":
            width = int(parts[1])
        elif parts[0] == "HEIGHT":
            height = int(parts[1])
        elif parts[0] == "DEPTH":
            depth = int(parts[1])
    if width is None or height is None or depth != 4:
        return None
    return width, height, data[end : end + width * height * 4]


def write_tga(dst: Path, width: int, height: int, rgba: bytes) -> None:
    """32-bit uncompressed TGA, top-down, BGRA - exactly what codec::tga reads."""
    header = bytes([0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]) + struct.pack(
        "<HHBB", width, height, 32, 0x28
    )
    rows = bytearray(width * height * 4)
    rows[0::4] = rgba[2::4]
    rows[1::4] = rgba[1::4]
    rows[2::4] = rgba[0::4]
    rows[3::4] = rgba[3::4]
    dst.parent.mkdir(parents=True, exist_ok=True)
    tmp = dst.with_suffix(dst.suffix + f".{__import__('os').getpid()}.tmp")
    with open(tmp, "wb") as file:
        file.write(header)
        file.write(rows)
    tmp.replace(dst)


def decode_scaled(src: Path, width: int, height: int) -> tuple[int, int, bytes] | None:
    """Decode src straight to (width, height) RGBA, resampling inside libwebp."""
    with tempfile.NamedTemporaryFile(suffix=".pam", delete=False) as handle:
        temp = Path(handle.name)
    try:
        result = subprocess.run(
            ["dwebp", "-quiet", "-pam", "-resize", str(width), str(height), str(src), "-o", str(temp)],
            capture_output=True,
        )
        if result.returncode == 0:
            parsed = parse_pam(temp.read_bytes())
            if parsed is not None:
                return parsed
    except OSError:
        pass
    finally:
        temp.unlink(missing_ok=True)
    # Fallback: let ffmpeg do the same job.
    try:
        result = subprocess.run(
            [
                "ffmpeg", "-v", "error", "-i", str(src),
                "-vf", f"scale={width}:{height}",
                "-frames:v", "1", "-f", "image2pipe", "-c:v", "pam", "-pix_fmt", "rgba", "-",
            ],
            capture_output=True,
        )
        if result.returncode == 0:
            return parse_pam(result.stdout)
    except OSError:
        pass
    return None


def source_scale(src: Path) -> int:
    """The scale the file is actually stored at (8x trees hold a few 4x files)."""
    actual = image_dims(src)
    base_tree = src.parent.name
    for suffix in ("_8x", "_4x", "_2x"):
        if base_tree.endswith(suffix):
            base_tree = base_tree[: -len(suffix)]
            break
    else:
        return 1
    base_dir = src.parent.parent / base_tree
    for ext in (".tga", ".webp", ".bmp", ".TGA", ".WEBP", ".BMP"):
        candidate = base_dir / (src.stem + ext)
        if candidate.is_file():
            original = image_dims(candidate)
            if original and actual and original[0]:
                ratio = actual[0] / original[0]
                return min((8, 4, 2, 1), key=lambda value: abs(ratio - value))
            break
    return 1


def build(task: tuple[Path, Path, int, int, bool]) -> tuple[str, str]:
    src, dst, target, source, force = task
    if not force and dst.is_file() and dst.stat().st_mtime >= src.stat().st_mtime:
        return src.stem, "skip"
    actual = image_dims(src)
    if actual is None:
        return src.stem, "FAIL unreadable source"
    width = max(1, actual[0] * target // source)
    height = max(1, actual[1] * target // source)
    decoded = decode_scaled(src, width, height)
    if decoded is None:
        return src.stem, "FAIL decode"
    got_w, got_h, rgba = decoded
    if len(rgba) < got_w * got_h * 4:
        return src.stem, "FAIL truncated decode"
    write_tga(dst, got_w, got_h, rgba[: got_w * got_h * 4])
    return src.stem, "ok"


def collect(scales: list[int], force: bool, only: str | None) -> list[tuple[Path, Path, int, int, bool]]:
    tasks: list[tuple[Path, Path, int, int, bool]] = []
    for tree in TREES:
        src_dir = ROOT / tree
        if not src_dir.is_dir():
            continue
        for src in sorted(src_dir.glob("*.webp")):
            if only and only not in src.stem:
                continue
            source = source_scale(src)
            if source <= 1:
                continue
            for target in scales:
                if target >= source:
                    continue
                dst = ROOT / "cache" / "art" / f"{target}x" / tree / f"{src.stem}.tga"
                if force:
                    dst.unlink(missing_ok=True)
                tasks.append((src, dst, target, source, force))
    return tasks


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--scale", type=int, action="append", default=None,
                        help="target scale to build (repeatable, default 1)")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--limit", type=int, default=0, help="stop after this many files")
    parser.add_argument("--only", default=None, help="only stems containing this text")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    scales = sorted(set(args.scale or [1]))
    tasks = collect(scales, args.force, args.only)
    if args.limit:
        tasks = tasks[: args.limit]
    if not tasks:
        print("nothing to build (cache already complete for these scales)")
        return 0
    print(f"building {len(tasks)} cached images at {', '.join(f'{s}x' for s in scales)}")
    started = time.time()
    built = skipped = failed = 0
    failures: list[str] = []
    with ProcessPoolExecutor(max_workers=args.workers) as pool:
        futures = [pool.submit(build, task) for task in tasks]
        for index, future in enumerate(as_completed(futures), 1):
            stem, status = future.result()
            if status == "skip":
                skipped += 1
            elif status == "ok":
                built += 1
            else:
                failed += 1
                failures.append(f"{stem}: {status}")
            if index % 25 == 0 or index == len(tasks):
                print(f"  [{index}/{len(tasks)}] {time.time() - started:.0f}s "
                      f"built={built} skipped={skipped} failed={failed}")
    print(f"finished in {time.time() - started:.1f}s: {built} built, {skipped} skipped, {failed} failed")
    for line in failures[:20]:
        print(" ", line)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
