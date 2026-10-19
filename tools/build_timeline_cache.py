#!/usr/bin/env python3
"""Pre-render timeline thumbnails and write the freshness manifest.

The timeline chart draws one 104x59 thumbnail per scene and a 208x117
preview.  Decoding the full 8x WebP for each of the ~939 pictures takes
minutes (a lossless 10240x5760 background costs ~1.2 s to decode), so this
tool renders small 320x180 TGA thumbnails once.  The engine reads
`data/cache/timeline/<stem>.tga`.

Because the cache outlives asset regenerations, every build also writes
`data/cache/timeline/manifest.tsv`:

    stem  source_path  source_size  source_mtime  source_sha256  cache_size  cache_mtime

source_path is relative to the data root (e.g. `bg_8x/b100101.webp`),
size/mtime are bytes and nanoseconds since the epoch so the runtime can stat
both files and only trust a cache entry whose source is unchanged.

Usage:
    python3 tools/build_timeline_cache.py            # build missing/stale thumbs
    python3 tools/build_timeline_cache.py --force    # rebuild every thumb
    python3 tools/build_timeline_cache.py --verify   # check cache vs manifest
    python3 tools/build_timeline_cache.py --prune    # drop thumbs with no source
    python3 tools/build_timeline_cache.py --verify --hash   # also re-hash sources
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import sys
import time
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor, as_completed
from pathlib import Path

from PIL import Image

# Some 8x masters are slightly over PIL's decompression-bomb limit (91 MP);
# they are trusted local game assets, not downloads.
Image.MAX_IMAGE_PIXELS = None

DATA = Path(__file__).resolve().parent.parent / "data"
CACHE = DATA / "cache" / "timeline"
MANIFEST = CACHE / "manifest.tsv"
PICTURE = re.compile(r"^(?:tv|b|v)\d{6}$")
COLUMNS = [
    "stem",
    "source_path",
    "source_size",
    "source_mtime",
    "source_sha256",
    "cache_size",
    "cache_mtime",
]
SIZE = (320, 180)
# Preferred source tree per stem: Chinese override first, then the highest
# resolution master, then the originals. Runtime may read a different tree
# (4x in windowed mode); the manifest key is the stem, so that is fine.
TREE_ORDER = ["bg_cn_8x", "bg_8x", "bg_cn_4x", "bg_4x", "bg_cn", "bg"]
CHUNK = 1 << 20


def collect_sources() -> dict[str, Path]:
    """stem -> best source file, over the picture-bearing art trees."""
    order = list(TREE_ORDER)
    for tree in sorted(DATA.iterdir()) if DATA.is_dir() else []:
        if tree.is_dir() and tree.name not in order:
            order.append(tree.name)
    found: dict[str, Path] = {}
    for tree_name in order:
        tree = DATA / tree_name
        if not tree.is_dir():
            continue
        try:
            entries = list(tree.iterdir())
        except OSError:
            continue
        for path in entries:
            if not path.is_file():
                continue
            if path.suffix.lower() not in (".webp", ".tga", ".bmp"):
                continue
            stem = path.stem.lower()
            if not PICTURE.match(stem):
                continue
            # TREE_ORDER is priority order; the Chinese patch and the highest
            # scale win, originals only fill gaps.
            found.setdefault(stem, path)
    return found


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            block = handle.read(CHUNK)
            if not block:
                break
            digest.update(block)
    return digest.hexdigest()


def stat_fields(path: Path) -> tuple[int, int]:
    st = path.stat()
    return st.st_size, st.st_mtime_ns


def entry_for(stem: str, source: Path, cache: Path, source_sha: str) -> dict[str, str]:
    source_size, source_mtime = stat_fields(source)
    cache_size, cache_mtime = stat_fields(cache)
    return {
        "stem": stem,
        "source_path": source.relative_to(DATA).as_posix(),
        "source_size": str(source_size),
        "source_mtime": str(source_mtime),
        "source_sha256": source_sha,
        "cache_size": str(cache_size),
        "cache_mtime": str(cache_mtime),
    }


def render(task: tuple[str, str, str]) -> tuple[str, str, dict[str, str] | None, str]:
    """Worker: render one thumbnail, then record its manifest row."""
    stem, source_s, cache_s = task
    source = Path(source_s)
    cache = Path(cache_s)
    tmp = cache.with_name(f"{cache.name}.{os.getpid()}.tmp.tga")
    try:
        cache.parent.mkdir(parents=True, exist_ok=True)
        # Hash first: the sequential read warms the page cache for the decoder
        # and the manifest needs the digest anyway.
        source_sha = sha256_file(source)
        with Image.open(source) as image:
            image = image.convert("RGBA")
            # Reducing-gap lets libwebp/libjpeg-style decoders box-reduce while
            # decoding; straight 10240x5760 -> 320x180 is sampling-dominated.
            image = image.resize(SIZE, Image.LANCZOS, reducing_gap=3.0)
        image.save(tmp, compression="tga_rle")
        os.replace(tmp, cache)
        return stem, "ok", entry_for(stem, source, cache, source_sha), ""
    except Exception as exc:  # noqa: BLE001 - report at the end and keep going
        try:
            tmp.unlink(missing_ok=True)
        except OSError:
            pass
        return stem, f"FAIL {type(exc).__name__}: {exc}", None, ""
    finally:
        try:
            tmp.unlink(missing_ok=True)
        except OSError:
            pass


def load_manifest() -> dict[str, dict[str, str]]:
    if not MANIFEST.is_file():
        return {}
    entries: dict[str, dict[str, str]] = {}
    try:
        text = MANIFEST.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return {}
    for line in text.splitlines():
        if not line or line.startswith("stem\t"):
            continue
        parts = line.split("\t")
        if len(parts) != len(COLUMNS):
            continue
        entries[parts[0]] = dict(zip(COLUMNS, parts))
    return entries


def write_manifest(entries: dict[str, dict[str, str]]) -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    lines = ["\t".join(COLUMNS)]
    for stem in sorted(entries):
        lines.append("\t".join(entries[stem][column] for column in COLUMNS))
    tmp = MANIFEST.with_name(MANIFEST.name + ".tmp")
    tmp.write_text("\n".join(lines) + "\n", encoding="utf-8")
    os.replace(tmp, MANIFEST)


def cache_is_fresh(entry: dict[str, str] | None, source: Path, cache: Path) -> bool:
    if not entry or not cache.is_file():
        return False
    try:
        source_size, source_mtime = stat_fields(source)
        cache_size, cache_mtime = stat_fields(cache)
    except OSError:
        return False
    return (
        entry.get("source_size") == str(source_size)
        and entry.get("source_mtime") == str(source_mtime)
        and entry.get("cache_size") == str(cache_size)
        and entry.get("cache_mtime") == str(cache_mtime)
    )


def build(args: argparse.Namespace) -> int:
    global SIZE
    SIZE = (args.size[0], args.size[1])
    sources = collect_sources()
    if not sources:
        print("no picture files found")
        return 1
    if args.prune:
        prune(sources)
    previous = {} if args.force else load_manifest()
    tasks: list[tuple[str, str, str]] = []
    entries: dict[str, dict[str, str]] = {}
    for stem in sorted(sources):
        source = sources[stem]
        cache = CACHE / f"{stem}.tga"
        if not args.force and cache_is_fresh(previous.get(stem), source, cache):
            entries[stem] = previous[stem]
            continue
        tasks.append((stem, str(source), str(cache)))
    print(f"timeline cache: {len(entries)} fresh, {len(tasks)} to render at {SIZE[0]}x{SIZE[1]}")
    started = time.time()
    built = failed = 0
    failures: list[str] = []
    if tasks:
        with ProcessPoolExecutor(max_workers=args.workers) as pool:
            futures = [pool.submit(render, task) for task in tasks]
            for index, future in enumerate(as_completed(futures), 1):
                stem, status, entry, detail = future.result()
                if entry is not None:
                    entries[stem] = entry
                    built += 1
                else:
                    failed += 1
                    failures.append(f"{stem}: {status} {detail}".strip())
                if index % 100 == 0 or index == len(tasks):
                    elapsed = time.time() - started
                    print(f"  [{index}/{len(tasks)}] {elapsed:.0f}s built={built} failed={failed}")
    elapsed = time.time() - started
    write_manifest(entries)
    cache_bytes = sum((CACHE / f"{stem}.tga").stat().st_size for stem in entries if (CACHE / f"{stem}.tga").is_file())
    print(
        f"finished in {elapsed / 60:.1f} min: {built} built, {len(entries) - built} reused, "
        f"{failed} failed; manifest {MANIFEST} rows={len(entries)} cache={cache_bytes / (1 << 20):.0f} MB"
    )
    if failures:
        for line in failures[:20]:
            print(" ", line)
        return 1
    return 0


def prune(sources: dict[str, Path]) -> None:
    removed = 0
    for cache in sorted(CACHE.glob("*.tga")):
        if cache.stem not in sources:
            cache.unlink(missing_ok=True)
            removed += 1
    entries = {
        stem: entry
        for stem, entry in load_manifest().items()
        if stem in sources and (CACHE / f"{stem}.tga").is_file()
    }
    write_manifest(entries)
    print(f"pruned {removed} orphan thumbnail(s); manifest rows={len(entries)}")


def verify(args: argparse.Namespace) -> int:
    sources = collect_sources()
    entries = load_manifest()
    if not entries:
        print(f"error: {MANIFEST} is missing or empty; run --force", file=sys.stderr)
        return 1
    if not sources:
        print("no picture files found", file=sys.stderr)
        return 1
    problems: list[str] = []
    stale = missing = 0
    for stem in sorted(sources):
        entry = entries.get(stem)
        if entry is None:
            missing += 1
            problems.append(f"{stem}: no manifest entry")
            continue
        cache = CACHE / f"{stem}.tga"
        source = DATA / entry["source_path"]
        if not cache.is_file():
            stale += 1
            problems.append(f"{stem}: cache file missing")
            continue
        try:
            cache_size, cache_mtime = stat_fields(cache)
        except OSError as exc:
            problems.append(f"{stem}: cache stat failed: {exc}")
            continue
        if str(cache_size) != entry.get("cache_size") or str(cache_mtime) != entry.get("cache_mtime"):
            stale += 1
            problems.append(f"{stem}: cache size/mtime does not match manifest")
        if not source.is_file():
            stale += 1
            problems.append(f"{stem}: source {entry['source_path']} missing")
            continue
        source_size, source_mtime = stat_fields(source)
        if str(source_size) != entry.get("source_size") or str(source_mtime) != entry.get("source_mtime"):
            stale += 1
            problems.append(f"{stem}: source size/mtime changed since build")
        elif args.hash and sha256_file(source) != entry.get("source_sha256"):
            stale += 1
            problems.append(f"{stem}: source sha256 changed since build")
    extra = sorted(set(entries) - set(sources))
    for stem in extra:
        problems.append(f"{stem}: manifest entry has no source picture")
    print(
        f"verify: {len(sources)} pictures, {len(entries)} manifest rows, "
        f"{stale} stale, {missing} unlisted, {len(extra)} orphan"
        + (", sha256 checked" if args.hash else "")
    )
    for line in problems[:40]:
        print(" ", line)
    if len(problems) > 40:
        print(f"  ... {len(problems) - 40} more")
    return 0 if not problems else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--size", nargs=2, type=int, default=list(SIZE), metavar=("W", "H"))
    parser.add_argument("--workers", type=int, default=8)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--force", action="store_true", help="rebuild every thumbnail")
    mode.add_argument("--verify", action="store_true", help="check cache against manifest")
    mode.add_argument("--prune", action="store_true", help="drop thumbnails whose source picture is gone")
    parser.add_argument("--hash", action="store_true", help="--verify: re-hash source files (slow)")
    args = parser.parse_args()
    if args.verify:
        return verify(args)
    return build(args)


if __name__ == "__main__":
    sys.exit(main())
