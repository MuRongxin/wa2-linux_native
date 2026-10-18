#!/usr/bin/env python3
"""Send WA2 TGA assets to the remote 4x Real-ESRGAN + 2x Waifu2x API.

Results stay on this machine in bg_8x/ and sprite_8x/. Existing valid
outputs are skipped, so the same command can be resumed after interruption.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import http.client
import os
import ssl
import sys
import time
import uuid
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
from urllib.parse import urlsplit

from PIL import Image

from upscale_art import decode


DEFAULT_ROOT = "/home/lrxin/文档/wa2-upscale"
DEFAULT_API = "https://u109205-b0d8-d570bb6f.westb.seetacloud.com:8443/v1/upscale"
# 当前云实例的默认连接配置；仍可通过环境变量覆盖。
DEFAULT_API_TOKEN = "wa2-local-5090"
DEFAULT_API_IP = "36.103.198.205"


def output_for(root: Path, src: Path) -> Path:
    folder = src.parent.name
    return root / f"{folder}_8x" / f"{src.stem}.webp"


def done(src: Path, dst: Path) -> bool:
    if not dst.is_file():
        return False
    try:
        w, h, _ = decode(str(src))
        with Image.open(dst) as im:
            return im.size == (w * 8, h * 8) and im.format == "WEBP"
    except Exception:
        return False


def post_image(api: str, token: str, src: Path, timeout: int) -> bytes:
    boundary = ("----wa2" + uuid.uuid4().hex).encode("ascii")
    payload = src.read_bytes()
    body = (
        b"--" + boundary + b"\r\n"
        + (b'Content-Disposition: form-data; name="file"; filename="'
           + src.name.encode("utf-8") + b'"\r\n')
        + b"Content-Type: application/octet-stream\r\n\r\n"
        + payload + b"\r\n--" + boundary + b"--\r\n"
    )
    fixed_ip = os.environ.get("WA2_API_IP", DEFAULT_API_IP).strip()
    if fixed_ip:
        parsed = urlsplit(api)
        port = parsed.port or (443 if parsed.scheme == "https" else 80)
        headers = {
            "Authorization": "Bearer " + token,
            "Content-Type": "multipart/form-data; boundary=" + boundary.decode("ascii"),
            "Content-Length": str(len(body)),
            "Host": parsed.hostname or "",
        }
        context = ssl._create_unverified_context() if parsed.scheme == "https" else None
        conn = http.client.HTTPSConnection(fixed_ip, port, timeout=timeout, context=context)
        try:
            conn.request("POST", parsed.path or "/", body=body, headers=headers)
            response = conn.getresponse()
            data = response.read()
            if response.status != 200:
                raise RuntimeError(f"API HTTP {response.status}: {data[:500]!r}")
            return data
        finally:
            conn.close()
    req = Request(api, data=body, method="POST")
    req.add_header("Authorization", "Bearer " + token)
    req.add_header("Content-Type", "multipart/form-data; boundary=" + boundary.decode("ascii"))
    req.add_header("Content-Length", str(len(body)))
    with urlopen(req, timeout=timeout) as response:
        return response.read()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=os.environ.get("WA2_ROOT", DEFAULT_ROOT))
    ap.add_argument("--api", default=os.environ.get("WA2_API", DEFAULT_API))
    ap.add_argument("--token", default=os.environ.get("WA2_API_TOKEN", DEFAULT_API_TOKEN))
    ap.add_argument("--only", default="bg,sprite", help="comma-separated folders")
    ap.add_argument("--match", default="")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--timeout", type=int, default=1800)
    ap.add_argument("--concurrency", type=int, default=int(os.environ.get("WA2_CONCURRENCY", "3")),
                    help="同时提交的图片数（应与远端 API workers 对齐）")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    if not args.token and not args.dry_run:
        print("missing --token or WA2_API_TOKEN", file=sys.stderr)
        return 2

    root = Path(args.root)
    sources = []
    for folder in [x.strip() for x in args.only.split(",") if x.strip()]:
        src_dir = root / folder
        if not src_dir.is_dir():
            print("skip missing", src_dir)
            continue
        sources.extend(sorted(src_dir.glob("*.tga")))
    if args.match:
        sources = [p for p in sources if args.match in str(p)]

    pending = [(p, output_for(root, p)) for p in sources if not done(p, output_for(root, p))]
    print("sources", len(sources), "pending", len(pending))
    if args.dry_run:
        for src, dst in pending[:20]:
            print(src, "->", dst)
        return 0
    if args.limit:
        pending = pending[:args.limit]

    def process_one(index, src, dst):
        dst.parent.mkdir(parents=True, exist_ok=True)
        partial = dst.with_suffix(".partial.webp")
        print(f"[{index}/{len(pending)}] {src.relative_to(root)}", flush=True)
        error = None
        for attempt in range(1, 4):
            try:
                data = post_image(args.api, args.token, src, args.timeout)
                partial.write_bytes(data)
                if not done(src, partial):
                    raise RuntimeError("API returned invalid dimensions or WebP")
                os.replace(partial, dst)
                print("  ok", dst, flush=True)
                error = None
                break
            except (HTTPError, URLError, OSError, RuntimeError) as exc:
                error = exc
                print(f"  retry {attempt}: {exc}", file=sys.stderr, flush=True)
                time.sleep(2 * attempt)
        if error is not None:
            print("  FAILED", src, error, file=sys.stderr, flush=True)
        return error is None

    concurrency = max(1, args.concurrency)
    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        futures = [
            pool.submit(process_one, index, src, dst)
            for index, (src, dst) in enumerate(pending, 1)
        ]
        ok = sum(1 for future in as_completed(futures) if future.result())
    print("completed", ok, "/", len(pending))
    return 0 if ok == len(pending) else 1


if __name__ == "__main__":
    raise SystemExit(main())
