#!/usr/bin/env python3
"""HTTP API for Real-ESRGAN x4 followed by Waifu2x x2.

Run beside the two NCNN directories::

    python3 -m uvicorn upscale_api:app --host 0.0.0.0 --port 8000

The endpoint accepts TGA/BMP/PNG/JPEG/WebP and returns a lossless WebP.
The source alpha channel is scaled separately and restored after both models.
"""

from __future__ import annotations

import os
import subprocess
import tempfile
import threading
from pathlib import Path
from typing import Optional

from fastapi import FastAPI, File, Header, HTTPException, UploadFile
from fastapi.responses import Response
from PIL import Image

from upscale_art import decode, write_png


ROOT = Path(__file__).resolve().parent
REAL_BIN = Path(os.environ.get("REALESRGAN_BIN", ROOT / "realesrgan-ncnn-vulkan" / "realesrgan-ncnn-vulkan"))
REAL_MODELS = Path(os.environ.get("REALESRGAN_MODELS", ROOT / "realesrgan-ncnn-vulkan" / "models"))
WAIFU_BIN = Path(os.environ.get("WAIFU_BIN", ROOT / "waifu2x-ncnn-vulkan" / "waifu2x-ncnn-vulkan"))
WAIFU_MODELS = Path(os.environ.get("WAIFU_MODELS", ROOT / "waifu2x-ncnn-vulkan" / "models-cunet"))
GPU = os.environ.get("WA2_GPU", "0")
TILE = os.environ.get("WA2_TILE", "128")
REAL_JOBS = os.environ.get("WA2_REAL_JOBS", "4:8:4")
WAIFU_JOBS = os.environ.get("WA2_WAIFU_JOBS", "4:8:4")
API_TOKEN = os.environ.get("WA2_API_TOKEN", "")
WORKERS = max(1, int(os.environ.get("WA2_WORKERS", "3")))
TMP_ROOT = Path(os.environ.get("WA2_TMP", "/root/autodl-tmp/wa2-api-tmp"))

app = FastAPI(title="WA2 upscale API", version="1.0")
GPU_SLOTS = threading.BoundedSemaphore(WORKERS)


def authorized(authorization: Optional[str], x_api_key: Optional[str]) -> bool:
    if not API_TOKEN:
        return True
    return authorization == f"Bearer {API_TOKEN}" or x_api_key == API_TOKEN


def load_rgba(src: Path):
    if src.suffix.lower() in {".tga", ".bmp"}:
        width, height, rgba = decode(str(src))
        return width, height, rgba
    with Image.open(src) as image:
        rgba_image = image.convert("RGBA")
        return rgba_image.width, rgba_image.height, rgba_image.tobytes()


def run_model(command: list[str], label: str) -> None:
    try:
        # NCNN prints one progress line per tile. Capturing that stream without
        # draining it can fill the pipe and deadlock concurrent API requests.
        proc = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    except OSError as exc:
        raise RuntimeError(f"{label} cannot start: {exc}") from exc
    if proc.returncode != 0:
        raise RuntimeError(f"{label} failed with exit code {proc.returncode}")


def process_image(upload: Path, output: Path) -> tuple[int, int]:
    width, height, rgba = load_rgba(upload)
    alpha = Image.frombytes("L", (width, height), bytes(rgba[3::4]))
    input_png = upload.parent / "input.png"
    real4 = upload.parent / "real4.webp"
    waifu2 = output
    write_png(str(input_png), rgba, width, height)

    run_model([
        str(REAL_BIN), "-i", str(input_png), "-o", str(real4),
        "-m", str(REAL_MODELS), "-n", "realesrgan-x4plus-anime",
        "-s", "4", "-f", "webp", "-g", GPU, "-t", TILE, "-j", REAL_JOBS,
    ], "Real-ESRGAN x4")
    run_model([
        str(WAIFU_BIN), "-i", str(real4), "-o", str(waifu2),
        "-m", str(WAIFU_MODELS), "-n", "-1", "-s", "2",
        "-t", TILE, "-g", GPU, "-j", WAIFU_JOBS,
    ], "Waifu2x x2")

    with Image.open(waifu2) as image:
        result = image.convert("RGB")
    result.putalpha(alpha.resize(result.size, Image.Resampling.LANCZOS))
    result.save(waifu2, "WEBP", lossless=True, method=6)
    return result.width, result.height


@app.get("/health")
def health():
    return {"ok": True, "pipeline": "realesrgan-x4plus-anime -> waifu2x-x2", "gpu": GPU}


@app.post("/v1/upscale")
def upscale(
    file: UploadFile = File(...),
    authorization: Optional[str] = Header(default=None),
    x_api_key: Optional[str] = Header(default=None),
):
    if not authorized(authorization, x_api_key):
        raise HTTPException(status_code=401, detail="invalid API token")
    if not file.filename:
        raise HTTPException(status_code=400, detail="missing filename")
    suffix = Path(file.filename).suffix.lower() or ".bin"
    if suffix not in {".tga", ".bmp", ".png", ".jpg", ".jpeg", ".webp"}:
        raise HTTPException(status_code=415, detail="unsupported image format")

    TMP_ROOT.mkdir(parents=True, exist_ok=True)
    # Several independent NCNN processes keep a high-end GPU busier than one
    # image at a time. The semaphore limits VRAM pressure without serializing
    # all requests.
    with GPU_SLOTS, tempfile.TemporaryDirectory(prefix="job-", dir=TMP_ROOT) as job_dir:
        job = Path(job_dir)
        source = job / ("source" + suffix)
        output = job / "result.webp"
        source.write_bytes(file.file.read())
        try:
            width, height = process_image(source, output)
        except Exception as exc:
            raise HTTPException(status_code=500, detail=str(exc)) from exc
        body = output.read_bytes()
    return Response(
        content=body,
        media_type="image/webp",
        headers={"X-WA2-Width": str(width), "X-WA2-Height": str(height), "X-WA2-Scale": "8"},
    )
