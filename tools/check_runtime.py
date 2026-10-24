#!/usr/bin/env python3
"""Check Linux runtime/build prerequisites before downloading game assets."""

import argparse
import ctypes
from pathlib import Path
import re
import shutil
import subprocess
import sys

LIBRARIES = {
    "SDL2": ("libSDL2-2.0.so.0", "libSDL2.so.0", "libSDL2.so", "libSDL2-2.0.so"),
    "FreeType": ("libfreetype.so.6", "libfreetype.so"),
}


def check_runtime(need_build=False, verbose=False):
    missing = []
    for label, candidates in LIBRARIES.items():
        for name in candidates:
            try:
                ctypes.CDLL(name)
                if verbose:
                    print(f"  {label}: OK ({name})")
                break
            except OSError:
                pass
        else:
            missing.append(f"{label} 运行库")
    for name in ("ffmpeg", "ffprobe"):
        path = shutil.which(name)
        if path is None:
            missing.append(name)
        elif verbose:
            print(f"  {name}: OK ({path})")
    if need_build:
        if not shutil.which("cc"):
            missing.append("cc（C 编译器/链接器，首次构建需要）")
        for name in ("cargo", "rustc"):
            if not shutil.which(name):
                missing.append(f"{name}（首次构建需要 Rust 工具链）")
                continue
            try:
                result = subprocess.run([name, "--version"], capture_output=True, text=True, timeout=10)
                if result.returncode:
                    missing.append(f"{name}（Rust 工具链尚未安装完成或不可用）")
                elif name == "rustc" and (match := re.search(r"rustc (\d+)\.(\d+)", result.stdout)) and tuple(map(int, match.groups())) < (1, 90):
                    missing.append("rustc（需要 Rust 1.90 或更新版本）")
                elif verbose:
                    print(f"  {result.stdout.strip()}")
            except (OSError, subprocess.TimeoutExpired):
                missing.append(f"{name}（无法运行）")
    if verbose:
        decoder = shutil.which("dwebp")
        print(f"  dwebp: {decoder or '未安装，使用 ffmpeg 解码'}")
        folder = Path(__file__).resolve().parent
        has_fonts = any((base / "fonts/manifest.tsv").is_file() for base in (folder, folder.parent))
        print(f"  内置字体包: {'已找到' if has_fonts else '未找到，将由引擎检查系统字体'}")
    if missing:
        print("缺少或无法使用以下依赖：\n  " + "\n  ".join(missing), file=sys.stderr)
        print("请先安装 SDL2、FreeType、ffmpeg（包含 ffprobe）；从源码首次启动还需要 Rust/Cargo。"
              "安装完成后重新运行 ./run.sh。", file=sys.stderr)
        return False
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--need-build", action="store_true")
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()
    return 0 if check_runtime(args.need_build, args.verbose) else 1


if __name__ == "__main__":
    raise SystemExit(main())
