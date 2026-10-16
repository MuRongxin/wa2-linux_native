#!/usr/bin/env python3
"""Batch-upscale WA2 pictures with the settled waifu2x settings.

    waifu2x-ncnn-vulkan -g 1 -n -1 -s 8 -x -t 0 -f webp

cunet, no denoise, 8x, TTA, automatic tile size, lossless WebP, NVIDIA GPU.
The six-sprite trial that fixed these flags is the reference.

What is included (TGA and BMP under these directories):

    sprite/   standing art
    bg/       backgrounds, event CGs, effect frames, UI, title art,
              timeline thumbnails (tv*)
    bg_cn/    Chinese UI overrides

What is left alone:

    font/     glyph atlases (dialogue and menus use the vector font)
    save/     slot thumbnails the engine writes itself
    *.amp *.ani *.psh *.scc   curves and animation, not pictures
    audio, scripts, movies

waifu2x reads jpg/png/webp only. Each source is decoded to a temporary PNG
(the TGA path matches the engine, including depth-0 32-bit sprites and the
indexed color key) and then upscaled. Sources are never modified.

Each source folder is written to a sibling named with the scale. `data/bg/`
goes to `data/bg_8x/`, `data/sprite/` to `data/sprite_8x/`, `data/bg_cn/` to
`data/bg_cn_8x/`. The file name keeps its stem and becomes `.webp`. An output
whose size is already scale times the source, and whose source has not been
modified since it was written, is skipped, so the job can be stopped and
continued. One batch is processed per waifu2x process; if a process crashes,
only that batch needs to be retried.

    python3 upscale_art.py --dry-run          # Linux
    python upscale_art.py --check             # Windows: run.bat or python
    ./run.sh   or   run.bat

Windows 10 cloud: the Linux waifu2x binary in an old pack will not run. Extract
the data/script, then drop in the official Windows zip from nihui/waifu2x-ncnn-vulkan
(release 20250915) as waifu2x-ncnn-vulkan\\waifu2x-ncnn-vulkan.exe with models
alongside it. Install Python 3.10+ and use run.bat (--gpu 0 --workers 3).

Throughput: on a 4070-class card, one waifu2x process cannot keep the GPU busy;
use --workers 3 (default). Each worker gets -j 2:4:4 when workers>1 unless you
override --jobs. Each process receives 8 images by default so waifu2x can
overlap loading, GPU processing, and WebP encoding; lower WA2_BATCH/--batch if
temporary disk usage or VRAM is tight. Old 4GB mobile GPUs: WA2_WORKERS=1 or
--workers 1.

A detailed log is appended to upscale.log next to this script. The terminal
shows one line per image; the file also keeps the source header, the exact
command, and waifu2x's own output, so a bad frame can be traced after the run.

The engine still loads TGA. This script only writes the WebP folders.
"""

import argparse
import errno
import os
import shlex
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import traceback
import zlib
from concurrent.futures import ProcessPoolExecutor, as_completed

IMAGE_EXTS = (".tga", ".bmp")
FOLDERS = ("sprite", "bg", "bg_cn")

def default_binary():
    """Prefer ./waifu2x-ncnn-vulkan beside this script (Windows cloud layout)."""
    here = os.path.dirname(os.path.abspath(__file__))
    for name in ("waifu2x-ncnn-vulkan.exe", "waifu2x-ncnn-vulkan"):
        p = os.path.join(here, "waifu2x-ncnn-vulkan", name)
        if os.path.isfile(p):
            return p
    if sys.platform == "win32":
        return os.path.join(here, "waifu2x-ncnn-vulkan", "waifu2x-ncnn-vulkan.exe")
    return os.path.expanduser("~/文档/waifu2x-ncnn-vulkan/waifu2x-ncnn-vulkan")


def default_data():
    return os.environ.get("WA2_DATA", "data")


def iter_sources(data, only, match):
    found = []
    folders = tuple(only) if only else FOLDERS
    for folder in folders:
        base = os.path.join(data, folder)
        if not os.path.isdir(base):
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames.sort()
            for name in filenames:
                ext = os.path.splitext(name)[1].lower()
                if ext not in IMAGE_EXTS:
                    continue
                path = os.path.join(dirpath, name)
                rel = os.path.relpath(path, data)
                if match and match not in rel:
                    continue
                found.append(rel)
    found.sort()
    return found


def image_size(path):
    """Width and height from the header. TGA height is stored bottom-up."""
    ext = os.path.splitext(path)[1].lower()
    with open(path, "rb") as f:
        if ext == ".tga":
            hdr = f.read(18)
            if len(hdr) < 18:
                raise ValueError("TGA header truncated")
            w, h = struct.unpack_from("<HH", hdr, 12)
            return w, h
        if ext == ".bmp":
            hdr = f.read(26)
            if len(hdr) < 26 or hdr[:2] != b"BM":
                raise ValueError("not a BMP")
            w, h = struct.unpack_from("<ii", hdr, 18)
            return w, abs(h)
    raise ValueError("unsupported image")


def decode(path):
    ext = os.path.splitext(path)[1].lower()
    with open(path, "rb") as f:
        data = f.read()
    if ext == ".tga":
        return decode_tga(data)
    if ext == ".bmp":
        return decode_bmp(data)
    raise ValueError("unsupported image")


