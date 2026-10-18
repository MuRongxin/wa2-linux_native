#!/usr/bin/env python3
"""Download and restore the WA2 data release from GitHub Releases.

Usage (private repository needs a token with `repo` scope)::

    GITHUB_TOKEN=ghp_xxx python3 tools/fetch_data.py \
        --repo MuRongxin/wa2-linux_native --tag data-v1.0.0 --dest .

The release must contain ``manifest.json``, ``SHA256SUMS`` and one or more
``<version>.partNNN.tar.zst`` assets produced by ``tools/package_data.py``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

API = "https://api.github.com"
PARTS_DIR_NAME = ".wa2-release-parts"


def api_get(url: str, token: str, accept: str = "application/vnd.github+json") -> bytes:
    headers = {"User-Agent": "wa2-fetch-data", "Accept": accept}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=120) as resp:
        return resp.read()


def api_json(url: str, token: str) -> dict:
    return json.loads(api_get(url, token))


def find_release(repo: str, tag: str, token: str) -> dict:
    try:
        return api_json(f"{API}/repos/{repo}/releases/tags/{tag}", token)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            raise SystemExit(f"release {tag!r} not found in {repo}") from exc
        raise


def asset_content(asset: dict, token: str) -> bytes:
    if asset.get("url") and token:
        return api_get(asset["url"], token, accept="application/octet-stream")
    return api_get(asset["browser_download_url"], token, accept="application/octet-stream")


def download_asset(asset: dict, dest: Path, token: str) -> None:
    expected_size = int(asset["size"])
    expected_sha = ""
    part = dest.with_suffix(dest.suffix + ".part")
    if dest.is_file() and dest.stat().st_size == expected_size:
        print(f"  reuse {dest.name} ({expected_size / 1e9:.2f} GB)")
        return
    print(f"  download {dest.name} ({expected_size / 1e9:.2f} GB)")
    url = asset["url"] if token else asset["browser_download_url"]
    headers = {"User-Agent": "wa2-fetch-data", "Accept": "application/octet-stream"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, headers=headers)
    started = time.time()
    done = 0
    with urllib.request.urlopen(req, timeout=300) as resp, open(part, "wb") as out:
        while True:
            chunk = resp.read(8 * 1024 * 1024)
            if not chunk:
                break
            out.write(chunk)
            done += len(chunk)
            if time.time() - started > 5:
                speed = done / max(time.time() - started, 1e-9) / 1e6
                print(f"    {done / 1e9:.2f}/{expected_size / 1e9:.2f} GB ({speed:.1f} MB/s)", flush=True)
                started = time.time()
    if part.stat().st_size != expected_size:
        part.unlink(missing_ok=True)
        raise SystemExit(f"{dest.name}: size mismatch")
    part.replace(dest)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(8 * 1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def verify_manifest_files(dest: Path, manifest: dict) -> bool:
    ok = True
    for tree, info in manifest.get("inventory", {}).get("trees", {}).items():
        root = dest / "data" / tree
        files = 0
        size = 0
        if root.is_dir():
            for entry in root.rglob("*"):
                if entry.is_file():
                    files += 1
                    size += entry.stat().st_size
        good = files == info["files"] and size == info["bytes"]
        print(f"  {tree:12s} files {files:5d}/{info['files']:<5d} bytes {size}/{info['bytes']} {'OK' if good else 'MISMATCH'}")
        ok = ok and good
    return ok


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default="MuRongxin/wa2-linux_native")
    parser.add_argument("--tag", default="data-v1.0.0")
    parser.add_argument("--dest", default=".")
    parser.add_argument("--token", default=os.environ.get("GITHUB_TOKEN", ""))
    parser.add_argument("--parts-dir", default="",
                        help="where to keep downloaded parts (default <dest>/.wa2-release-parts/<tag>)")
    parser.add_argument("--check-only", action="store_true", help="only verify/extract already-downloaded parts")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    dest = Path(args.dest).expanduser().resolve()
    dest.mkdir(parents=True, exist_ok=True)
    parts_dir = Path(args.parts_dir).expanduser().resolve() if args.parts_dir else dest / PARTS_DIR_NAME / args.tag
    parts_dir.mkdir(parents=True, exist_ok=True)
    repo = args.repo.strip()

    print(f"release: {repo} @ {args.tag}")
    release = find_release(repo, args.tag, args.token)
    assets = {a["name"]: a for a in release.get("assets", [])}
    if "manifest.json" not in assets:
        raise SystemExit("release has no manifest.json asset")
    manifest = json.loads(asset_content(assets["manifest.json"], args.token))
    parts = manifest["parts"]
    print(f"manifest: {manifest['version']} | {manifest['inventory']['files']} files, "
          f"{manifest['inventory']['bytes'] / 1e9:.2f} GB | {len(parts)} parts")
    if args.dry_run:
        for part in parts:
            print(f"  {part['name']} {part['size'] / 1e9:.2f} GB {part['sha256']}")
        return 0

    if not args.check_only:
        for part in parts:
            asset = assets.get(part["name"])
            if not asset:
                raise SystemExit(f"missing asset in release: {part['name']}")
            target = parts_dir / part["name"]
            download_asset(asset, target, args.token)
    # verify parts
    for part in parts:
        target = parts_dir / part["name"]
        if not target.is_file() or target.stat().st_size != part["size"]:
            raise SystemExit(f"part missing/incomplete: {target}")
        digest = sha256_file(target)
        if digest != part["sha256"]:
            target.unlink()
            raise SystemExit(f"sha256 mismatch for {target.name} (deleted, re-run)")
    print("all parts verified")

    print(f"extracting into {dest} ...")
    zstd = shutil.which("zstd")
    tar = shutil.which("tar")
    if not zstd or not tar:
        raise SystemExit("zstd and tar must be installed")
    cmd = f"cat {parts_dir}/{manifest['version']}.part*.tar.zst | zstd -d | tar -xf - -C {dest}"
    subprocess.run(["sh", "-c", cmd], check=True)
    print("verifying extracted tree ...")
    ok = verify_manifest_files(dest, manifest)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
