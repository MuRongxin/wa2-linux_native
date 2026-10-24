#!/usr/bin/env python3
"""Package the loose game data tree into split release assets.

Default format is **independent ZIP parts**: every ``.partNNN.zip`` is a complete
archive that extracts straight into ``./data/...``, so Windows Explorer, macOS
Archive Utility and Linux ``unzip`` can all open it by double click / one
command.  The ``tarzst`` format is kept as an option for Linux-first mirrors
(one concatenated zstd stream, needs ``cat ... | zstd -d | tar -xf -``).

Only the trees needed at runtime are included; derived caches are intentionally
skipped and can be regenerated with ``tools/build_ui_cache.py`` /
``tools/build_timeline_cache.py``.

Examples::

    python3 tools/package_data.py --version data-v1.1.0        # 2x/4x/8x pack
    python3 tools/package_data.py --version data-v1.1.0 --with-1x-art
"""

from __future__ import annotations

import argparse
import hashlib
import json
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNTIME_TREES = [
    "script",
    "script_cn",
    "voice",
    "bgm",
    "se",
    "font",
    "movie",
    "movie_2x",
]
# Keep the 1x background directory only for the small .AMP colour curves. The
# actual 1x pictures are superseded by the scaled siblings below.
DEFAULT_TREES = RUNTIME_TREES + [
    "bg",
    "bg_2x",
    "sprite_2x",
    "bg_cn_2x",
    "bg_4x",
    "sprite_4x",
    "bg_cn_4x",
    "bg_8x",
    "sprite_8x",
    "bg_cn_8x",
]
LEGACY_1X_ART_TREES = {"bg", "sprite", "bg_cn"}
OPTIONAL_4X = ["bg_4x", "sprite_4x", "bg_cn_4x"]
# Already-compressed formats: store them instead of wasting CPU on deflate.
STORE_EXTS = {
    ".webp", ".wmv", ".avi", ".mp4", ".mkv", ".ogg", ".mp3", ".m4a", ".aac",
    ".jpg", ".jpeg", ".png", ".zip", ".zst", ".gz", ".xz", ".7z", ".rar",
}
DEFLATE_LEVEL = 6


def human(n: int) -> str:
    return f"{n / 1e9:.2f} GB"


def include_file(tree: str, path: Path, include_1x_art: bool) -> bool:
    if include_1x_art or tree not in LEGACY_1X_ART_TREES:
        return True
    # These curves are still used by the runtime for ambient and explicit
    # colour filters, even though the 1x image pixels are no longer needed.
    return path.suffix.lower() == ".amp"


def tree_inventory(data_dir: Path, trees: list[str], include_1x_art: bool) -> dict:
    total_files = 0
    total_bytes = 0
    per_tree = {}
    for tree in trees:
        path = data_dir / tree
        if not path.is_dir():
            raise SystemExit(f"missing data tree: {path}")
        files = 0
        size = 0
        for entry in path.rglob("*"):
            if entry.is_file() and include_file(tree, entry, include_1x_art):
                files += 1
                size += entry.stat().st_size
        per_tree[tree] = {"files": files, "bytes": size}
        total_files += files
        total_bytes += size
    return {"trees": per_tree, "files": total_files, "bytes": total_bytes}


def zip_safe_name(arcname: str, src: Path) -> str:
    """ZIP entry names must be valid Unicode; some archive-extracted filenames
    still carry raw CP932 bytes, which Python exposes as surrogates."""
    try:
        arcname.encode("utf-8")
        return arcname
    except UnicodeEncodeError:
        raw = arcname.encode("utf-8", "surrogateescape")
        for encoding in ("cp932", "shift_jis"):
            try:
                fixed = raw.decode(encoding)
                print(f"  note: zip name {raw!r} -> {fixed!r}", flush=True)
                return fixed
            except UnicodeDecodeError:
                continue
        print(f"  warning: dropping unencodable entry {src}", flush=True)
        return ""


def collect_files(data_dir: Path, trees: list[str], include_1x_art: bool) -> list[tuple[Path, str, int]]:
    entries = []
    for tree in trees:
        for path in sorted((data_dir / tree).rglob("*")):
            if path.is_file() and include_file(tree, path, include_1x_art):
                arcname = f"data/{tree}/{path.relative_to(data_dir / tree).as_posix()}"
                safe = zip_safe_name(arcname, path)
                if not safe:
                    continue
                entries.append((path, safe, path.stat().st_size))
    entries.sort(key=lambda item: item[1])
    return entries


