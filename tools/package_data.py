#!/usr/bin/env python3
"""Package the loose game data tree into split release assets.

The output is a set of ``*.tar.zst.partNNN`` volumes (each below GitHub's 2 GiB
per-asset limit), plus ``manifest.json`` and ``SHA256SUMS``.  Concatenating the
volumes reproduces one zstd-compressed tar stream:

    cat wa2-data-v1.0.0.part*.tar.zst | zstd -d | tar -xf - -C .

Only the trees needed at runtime are included; derived caches
(``data/cache``, ``data/*_2x`` proxy trees) are intentionally skipped and can be
regenerated with ``tools/build_ui_cache.py`` / ``tools/build_timeline_cache.py``.

Example::

    python3 tools/package_data.py --version data-v1.0.0
    python3 tools/package_data.py --version data-v1.0.0 --with-4x \
        --out release/data-v1.0.0-4x
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_TREES = [
    "script",
    "script_cn",
    "bg",
    "sprite",
    "bg_cn",
    "voice",
    "bgm",
    "se",
    "font",
    "movie",
    "movie_2x",
    "bg_8x",
    "sprite_8x",
    "bg_cn_8x",
]
OPTIONAL_4X = ["bg_4x", "sprite_4x", "bg_cn_4x"]


def human(n: int) -> str:
    return f"{n / 1e9:.2f} GB"


def tree_inventory(data_dir: Path, trees: list[str]) -> dict:
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
            if entry.is_file():
                files += 1
                size += entry.stat().st_size
        per_tree[tree] = {"files": files, "bytes": size}
        total_files += files
        total_bytes += size
    return {"trees": per_tree, "files": total_files, "bytes": total_bytes}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--data", default=str(ROOT / "data"))
    parser.add_argument("--version", default="data-v1.0.0")
    parser.add_argument("--out", default="")
    parser.add_argument("--part-size-mib", type=int, default=1800,
                        help="max size per part (GitHub allows 2048 MiB per asset)")
    parser.add_argument("--zstd-level", type=int, default=3)
    parser.add_argument("--trees", default=",".join(DEFAULT_TREES),
                        help="comma separated trees under data/ (default: runtime trees)")
    parser.add_argument("--with-4x", action="store_true",
                        help="also include the optional bg_4x/sprite_4x/bg_cn_4x trees")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    data_dir = Path(args.data).expanduser().resolve()
    trees = [t.strip() for t in args.trees.split(",") if t.strip()]
    if args.with_4x:
        trees += OPTIONAL_4X
    out_dir = Path(args.out).expanduser().resolve() if args.out else ROOT / "release" / args.version
    out_dir.mkdir(parents=True, exist_ok=True)
    part_size = args.part_size_mib * 1024 * 1024
    inv = tree_inventory(data_dir, trees)
    print(f"version : {args.version}")
    print(f"output  : {out_dir}")
    print(f"trees   : {', '.join(trees)}")
    print(f"input   : {inv['files']} files, {human(inv['bytes'])}")
    if args.dry_run:
        print("dry-run: nothing written")
        return 0

    tar_cmd = ["tar", "-cf", "-", "-C", str(data_dir.parent), "--sort=name"]
    tar_cmd += [f"data/{t}" for t in trees]
    print("tar     :", " ".join(tar_cmd[:6]), "...")
    zstd_cmd = ["zstd", f"-{args.zstd_level}", "-T0", "-q", "-c"]
    tar_proc = subprocess.Popen(tar_cmd, stdout=subprocess.PIPE)
    zstd_proc = subprocess.Popen(zstd_cmd, stdin=tar_proc.stdout, stdout=subprocess.PIPE)
    assert tar_proc.stdout is not None and zstd_proc.stdout is not None
    tar_proc.stdout.close()  # let zstd own the only read end

    shas: dict[str, str] = {}
    parts: list[dict] = []
    started = time.time()
    index = 1
    fh = None
    hasher = None
    part_bytes = 0
    total_out = 0

    def open_part() -> None:
        nonlocal fh, hasher, part_bytes, index
        name = f"{args.version}.part{index:03d}.tar.zst"
        fh = open(out_dir / name, "wb")
        hasher = hashlib.sha256()
        part_bytes = 0
        print(f"  -> {name}", flush=True)

    def close_part() -> None:
        nonlocal fh, hasher, index
        if fh is None:
            return
        fh.close()
        name = f"{args.version}.part{index:03d}.tar.zst"
        digest = hasher.hexdigest()
        shas[name] = digest
        parts.append({"name": name, "size": part_bytes, "sha256": digest})
        fh = None
        hasher = None
        index += 1

    open_part()
    try:
        while True:
            chunk = zstd_proc.stdout.read(8 * 1024 * 1024)
            if not chunk:
                break
            offset = 0
            while offset < len(chunk):
                room = part_size - part_bytes
                piece = chunk[offset:offset + room]
                fh.write(piece)
                hasher.update(piece)
                part_bytes += len(piece)
                total_out += len(piece)
                offset += len(piece)
                if part_bytes >= part_size:
                    close_part()
                    open_part()
            if time.time() - started > 5:
                print(f"  {human(total_out)} compressed, {time.time() - started:.0f}s", flush=True)
                started = time.time()
    finally:
        close_part()
        zstd_proc.wait()
        tar_proc.wait()
        zstd_proc.stdout.close()
    if tar_proc.returncode not in (0, 1):  # 1 = "changed as we read it" warning
        raise SystemExit(f"tar failed (rc={tar_proc.returncode})")
    if zstd_proc.returncode != 0:
        raise SystemExit(f"zstd failed (rc={zstd_proc.returncode})")

    sums_path = out_dir / "SHA256SUMS"
    sums_path.write_text("".join(f"{p['sha256']}  {p['name']}\n" for p in parts), encoding="utf-8")
    manifest = {
        "format": "wa2-data-release/1",
        "version": args.version,
        "created_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "generator": "tools/package_data.py",
        "compression": f"zstd -{args.zstd_level}",
        "part_size_bytes": part_size,
        "trees": trees,
        "inventory": inv,
        "archive_bytes": total_out,
        "extract": "cat " + args.version + ".part*.tar.zst | zstd -d | tar -xf - -C .",
        "parts": parts,
    }
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print()
    print(f"done: {len(parts)} parts, {human(total_out)} total")
    print(f"  {out_dir / 'manifest.json'}")
    print(f"  {out_dir / 'SHA256SUMS'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