def decode_tga(data):
    """Same rules as src/codec/tga.rs decode() (not the font-atlas path)."""
    if len(data) < 18:
        raise ValueError("TGA too short")
    id_len = data[0]
    cmap_type = data[1]
    img_type = data[2]
    cmap_start, cmap_len = struct.unpack_from("<HH", data, 3)
    cmap_depth = data[7]
    w, h = struct.unpack_from("<HH", data, 12)
    depth = data[16]
    desc = data[17]
    if w == 0 or h == 0:
        raise ValueError("TGA zero dimension")
    pos = 18 + id_len
    cmap = None
    if cmap_type != 0 and cmap_len > 0:
        cb = max(cmap_depth // 8, 1)
        total = cmap_len * cb
        if pos + total > len(data):
            raise ValueError("TGA colormap out of bounds")
        cmap = (cmap_start, cmap_depth, data[pos:pos + total])
        pos += total

    base_type = img_type & 0x7
    if depth == 0 and base_type == 2:
        pixel_bytes = 4
    else:
        pixel_bytes = max(depth // 8, 1)
    expected = w * h * pixel_bytes
    rle = img_type in (9, 10, 11)
    if rle:
        raw = bytearray()
        while len(raw) < expected and pos < len(data):
            header = data[pos]
            pos += 1
            packets = (header & 0x7F) + 1
            if header & 0x80:
                if pos + pixel_bytes > len(data):
                    break
                px = data[pos:pos + pixel_bytes]
                pos += pixel_bytes
                raw.extend(px * packets)
            else:
                need = packets * pixel_bytes
                if pos + need > len(data):
                    break
                raw.extend(data[pos:pos + need])
                pos += need
        raw = bytes(raw)
    else:
        if pos + expected > len(data):
            raise ValueError(f"short TGA payload ({len(data) - pos} < {expected})")
        raw = data[pos:pos + expected]
    if len(raw) < expected:
        raise ValueError(f"short TGA pixels ({len(raw)} < {expected})")

    # These are the overwhelmingly common WA2 image formats.  The old
    # fallback below assigns one Python slice per pixel, which makes a 1280
    # x 720 background take about a second before waifu2x even starts.  Row
    # slices with extended-slice assignment run the channel shuffles in C and
    # keep the exact same BGRA/BGR -> RGBA conversion.
    if base_type == 2 and pixel_bytes in (3, 4):
        return _decode_tga_truecolor(raw, w, h, pixel_bytes, desc)
    if base_type == 1 and pixel_bytes == 1 and cmap is not None:
        return _decode_tga_indexed(raw, w, h, desc, cmap)

    rgba = bytearray(w * h * 4)
    flip = (desc & 0x20) == 0
    o = 0
    for y in range(h):
        sy = (h - 1 - y) if flip else y
        for x in range(w):
            idx = (sy * w + x) * pixel_bytes
            if base_type == 1:
                if cmap is None:
                    raise ValueError("indexed TGA without colormap")
                cstart, cdepth, table = cmap
                cb = max(cdepth // 8, 1)
                ci = raw[idx]
                if ci == 0:
                    pixel = (0, 0, 0, 0)
                else:
                    entry = max(ci - cstart, 0) * cb
                    if entry + cb <= len(table):
                        r, g, b = table[entry + 2], table[entry + 1], table[entry]
                        # Engine color key: index 0 and black are transparent.
                        pixel = (0, 0, 0, 0) if (r, g, b) == (0, 0, 0) else (r, g, b, 255)
                    else:
                        pixel = (0, 0, 0, 255)
            elif base_type == 2 and pixel_bytes == 2:
                v = raw[idx] | (raw[idx + 1] << 8)
                r = ((v >> 10) & 0x1F) * 255 // 31
                g = ((v >> 5) & 0x1F) * 255 // 31
                b = (v & 0x1F) * 255 // 31
                pixel = (r, g, b, 255)
            elif base_type == 2 and pixel_bytes == 4:
                pixel = (raw[idx + 2], raw[idx + 1], raw[idx], raw[idx + 3])
            elif base_type == 2:
                pixel = (raw[idx + 2], raw[idx + 1], raw[idx], 255)
            elif base_type == 3:
                v = raw[idx]
                pixel = (v, v, v, 255)
            else:
                raise ValueError(f"unsupported TGA type {img_type}")
            rgba[o:o + 4] = bytes(pixel)
            o += 4
    return w, h, bytes(rgba)


def _decode_tga_truecolor(raw, w, h, pixel_bytes, desc):
    """Decode 24/32-bit true-color TGA rows without a Python pixel loop."""
    rgba = bytearray(w * h * 4)
    flip_y = (desc & 0x20) == 0
    flip_x = (desc & 0x10) != 0
    src_stride = w * pixel_bytes
    dst_stride = w * 4
    alpha = b"\xff" * w
    for y in range(h):
        sy = h - 1 - y if flip_y else y
        src = raw[sy * src_stride:(sy + 1) * src_stride]
        if flip_x:
            # This origin is not present in the shipped assets, but handling
            # it here keeps the fast path faithful to the TGA descriptor.
            src = b"".join(
                src[x * pixel_bytes:(x + 1) * pixel_bytes]
                for x in range(w - 1, -1, -1)
            )
        dst_at = y * dst_stride
        dst = rgba[dst_at:dst_at + dst_stride]
        dst[0::4] = src[2::pixel_bytes]
        dst[1::4] = src[1::pixel_bytes]
        dst[2::4] = src[0::pixel_bytes]
        dst[3::4] = src[3::4] if pixel_bytes == 4 else alpha
        rgba[dst_at:dst_at + dst_stride] = dst
    return w, h, bytes(rgba)


def _decode_tga_indexed(raw, w, h, desc, cmap):
    """Decode the 8-bit keyed UI TGAs using a palette lookup per row."""
    cstart, cdepth, table = cmap
    cb = max(cdepth // 8, 1)
    lut = []
    for ci in range(256):
        if ci == 0:
            pixel = b"\x00\x00\x00\x00"
        else:
            entry = max(ci - cstart, 0) * cb
            if entry + cb <= len(table):
                r, g, b = table[entry + 2], table[entry + 1], table[entry]
                pixel = b"\x00\x00\x00\x00" if (r, g, b) == (0, 0, 0) else bytes((r, g, b, 255))
            else:
                pixel = b"\x00\x00\x00\xff"
        lut.append(pixel)

    flip_y = (desc & 0x20) == 0
    flip_x = (desc & 0x10) != 0
    rgba = bytearray(w * h * 4)
    for y in range(h):
        sy = h - 1 - y if flip_y else y
        row = raw[sy * w:(sy + 1) * w]
        if flip_x:
            row = row[::-1]
        rgba[y * w * 4:(y + 1) * w * 4] = b"".join(lut[ci] for ci in row)
    return w, h, bytes(rgba)


def decode_bmp(data):
    if len(data) < 54 or data[:2] != b"BM":
        raise ValueError("not a BMP")
    off = struct.unpack_from("<I", data, 10)[0]
    dib = struct.unpack_from("<I", data, 14)[0]
    w, h = struct.unpack_from("<ii", data, 18)
    bpp = struct.unpack_from("<H", data, 28)[0]
    comp = struct.unpack_from("<I", data, 30)[0]
    if comp != 0:
        raise ValueError(f"compressed BMP (compression {comp})")
    top_down = h < 0
    h = abs(h)
    if w <= 0 or h <= 0:
        raise ValueError("BMP zero dimension")
    if bpp not in (8, 24, 32):
        raise ValueError(f"BMP bit depth {bpp}")
    palette = b""
    if bpp == 8:
        colors = struct.unpack_from("<I", data, 46)[0] if dib >= 36 else 0
        if colors == 0:
            colors = 256
        pal_at = 14 + dib
        palette = data[pal_at:pal_at + colors * 4]
    stride = ((w * bpp + 31) // 32) * 4
    need = off + stride * h
    if need > len(data):
        raise ValueError(f"short BMP ({len(data)} < {need})")
    rgba = bytearray(w * h * 4)
    for y in range(h):
        sy = y if top_down else (h - 1 - y)
        row = data[off + sy * stride:off + (sy + 1) * stride]
        dest = y * w * 4
        if bpp == 8:
            for x in range(w):
                entry = row[x] * 4
                if entry + 2 >= len(palette):
                    pixel = (0, 0, 0, 255)
                else:
                    pixel = (palette[entry + 2], palette[entry + 1], palette[entry], 255)
                rgba[dest:dest + 4] = bytes(pixel)
                dest += 4
        elif bpp in (24, 32):
            pixel_bytes = bpp // 8
            src = row[:w * pixel_bytes]
            dst = rgba[dest:dest + w * 4]
            dst[0::4] = src[2::pixel_bytes]
            dst[1::4] = src[1::pixel_bytes]
            dst[2::4] = src[0::pixel_bytes]
            dst[3::4] = src[3::4] if bpp == 32 else b"\xff" * w
            rgba[dest:dest + w * 4] = dst
    return w, h, bytes(rgba)


def write_png(path, rgba, w, h):
    n = w * h
    opaque = rgba[3::4] == b"\xff" * n
    if opaque:
        raw = bytearray((w * 3 + 1) * h)
        for y in range(h):
            src_at = y * w * 4
            dst_at = y * (w * 3 + 1)
            raw[dst_at] = 0
            src = rgba[src_at:src_at + w * 4]
            dst = raw[dst_at + 1:dst_at + 1 + w * 3]
            dst[0::3] = src[0::4]
            dst[1::3] = src[1::4]
            dst[2::3] = src[2::4]
            raw[dst_at + 1:dst_at + 1 + w * 3] = dst
        color_type = 2
    else:
        raw = bytearray((w * 4 + 1) * h)
        for y in range(h):
            src_at = y * w * 4
            dst_at = y * (w * 4 + 1)
            raw[dst_at] = 0
            raw[dst_at + 1:dst_at + 1 + w * 4] = rgba[src_at:src_at + w * 4]
        color_type = 6

    def chunk(tag, payload):
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, color_type, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 1))
    png += chunk(b"IEND", b"")
    tmp = path + ".partial"
    with open(tmp, "wb") as f:
        f.write(png)
    os.replace(tmp, path)


def webp_size(path):
    with open(path, "rb") as f:
        hdr = f.read(12)
        if len(hdr) < 12 or hdr[:4] != b"RIFF" or hdr[8:12] != b"WEBP":
            return None
        while True:
            ch = f.read(8)
            if len(ch) < 8:
                return None
            tag = ch[:4]
            size = struct.unpack("<I", ch[4:])[0]
            payload = f.read(size)
            if len(payload) < size:
                return None
            if size & 1:
                f.read(1)
            if tag == b"VP8X" and len(payload) >= 10:
                w = 1 + int.from_bytes(payload[4:7], "little")
                h = 1 + int.from_bytes(payload[7:10], "little")
                return w, h
            if tag == b"VP8L" and len(payload) >= 5 and payload[0] == 0x2F:
                bits = int.from_bytes(payload[1:5], "little")
                w = (bits & 0x3FFF) + 1
                h = ((bits >> 14) & 0x3FFF) + 1
                return w, h
            if tag == b"VP8 " and len(payload) >= 10:
                sig = payload.find(b"\x9d\x01\x2a")
                if sig >= 0 and sig + 6 <= len(payload):
                    w = struct.unpack_from("<H", payload, sig + 3)[0] & 0x3FFF
                    h = struct.unpack_from("<H", payload, sig + 5)[0] & 0x3FFF
                    return w, h


def dest_for(data, rel, scale):
    """data/bg/a.tga -> data/bg_8x/a.webp, beside the source folder."""
    folder, rest = rel.split(os.sep, 1)
    stem, _ext = os.path.splitext(rest)
    return os.path.join(data, f"{folder}_{scale}x", stem + ".webp")


def install_output(src, dest):
    """Install a completed WebP even when temp and data are on different drives.

    ``os.replace`` is atomic, but Windows rejects it across volumes (the
    normal case when TEMP is on C: and the project is on D:).  Copy into a
    partial file beside the final destination, then replace within that
    destination volume.  A same-volume move keeps the fast atomic path.
    """
    src = os.path.abspath(src)
    dest = os.path.abspath(dest)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    src_drive = os.path.splitdrive(src)[0].casefold()
    dest_drive = os.path.splitdrive(dest)[0].casefold()
    partial = dest + ".partial.webp"
    try:
        if src_drive and dest_drive and src_drive != dest_drive:
            raise OSError(errno.EXDEV, "cross-device output move")
        os.replace(src, dest)
        return
    except OSError as exc:
        # ERROR_NOT_SAME_DEVICE is WinError 17; Linux uses errno.EXDEV.
        if exc.errno != errno.EXDEV and getattr(exc, "winerror", None) != 17:
            raise
    try:
        shutil.copyfile(src, partial)
        os.replace(partial, dest)
        os.unlink(src)
    except Exception:
        try:
            os.unlink(partial)
        except OSError:
            pass
        raise


class UpscaleError(RuntimeError):
    def __init__(self, message, **info):
        super().__init__(message)
        self.info = info


class Log:
    """Append-only log next to the script. INFO/WARN/FAIL also go to the terminal."""

    def __init__(self, path):
        self.path = path
        self._fp = open(path, "a", encoding="utf-8", errors="backslashreplace")

    def close(self):
        self._fp.close()

    def _line(self, level, text, console):
        stamp = time.strftime("%Y-%m-%d %H:%M:%S")
        line = f"{stamp}  {level:<4}  {text}"
        self._fp.write(line + "\n")
        self._fp.flush()
        if console:
            print(line, flush=True)

    def info(self, text):
        self._line("INFO", text, True)

    def warn(self, text):
        self._line("WARN", text, True)

    def fail(self, text):
        self._line("FAIL", text, True)

    def debug(self, text):
        self._line("DBG", text, False)

    def block(self, title, text):
        body = text or ""
        if isinstance(body, bytes):
            body = body.decode("utf-8", "backslashreplace")
        self._fp.write(f"----- {title} -----\n")
        if body:
            self._fp.write(body)
            if not body.endswith("\n"):
                self._fp.write("\n")
        self._fp.write("----- end -----\n")
        self._fp.flush()


def output_state(src, dest, scale):
    """'ok', 'missing', or a short reason the existing file will be redone.

    Dimensions alone are not a sufficient cache key: replacing a source with
    another image of the same size used to leave the old WebP in place.  Use
    nanosecond mtimes as a cheap invalidation signal before doing the header
    checks.  A copied tree with equal timestamps keeps the old fast resume
    behavior.
    """
    if not os.path.isfile(dest):
        return "missing"
    try:
        if os.stat(src).st_mtime_ns > os.stat(dest).st_mtime_ns:
            return "source newer"
        sw, sh = image_size(src)
        got = webp_size(dest)
    except (OSError, ValueError) as exc:
        return f"unreadable ({exc})"
    expect = (sw * scale, sh * scale)
    if got == expect:
        return "ok"
    return f"size {got}, expected {expect[0]}x{expect[1]}"


def describe_source(path):
    size = os.path.getsize(path)
    ext = os.path.splitext(path)[1].lower()
    with open(path, "rb") as f:
        hdr = f.read(54)
    if ext == ".tga":
        img_type = hdr[2]
        w, h = struct.unpack_from("<HH", hdr, 12)
        depth = hdr[16]
        desc = hdr[17]
        return f"tga type={img_type} depth={depth} desc=0x{desc:02x} {w}x{h} {size}B"
    w, h = struct.unpack_from("<ii", hdr, 18)
    bpp = struct.unpack_from("<H", hdr, 28)[0]
    comp = struct.unpack_from("<I", hdr, 30)[0]
    return f"bmp bpp={bpp} comp={comp} {w}x{abs(h)} {size}B"


def upscale_one(binary, src, dest, scale, gpu, noise, tile, tta, jobs=""):
    dest = os.path.abspath(dest)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    # The encoder rejects any output path whose extension is not jpg/png/webp.
    # Paths are absolute because waifu2x is started in its own directory.
    partial = dest + ".partial.webp"
    info = {}
    with tempfile.TemporaryDirectory(prefix="wa2-upscale-") as tmp:
        png = os.path.join(tmp, "in.png")
        t0 = time.monotonic()
        w, h, rgba = decode(src)
        decode_s = time.monotonic() - t0
        alphas = rgba[3::4]
        t1 = time.monotonic()
        write_png(png, rgba, w, h)
        png_s = time.monotonic() - t1
        cmd = [
            binary,
            "-g", str(gpu),
            "-n", str(noise),
            "-s", str(scale),
            "-t", str(tile),
            "-f", "webp",
            "-i", png,
            "-o", partial,
        ]
        if tta:
            cmd.append("-x")
        if jobs:
            cmd.extend(["-j", jobs])
        info.update({
            "w": w,
            "h": h,
            "batch_count": 1,
            "alpha0": alphas.count(0),
            "opaque": alphas.count(255),
            "png_bytes": os.path.getsize(png),
            "decode_s": decode_s,
            "png_s": png_s,
            "cmd": cmd,
        })
        t2 = time.monotonic()
        proc = subprocess.run(cmd, cwd=os.path.dirname(binary), capture_output=True)
        info["waifu_s"] = time.monotonic() - t2
        info["returncode"] = proc.returncode
        info["stdout"] = proc.stdout
        info["stderr"] = proc.stderr
        if proc.returncode != 0:
            tail = (proc.stderr or proc.stdout or b"").decode("utf-8", "backslashreplace").strip()
            message = tail.splitlines()[-1] if tail else f"exit {proc.returncode}"
            raise UpscaleError(message, **info)
        if not os.path.isfile(partial):
            raise UpscaleError(f"waifu2x exited 0 but did not write {partial}", **info)
        got = webp_size(partial)
        info["got"] = got
        if got != (w * scale, h * scale):
            raise UpscaleError(
                f"output size {got}, expected {w * scale}x{h * scale}", **info
            )
        os.replace(partial, dest)
        info["bytes"] = os.path.getsize(dest)
        return info


def upscale_group(items, binary, scale, gpu, noise, tile, tta, jobs=""):
    """Run one waifu2x process on several images so encode and the next pass overlap.

    ``items`` is a list of ``(rel, src, dest)``. Returns one result dict per item.
    """
    results = []
    prepared = []
    with tempfile.TemporaryDirectory(prefix="wa2-upscale-") as tmp:
        in_dir = os.path.join(tmp, "in")
        out_dir = os.path.join(tmp, "out")
        os.makedirs(in_dir)
        os.makedirs(out_dir)
        decode_s = 0.0
        png_s = 0.0
        for i, (rel, src, dest) in enumerate(items):
            name = f"{i:04d}.png"
            png = os.path.join(in_dir, name)
            try:
                t0 = time.monotonic()
                w, h, rgba = decode(src)
                decode_s += time.monotonic() - t0
                alphas = rgba[3::4]
                t1 = time.monotonic()
                write_png(png, rgba, w, h)
                png_s += time.monotonic() - t1
            except (OSError, ValueError) as exc:
                results.append({
                    "status": "fail",
                    "rel": rel,
                    "src": src,
                    "dest": dest,
                    "exc": exc,
                    "elapsed": 0.0,
                })
                continue
            prepared.append({
                "rel": rel,
                "src": src,
                "dest": os.path.abspath(dest),
                "name": name,
                "w": w,
                "h": h,
                "alpha0": alphas.count(0),
                "opaque": alphas.count(255),
                "png_bytes": os.path.getsize(png),
            })
        if not prepared:
            return results

        cmd = [
            binary,
            "-g", str(gpu),
            "-n", str(noise),
            "-s", str(scale),
            "-t", str(tile),
            "-f", "webp",
            "-i", in_dir,
            "-o", out_dir,
        ]
        if tta:
            cmd.append("-x")
        if jobs:
            cmd.extend(["-j", jobs])
        t2 = time.monotonic()
        proc = subprocess.run(cmd, cwd=os.path.dirname(binary), capture_output=True)
        waifu_s = time.monotonic() - t2
        shared = {
            "decode_s": decode_s,
            "png_s": png_s,
            "waifu_s": waifu_s,
            "batch_count": len(prepared),
            "returncode": proc.returncode,
            "stdout": proc.stdout,
            "stderr": proc.stderr,
            "cmd": cmd,
        }
        if proc.returncode != 0:
            tail = (proc.stderr or proc.stdout or b"").decode("utf-8", "backslashreplace").strip()
            message = tail.splitlines()[-1] if tail else f"exit {proc.returncode}"
            exc = UpscaleError(message, **shared)
            for item in prepared:
                results.append({
                    "status": "fail",
                    "rel": item["rel"],
                    "src": item["src"],
                    "dest": item["dest"],
                    "exc": exc,
                    "elapsed": waifu_s / len(prepared),
                })
            return results

        each = waifu_s / len(prepared)
        for item in prepared:
            stem = os.path.splitext(item["name"])[0]
            out = os.path.join(out_dir, stem + ".webp")
            dest = item["dest"]
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            if not os.path.isfile(out):
                results.append({
                    "status": "fail",
                    "rel": item["rel"],
                    "src": item["src"],
                    "dest": dest,
                    "exc": UpscaleError(f"waifu2x wrote no {stem}.webp", **shared),
                    "elapsed": each,
                })
                continue
            got = webp_size(out)
            expect = (item["w"] * scale, item["h"] * scale)
            if got != expect:
                results.append({
                    "status": "fail",
                    "rel": item["rel"],
                    "src": item["src"],
                    "dest": dest,
                    "exc": UpscaleError(
                        f"output size {got}, expected {expect[0]}x{expect[1]}", **shared
                    ),
                    "elapsed": each,
                })
                continue
            install_output(out, dest)
            info = {
                "w": item["w"],
                "h": item["h"],
                "alpha0": item["alpha0"],
                "opaque": item["opaque"],
                "png_bytes": item["png_bytes"],
                "bytes": os.path.getsize(dest),
                "got": got,
                **shared,
            }
            results.append({
                "status": "ok",
                "rel": item["rel"],
                "dest": dest,
                "info": info,
                "elapsed": each,
            })
    return results


def _worker_group(payload):
    """Top-level entry for ProcessPoolExecutor (pickle-safe)."""
    items, kw = payload
    try:
        return upscale_group(items, **kw)
    except (OSError, ValueError, UpscaleError) as exc:
        return [{
            "status": "fail",
            "rel": rel,
            "src": src,
            "dest": dest,
            "exc": exc,
            "elapsed": 0.0,
        } for rel, src, dest in items]


def _worker_payload(payload):
    """Top-level entry for ProcessPoolExecutor (pickle-safe)."""
    rel, src, dest, kw = payload
    t0 = time.monotonic()
    try:
        info = upscale_one(src=src, dest=dest, **kw)
        return {
            "status": "ok",
            "rel": rel,
            "dest": dest,
            "info": info,
            "elapsed": time.monotonic() - t0,
        }
    except (OSError, ValueError, UpscaleError) as exc:
        return {
            "status": "fail",
            "rel": rel,
            "src": src,
            "dest": dest,
            "exc": exc,
            "elapsed": time.monotonic() - t0,
        }


def fmt_size(n):
    """Format output sizes without rounding small valid WebPs to 0.0MB."""
    if n < 1024:
        return f"{n}B"
    if n < 1024 * 1024:
        return f"{n / 1024:.1f}KB"
    return f"{n / (1024 * 1024):.2f}MB"


def fmt_eta(seconds):
    seconds = int(seconds)
    h, rem = divmod(seconds, 3600)
    m, s = divmod(rem, 60)
    if h:
        return f"{h}h{m:02d}m"
    return f"{m}m{s:02d}s"


def run_check(data):
    """Decode a few known files and confirm the header reader agrees."""
    expect = [
        ("sprite/kaz001101.tga", 512, 720, "sprite"),
        ("sprite/koh001001.tga", 488, 720, "sprite"),
        ("bg/b100101.tga", 1280, 720, "opaque"),
        ("bg/sys_1000.tga", 1280, 460, "keyed"),
    ]
    failed = 0
    for rel, w, h, kind in expect:
        path = os.path.join(data, rel)
        if not os.path.isfile(path):
            print(f"missing {rel}")
            failed += 1
            continue
        sw, sh = image_size(path)
        dw, dh, rgba = decode(path)
        alpha0 = rgba[3::4].count(0)
        opaque = rgba[3::4].count(255)
        ok = (sw, sh) == (w, h) == (dw, dh)
        if kind == "opaque":
            ok = ok and alpha0 == 0 and opaque == w * h
        elif kind == "sprite":
            frac = alpha0 / (w * h)
            ok = ok and 0.30 < frac < 0.70
        elif kind == "keyed":
            ok = ok and alpha0 > 0
        print(f"{'ok' if ok else 'FAIL'}  {rel}  {dw}x{dh}  alpha0={alpha0} opaque={opaque}")
        if not ok:
            failed += 1

    bmp8 = bmp32 = None
    for dirpath, _dirs, names in os.walk(os.path.join(data, "bg")):
        for name in sorted(names):
            if not name.lower().endswith(".bmp"):
                continue
            path = os.path.join(dirpath, name)
            with open(path, "rb") as f:
                hdr = f.read(30)
            bpp = struct.unpack_from("<H", hdr, 28)[0]
            if bpp == 8 and bmp8 is None:
                bmp8 = path
            elif bpp == 32 and bmp32 is None:
                bmp32 = path
        if bmp8 and bmp32:
            break
    for path, kind in ((bmp8, "bmp8"), (bmp32, "bmp32")):
        if path is None:
            print(f"missing {kind}")
            failed += 1
            continue
        sw, sh = image_size(path)
        dw, dh, rgba = decode(path)
        ok = (sw, sh) == (dw, dh) and dw > 0 and dh > 0
        if kind == "bmp32":
            alphas = set(rgba[3::4])
            ok = ok and len(alphas) > 1
        else:
            ok = ok and rgba[3::4] == b"\xff" * (dw * dh)
        rel = os.path.relpath(path, data)
        print(f"{'ok' if ok else 'FAIL'}  {rel}  {dw}x{dh}  {kind}")
        if not ok:
            failed += 1

    sample = os.path.expanduser("~/文档/wa2-立绘/01-冬马和纱-8x-tta.webp")
    if os.path.isfile(sample):
        got = webp_size(sample)
        ok = got == (4096, 5760)
        print(f"{'ok' if ok else 'FAIL'}  webp header {got}")
        if not ok:
            failed += 1
    if failed:
        raise SystemExit(f"{failed} check(s) failed")
    print("check passed")


def main():
    ap = argparse.ArgumentParser(description="8x TTA upscale of WA2 art to lossless WebP")
    ap.add_argument("--data", default=default_data(), help="data tree (default: $WA2_DATA or ./data)")
    ap.add_argument(
        "--bin",
        default=os.environ.get("WAIFU2X", default_binary()),
        help="waifu2x-ncnn-vulkan binary",
    )
    ap.add_argument("--gpu", type=int, default=1, help="Vulkan GPU id (1 is the NVIDIA card on this machine)")
    ap.add_argument("--scale", type=int, default=8, choices=(1, 2, 4, 8, 16, 32))
    ap.add_argument("--noise", type=int, default=-1, choices=(-1, 0, 1, 2, 3))
    ap.add_argument("--tile", type=int, default=0, help="tile size, 0 = automatic")
    ap.add_argument(
        "--jobs",
        default=os.environ.get("WA2_JOBS", "4:8:4"),
        help="waifu2x -j load:proc:save (default 4:8:4; use 2:4:4 with --workers 3+)",
    )
    ap.add_argument(
        "--workers",
        type=int,
        default=int(os.environ.get("WA2_WORKERS", "3")),
        help="parallel waifu2x processes (default 3 for desktop NVIDIA; use 1 on 960M)",
    )
    ap.add_argument(
        "--batch",
        type=int,
        default=int(os.environ.get("WA2_BATCH", "8")),
        help="images per waifu2x process (default 8; lower for limited VRAM/disk)",
    )
    ap.add_argument("--no-tta", action="store_true")
    ap.add_argument("--only", default="", help="comma-separated folders: sprite,bg,bg_cn")
    ap.add_argument("--match", default="", help="only relative paths containing this text")
    ap.add_argument("--limit", type=int, default=0, help="stop after this many new images")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--check", action="store_true", help="decode a few known files and exit")
    args = ap.parse_args()

    data = args.data
    if not os.path.isdir(data):
        print("data directory not found:", data, file=sys.stderr)
        return 1
    if args.check:
        run_check(data)
        return 0

    only = tuple(p.strip() for p in args.only.split(",") if p.strip())
    unknown = [p for p in only if p not in FOLDERS]
    if unknown:
        print("unknown folder:", ", ".join(unknown), file=sys.stderr)
        return 1
    rels = iter_sources(data, only, args.match)
    pending = []
    redo = []
    done = 0
    for rel in rels:
        src = os.path.join(data, rel)
        dest = dest_for(data, rel, args.scale)
        state = output_state(src, dest, args.scale)
        if state == "ok":
            done += 1
        else:
            pending.append(rel)
            if state != "missing":
                redo.append((rel, state))

    by_folder = {}
    for rel in rels:
        folder = rel.split(os.sep, 1)[0]
        ext = os.path.splitext(rel)[1].lower()
        by_folder[(folder, ext)] = by_folder.get((folder, ext), 0) + 1
    outs = ", ".join(
        f"{folder}_{args.scale}x"
        for folder in sorted({r.split(os.sep, 1)[0] for r in rels})
    )
    print(f"sources {len(rels)}   done {done}   pending {len(pending)}   -> {outs}")
    for key in sorted(by_folder):
        print(f"  {by_folder[key]:5}  {key[0]}/*{key[1]}  ->  {key[0]}_{args.scale}x/")
    if args.dry_run:
        for rel, state in redo:
            print(f"  redo  {rel}  {state}")
        return 0
    if not pending:
        return 0
    if not os.path.isfile(args.bin):
        print("waifu2x binary not found:", args.bin, file=sys.stderr)
        return 1

    log_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "upscale.log")
    log = Log(log_path)
    fail_path = os.path.join(os.path.dirname(log_path), "upscale-failed.txt")
    tta = not args.no_tta
    try:
        return run_queue(args, data, pending, redo, done, log, fail_path, tta)
    except KeyboardInterrupt:
        log.warn("stopped; rerun the same command to continue")
        print("stopped; rerun the same command to continue", file=sys.stderr)
        return 130
    finally:
        log.close()


def effective_jobs(jobs, workers):
    """Several GPU clients share one card; keep proc threads modest."""
    if workers <= 1:
        return jobs
    if jobs == "4:8:4":
        return "2:4:4"
    return jobs


def run_queue_serial(log, failed, groups, data, scale, kw, started, limit):
    made = 0
    total = sum(len(g) for g in groups)
    seen = 0
    for group in groups:
        for rel, _src, _dest in group:
            seen += 1
            log.info(f"[{seen}/{total}] {rel}")
        try:
            results = upscale_group(group, **kw)
        except (OSError, ValueError, UpscaleError) as exc:
            for rel, src, dest in group:
                record_failure(log, failed, rel, src, dest, exc)
            continue
        made += _record_group(log, failed, results, scale, started, limit)
    return made


def run_queue_parallel(log, failed, groups, scale, kw, workers, started, limit):
    made = 0
    total = sum(len(g) for g in groups)
    log.info(f"parallel pool workers={workers} images={total} groups={len(groups)}")
    done_count = 0
    with ProcessPoolExecutor(max_workers=workers) as pool:
        futures = [pool.submit(_worker_group, (group, kw)) for group in groups]
        for fut in as_completed(futures):
            for result in fut.result():
                done_count += 1
                rel = result["rel"]
                log.info(f"[{done_count}/{total}] {rel}")
                if result["status"] == "ok":
                    made += 1
                    avg = (time.monotonic() - started) / done_count
                    left = avg * (total - done_count)
                    record_success(
                        log, rel, result["dest"], result["info"], scale, result["elapsed"], left
                    )
                else:
                    record_failure(
                        log, failed, rel, result["src"], result["dest"], result["exc"]
                    )
    return made


def _record_group(log, failed, results, scale, started, limit):
    made = 0
    for result in results:
        rel = result["rel"]
        if result["status"] == "ok":
            made += 1
            avg = (time.monotonic() - started) / max(made, 1)
            left = avg * max(limit - made, 0)
            record_success(
                log, rel, result["dest"], result["info"], scale, result["elapsed"], left
            )
        else:
            record_failure(log, failed, rel, result["src"], result["dest"], result["exc"])
    return made


def run_queue(args, data, pending, redo, done, log, fail_path, tta):
    workers = max(1, args.workers)
    jobs = effective_jobs(args.jobs, workers)
    binary_stat = os.stat(args.bin)
    log.info(
        f"session pid={os.getpid()} scale={args.scale} noise={args.noise} "
        f"tile={args.tile} tta={int(tta)} gpu={args.gpu} "
        f"workers={workers} jobs={jobs} batch={max(1, args.batch)}"
    )
    log.info(f"argv {' '.join(sys.argv)}")
    log.info(
        f"binary {args.bin}  {binary_stat.st_size}B  "
        f"mtime {time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(binary_stat.st_mtime))}"
    )
    log.info(f"data {os.path.abspath(data)}")
    log.info(f"queue sources_done={done} pending={len(pending)} limit={args.limit or len(pending)}")
    for rel, state in redo:
        log.warn(f"redo {rel}  {state}")

    kw = {
        "binary": args.bin,
        "scale": args.scale,
        "gpu": args.gpu,
        "noise": args.noise,
        "tile": args.tile,
        "tta": tta,
        "jobs": jobs,
    }
    made = 0
    failed = []
    started = time.monotonic()
    limit = args.limit or len(pending)
    batch = pending[:limit]

    groups = []
    cur = []
    size = max(1, args.batch)
    for rel in batch:
        cur.append((rel, os.path.join(data, rel), dest_for(data, rel, args.scale)))
        if len(cur) >= size:
            groups.append(cur)
            cur = []
    if cur:
        groups.append(cur)

    if workers == 1:
        made = run_queue_serial(log, failed, groups, data, args.scale, kw, started, limit)
    else:
        made = run_queue_parallel(log, failed, groups, args.scale, kw, workers, started, limit)

    log.info(f"finished wrote={made} failed={len(failed)} skipped={done}")
    if failed:
        with open(fail_path, "w", encoding="utf-8", errors="backslashreplace") as f:
            for rel, message in failed:
                f.write(f"{rel}\t{message}\n")
        log.fail(f"failure list {fail_path}")
        return 1
    if os.path.exists(fail_path):
        os.remove(fail_path)
    return 0


def record_success(log, rel, dest, info, scale, elapsed, left):
    w, h = info["w"], info["h"]
    log.debug(
        f"decoded {w}x{h} alpha0={info['alpha0']} opaque={info['opaque']} "
        f"png={info['png_bytes']}B decode={info['decode_s']:.2f}s png={info['png_s']:.2f}s"
    )
    log.debug("cmd " + shlex.join(info["cmd"]))
    log.debug(f"waifu exit={info['returncode']} {info['waifu_s']:.2f}s")
    log.block(f"stdout {rel}", info["stdout"])
    log.block(f"stderr {rel}", info["stderr"])
    batch_count = info.get("batch_count", 1)
    if batch_count > 1:
        timing = f"waifu_batch={info['waifu_s']:.0f}s avg={elapsed:.0f}s"
    else:
        timing = f"waifu={info['waifu_s']:.0f}s total={elapsed:.0f}s"
    log.info(
        f"ok {rel}  {w}x{h} -> {w * scale}x{h * scale}  {fmt_size(info['bytes'])}  "
        f"{timing} eta {fmt_eta(left)}  -> {dest}"
    )


def record_failure(log, failed, rel, src, dest, exc):
    partial = dest + ".partial.webp"
    partial_note = ""
    if os.path.exists(partial):
        partial_note = f" partial={os.path.getsize(partial)}B"
        os.remove(partial)
    message = str(exc)
    failed.append((rel, message))
    log.fail(f"{rel}  {message}{partial_note}")
    log.debug(f"source path {src}")
    log.debug(f"dest path {dest}")
    info = getattr(exc, "info", None) or {}
    if info:
        log.debug(
            f"decoded {info.get('w')}x{info.get('h')} alpha0={info.get('alpha0')} "
            f"opaque={info.get('opaque')} png={info.get('png_bytes')}B "
            f"decode={info.get('decode_s', 0):.2f}s png={info.get('png_s', 0):.2f}s "
            f"waifu={info.get('waifu_s', 0):.2f}s exit={info.get('returncode')} "
            f"got={info.get('got')}"
        )
        if info.get("cmd"):
            log.debug("cmd " + shlex.join(info["cmd"]))
        log.block(f"stdout {rel}", info.get("stdout") or "")
        log.block(f"stderr {rel}", info.get("stderr") or "")
    if not isinstance(exc, UpscaleError):
        log.block(f"traceback {rel}", traceback.format_exc())


if __name__ == "__main__":
    # Windows ProcessPoolExecutor needs spawn; guard keeps imports clean in workers.
    sys.exit(main())