def write_zip_parts(entries, out_dir: Path, version: str, budget: int, dry_run: bool) -> tuple[list[dict], int]:
    parts: list[dict] = []
    started = time.time()
    index = 1
    part_entries: list[tuple[Path, str, int]] = []
    part_bytes = 0

    def flush() -> None:
        nonlocal index, part_entries, part_bytes
        if not part_entries:
            return
        name = f"{version}.part{index:03d}.zip"
        target = out_dir / name
        if dry_run:
            print(f"  would write {name}: {len(part_entries)} files, input {human(part_bytes)}")
        else:
            with zipfile.ZipFile(target, "w", allowZip64=True) as zf:
                for src, arcname, _size in part_entries:
                    compress = (
                        zipfile.ZIP_STORED
                        if src.suffix.lower() in STORE_EXTS
                        else zipfile.ZIP_DEFLATED
                    )
                    if compress == zipfile.ZIP_DEFLATED:
                        zf.write(src, arcname, compress_type=compress, compresslevel=DEFLATE_LEVEL)
                    else:
                        zf.write(src, arcname, compress_type=compress)
            digest = sha256_file(target)
            size = target.stat().st_size
            parts.append({
                "name": name,
                "size": size,
                "sha256": digest,
                "files": len(part_entries),
                "bytes": part_bytes,
            })
            print(f"  -> {name} ({len(part_entries)} files, {human(part_bytes)} input, {human(size)} zip)", flush=True)
        index += 1
        part_entries = []
        part_bytes = 0

    for entry in entries:
        src, arcname, size = entry
        if size > budget:
            # A single huge file (none in practice) still gets its own part.
            flush()
            part_entries = [entry]
            part_bytes = size
            flush()
            continue
        if part_bytes + size > budget:
            flush()
        part_entries.append(entry)
        part_bytes += size
        if time.time() - started > 10:
            print(f"  ... {len(part_entries)} files buffered, {human(part_bytes)}", flush=True)
            started = time.time()
    flush()
    return parts, sum(p["size"] for p in parts)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(8 * 1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--data", default=str(ROOT / "data"))
    parser.add_argument("--version", default="data-v1.1.0")
    parser.add_argument("--out", default="")
    parser.add_argument("--format", choices=["zip", "tarzst"], default="zip")
    parser.add_argument("--part-size-mib", type=int, default=1750,
                        help="input-bytes budget per part (keeps the zip below GitHub's 2048 MiB limit)")
    parser.add_argument("--trees", default=",".join(DEFAULT_TREES))
    parser.add_argument("--with-4x", action="store_true",
                        help="legacy flag; 4x trees are included by default")
    parser.add_argument("--with-1x-art", action="store_true",
                        help="also include original 1x bg/sprite/bg_cn pixels")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    data_dir = Path(args.data).expanduser().resolve()
    trees = [t.strip() for t in args.trees.split(",") if t.strip()]
    if args.with_4x:
        for tree in OPTIONAL_4X:
            if tree not in trees:
                trees.append(tree)
    if args.with_1x_art:
        for tree in ("sprite", "bg_cn"):
            if tree not in trees:
                trees.append(tree)
    out_dir = Path(args.out).expanduser().resolve() if args.out else ROOT / "release" / args.version
    out_dir.mkdir(parents=True, exist_ok=True)
    budget = args.part_size_mib * 1024 * 1024
    inv = tree_inventory(data_dir, trees, args.with_1x_art)
    entries = collect_files(data_dir, trees, args.with_1x_art)
    print(f"version : {args.version}")
    print(f"format  : {args.format}")
    print(f"output  : {out_dir}")
    print(f"trees   : {', '.join(trees)}")
    print(f"input   : {inv['files']} files, {human(inv['bytes'])}")
    if args.dry_run:
        print("dry-run: nothing written")
        return 0
    if args.format != "zip":
        raise SystemExit("only --format zip is currently supported by this script")

    parts, archive_bytes = write_zip_parts(entries, out_dir, args.version, budget, args.dry_run)
    sums_path = out_dir / "SHA256SUMS"
    sums_path.write_text("".join(f"{p['sha256']}  {p['name']}\n" for p in parts), encoding="utf-8")
    manifest = {
        "format": "wa2-data-release/2",
        "version": args.version,
        "created_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "generator": "tools/package_data.py",
        "container": "zip-parts",
        "include_1x_art": args.with_1x_art,
        "tree_filters": {} if args.with_1x_art else {"bg": {"suffixes": [".amp"]}},
        "part_size_budget_bytes": budget,
        "trees": trees,
        "inventory": inv,
        "archive_bytes": archive_bytes,
        "extract": {
            "linux": "for f in " + args.version + ".part*.zip; do unzip -oq \"$f\" -d .; done",
            "macos": "for f in " + args.version + ".part*.zip; do unzip -oq \"$f\" -d .; done"
                     "   (or double-click each part into the same folder)",
            "windows": "select all parts -> 7-Zip -> Extract to the same folder"
                       "   (Explorer: extract each zip to the same folder)",
            "check": "sha256sum -c SHA256SUMS",
        },
        "parts": parts,
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print()
    print(f"done: {len(parts)} zip parts, {human(archive_bytes)} total")
    print(f"  {out_dir / 'manifest.json'}")
    print(f"  {out_dir / 'SHA256SUMS'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
