#!/usr/bin/env python3
"""Create a GitHub Release and upload the packaged data volumes.

Requires a personal access token with `repo` scope (classic) or
`Contents: write` (fine-grained) and, for copyrighted game assets, a
**private** repository.  The script refuses public repositories unless
``--allow-public`` is passed (publishing the game data publicly is a
copyright violation).

Typical flow::

    python3 tools/package_data.py --version data-v1.0.0
    GITHUB_TOKEN=ghp_xxx python3 tools/upload_release.py \
        --repo MuRongxin/wa2-linux_native --tag data-v1.0.0 \
        --dir release/data-v1.0.0 --notes-file docs/RELEASE_DATA.md

The release is created as a draft, all assets are uploaded, and it is
published at the end so users never see a half-uploaded release.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

API = "https://api.github.com"
UPLOADS = "uploads.github.com"


def api(method: str, path: str, token: str, payload: dict | None = None) -> dict:
    data = None
    headers = {"User-Agent": "wa2-upload-release", "Accept": "application/vnd.github+json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if payload is not None:
        data = json.dumps(payload).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(API + path, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as exc:
        print(exc.read().decode("utf-8", "replace")[:1000], file=sys.stderr)
        raise


def get_release(repo: str, tag: str, token: str) -> dict | None:
    req = urllib.request.Request(
        f"{API}/repos/{repo}/releases/tags/{tag}",
        headers={"User-Agent": "wa2-upload-release", "Authorization": f"Bearer {token}"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return None
        raise


def upload_asset(repo: str, release_id: int, path: Path, token: str) -> None:
    size = path.stat().st_size
    quoted = urllib.parse.quote(path.name)
    print(f"  upload {path.name} ({size / 1e9:.2f} GB)")
    conn = http.client.HTTPSConnection(UPLOADS, timeout=600)
    try:
        conn.putrequest("POST", f"/repos/{repo}/releases/{release_id}/assets?name={quoted}")
        conn.putheader("Authorization", f"Bearer {token}")
        conn.putheader("User-Agent", "wa2-upload-release")
        conn.putheader("Content-Type", "application/octet-stream")
        conn.putheader("Content-Length", str(size))
        conn.endheaders()
        sent = 0
        started = time.time()
        with open(path, "rb") as fh:
            while True:
                chunk = fh.read(8 * 1024 * 1024)
                if not chunk:
                    break
                conn.send(chunk)
                sent += len(chunk)
                if time.time() - started > 10:
                    speed = sent / max(time.time() - started, 1e-9) / 1e6
                    print(f"    {sent / 1e9:.2f}/{size / 1e9:.2f} GB ({speed:.1f} MB/s)", flush=True)
                    started = time.time()
        resp = conn.getresponse()
        body = resp.read()
        if resp.status not in (200, 201):
            raise RuntimeError(f"upload failed HTTP {resp.status}: {body[:500]!r}")
    finally:
        conn.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default="MuRongxin/wa2-linux_native")
    parser.add_argument("--tag", default="data-v1.0.0")
    parser.add_argument("--title", default="")
    parser.add_argument("--notes-file", default="")
    parser.add_argument("--dir", required=True, help="directory with manifest.json / SHA256SUMS / parts")
    parser.add_argument("--token", default=os.environ.get("GITHUB_TOKEN", ""))
    parser.add_argument("--allow-public", action="store_true",
                        help="allow publishing on a public repository (copyright risk!)")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--publish", action="store_true", help="publish the release after uploading")
    args = parser.parse_args()
    if not args.token and not args.dry_run:
        raise SystemExit("GITHUB_TOKEN (repo scope) is required")

    root = Path(args.dir).expanduser().resolve()
    manifest_path = root / "manifest.json"
    if not manifest_path.is_file():
        raise SystemExit(f"missing {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    part_names = [p["name"] for p in manifest["parts"]]
    files = [root / "manifest.json", root / "SHA256SUMS"] + [root / n for n in part_names]
    missing = [str(f) for f in files if not f.is_file()]
    if missing:
        raise SystemExit(f"missing release files: {missing}")
    body = Path(args.notes_file).read_text(encoding="utf-8") if args.notes_file else manifest.get("extract", "")
    title = args.title or f"WA2 data {manifest['version']}"
    print(f"repo  : {args.repo}")
    print(f"tag   : {args.tag}")
    print(f"assets: {len(files)} files, {sum(f.stat().st_size for f in files) / 1e9:.2f} GB")

    if not args.dry_run:
        repo_info = api("GET", f"/repos/{args.repo}", args.token)
        is_private = bool(repo_info.get("private"))
        if not is_private:
            # A draft release is NOT visible to the public, even in a public
            # repository, so it is safe to upload assets into a draft and make
            # the repository private before publishing.  Only the final
            # --publish step needs the private repository (or an explicit
            # override for the copyright risk).
            print(
                "WARNING: repository is public. Assets will be uploaded to a DRAFT "
                "release (not publicly visible); publishing requires a private "
                "repository unless --allow-public is passed."
            )
        release = get_release(args.repo, args.tag, args.token)
        if release is None:
            release = api("POST", f"/repos/{args.repo}/releases", args.token, {
                "tag_name": args.tag,
                "name": title,
                "body": body,
                "draft": True,
            })
            print(f"created draft release id={release['id']}")
        else:
            print(f"using existing release id={release['id']} (draft={release['draft']})")
        existing = {a["name"] for a in release.get("assets", [])}
        for path in files:
            if path.name in existing:
                print(f"  skip existing {path.name}")
                continue
            upload_asset(args.repo, release["id"], path, args.token)
        if args.publish:
            if not is_private and not args.allow_public:
                raise SystemExit(
                    "REFUSED to publish: repository is still public. Game assets are "
                    "copyrighted; make the repository private first (the draft release "
                    "with all uploaded assets is kept) or pass --allow-public."
                )
            api("PATCH", f"/repos/{args.repo}/releases/{release['id']}", args.token, {"draft": False})
            print(f"published release {args.tag}")
        else:
            print("release left as draft; re-run with --publish when ready")
    else:
        for f in files:
            print(f"  would upload {f.name} ({f.stat().st_size / 1e9:.2f} GB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
