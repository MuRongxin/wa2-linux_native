#!/usr/bin/env python3
"""Build the x86_64 player AppImage without changing host packages.

Requires Linux user namespaces, bwrap, mksquashfs, patchelf and Rust. The build
uses a private Ubuntu 20.04 filesystem under target/appimage-20.04. Inputs are pinned
in tools/appimage/sources.lock.json; no game data is added to the image.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "tools/appimage"
PACKAGES = """build-essential pkg-config nasm ca-certificates libfreetype6-dev
libwebp-dev webp libx11-dev libxext-dev libxcursor-dev libxrandr-dev libxinerama-dev
libxfixes-dev libxss-dev libxrender-dev libxi-dev libwayland-dev wayland-protocols
libxkbcommon-dev libegl1-mesa-dev libgl1-mesa-dev libasound2-dev libpulse-dev
libudev-dev libdbus-1-dev""".split()


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(command, **kwargs):
    subprocess.run([str(a) for a in command], check=True, **kwargs)


def download(source, cache):
    path = cache / source["name"]
    if path.is_file() and sha256(path) == source["sha256"]:
        return path
    print(f"Download {source['name']}", flush=True)
    temporary = path.with_suffix(path.suffix + ".part")
    last_error = None
    for url in source.get("mirrors", []) + [source["url"]]:
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "wa2-appimage-build"})
            with urllib.request.urlopen(request, timeout=30) as response, temporary.open("wb") as output:
                last = time.monotonic()
                done = 0
                while chunk := response.read(1024 * 1024):
                    output.write(chunk)
                    done += len(chunk)
                    if time.monotonic() - last > 5:
                        print(f"  {done / 1e6:.1f} MB", flush=True)
                        last = time.monotonic()
            if sha256(temporary) != source["sha256"]:
                raise RuntimeError(f"SHA-256 mismatch: {source['name']}")
            temporary.replace(path)
            return path
        except (OSError, RuntimeError) as exc:
            last_error = exc
            print(f"  Source unavailable: {exc}; trying next pinned mirror", flush=True)
    raise RuntimeError(f"Cannot obtain verified build input {source['name']}: {last_error}")


def extract(path, destination):
    # These are publisher archives verified against the source lock, including
    # the absolute symlinks used by an Ubuntu base filesystem.
    with tarfile.open(path) as archive:
        for item in archive:
            if item.name.startswith("/") or ".." in Path(item.name).parts:
                raise RuntimeError(f"Unsafe archive path: {item.name}")
        archive.extractall(destination, filter="fully_trusted")


def sandbox(work, command):
    rust = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    return ["bwrap", "--unshare-all", "--share-net", "--uid", "0", "--gid", "0",
            "--bind", work / "ubuntu-20.04", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
            "--ro-bind", ROOT, "/project", "--bind", work, "/build", "--ro-bind", rust, "/opt/rust",
            "--setenv", "PATH", "/opt/rust/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            "--setenv", "DEBIAN_FRONTEND", "noninteractive", "--setenv", "LC_ALL", "C.UTF-8",
            "--chdir", "/project", *command]


def prepare_baseline(work, ubuntu):
    root = work / "ubuntu-20.04"
    root.mkdir(exist_ok=True)
    if not (root / "usr/bin/apt-get").exists():
        extract(ubuntu, root)
    resolv = root / "etc/resolv.conf"
    resolv.unlink(missing_ok=True)
    shutil.copyfile("/etc/resolv.conf", resolv)
    certs = root / "etc/ssl/certs"
    certs.mkdir(parents=True, exist_ok=True)
    if not (certs / "ca-certificates.crt").exists():
        shutil.copyfile("/etc/ssl/certs/ca-certificates.crt", certs / "ca-certificates.crt")
    (root / "etc/apt/apt.conf.d/99wa2-build").write_text('APT::Sandbox::User "root";\n')
    policy = root / "usr/sbin/policy-rc.d"
    policy.write_text("#!/bin/sh\nexit 101\n")
    policy.chmod(0o755)
    sources = root / "etc/apt/sources.list"
    apt_sources = sources.read_text().replace("http://", "https://")
    mirror = os.environ.get("WA2_BUILD_APT_MIRROR", "").rstrip("/")
    if mirror:
        if not re.fullmatch(r"https://[A-Za-z0-9./_-]+", mirror):
            raise RuntimeError("WA2_BUILD_APT_MIRROR must be an HTTPS mirror URL")
        apt_sources = re.sub(r"https://(?:archive|security)\.ubuntu\.com/ubuntu/?", mirror + "/", apt_sources)
    sources.write_text(apt_sources)
    marker = work / "baseline-ready"
    requirements = ["usr/bin/gcc", "usr/bin/nasm", "usr/bin/dwebp", "usr/include/wayland-client.h",
                    "usr/include/pulse/pulseaudio.h", "usr/include/freetype2/ft2build.h"]
    if not all((root / p).exists() for p in requirements):
        print("Install build dependencies in the isolated baseline", flush=True)
        run(sandbox(work, ["/bin/bash", "-ec", "apt-get update && apt-get install -y --no-install-recommends " + " ".join(PACKAGES)]))
    marker.write_text("Ubuntu 20.04 / glibc 2.31\n")


def copy_libraries(work, appdir):
    # SDL backends use dlopen for the host's graphics/audio drivers. Bundle the
    # engine libraries and the complete ELF closure of the two image decoders,
    # while leaving glibc and hardware drivers to the desktop OS.
    libraries = appdir / "usr/lib"
    libraries.mkdir(parents=True, exist_ok=True)
    root = work / "ubuntu-20.04"
    # dlopen dependencies are invisible to ldd on SDL itself. Ship their
    # closure, leaving glibc, GPU drivers and sound servers to the host.
    backend_names = (
        "libfreetype.so.6", "libX11.so.6", "libXext.so.6", "libXcursor.so.1",
        "libXi.so.6", "libXfixes.so.3", "libXrandr.so.2", "libXinerama.so.1",
        "libXss.so.1", "libXrender.so.1", "libwayland-client.so.0",
        "libwayland-cursor.so.0", "libwayland-egl.so.1", "libxkbcommon.so.0",
        "libasound.so.2", "libpulse.so.0", "libpulse-simple.so.0",
        "libdbus-1.so.3", "libudev.so.1",
    )
    initial = ["/usr/bin/dwebp"] + ["/usr/lib/x86_64-linux-gnu/" + name for name in backend_names]
    initial = [p if (root / p.lstrip("/")).exists() else p.replace("/usr/lib/", "/lib/", 1)
               for p in initial]
    absent = [p for p in initial if not (root / p.lstrip("/")).exists()]
    if absent:
        raise RuntimeError("Missing baseline runtime libraries: " + ", ".join(absent))
    output = subprocess.check_output(sandbox(work, ["/bin/bash", "-ec", "ldd " + " ".join(initial)]), text=True)
    paths = {Path(p) for p in initial[1:]}
    for match in re.finditer(r"=> (/\S+)", output):
        paths.add(Path(match[1]))
    system = re.compile(r"^(?:(?:libc|libm|libdl|libpthread|librt|libresolv|libutil|libgcc_s|libstdc\+\+)\.so(?:\.|$)|ld-linux|libnss_)")
    for path in sorted(paths):
        if system.match(path.name):
            continue
        # Resolve links in the baseline, never against the host's /lib tree.
        source = root / str(path).lstrip("/")
        while source.is_symlink():
            target = os.readlink(source)
            source = root / target.lstrip("/") if target.startswith("/") else source.parent / target
        shutil.copy2(source, libraries / path.name)
    shutil.copy2(root / "usr/bin/dwebp", appdir / "usr/bin/dwebp")
    shutil.copytree(root / "usr/share/alsa", appdir / "usr/share/alsa")
    for path in libraries.iterdir():
        if path.is_file() and not path.is_symlink():
            run(["patchelf", "--set-rpath", "$ORIGIN", path])
    return sorted(p.name for p in libraries.iterdir())


def prune_python(appdir):
    python = appdir / "python"
    for pattern in ("include", "lib/pkgconfig", "share/man", "lib/python*/site-packages",
                    "lib/python*/ensurepip", "lib/python*/idlelib", "lib/python*/tkinter",
                    "lib/python*/turtledemo", "lib/python*/test", "lib/libpython*.a",
                    "lib/libtcl*.so", "lib/tcl*", "lib/tk*"):
        for path in python.glob(pattern):
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    for path in python.rglob("__pycache__"):
        shutil.rmtree(path)
    # The downloader needs TLS, hashes, zlib and ctypes. Omit optional modules
    # for UI, databases and readline, including their unrelated native code.
    for pattern in ("readline*", "_tkinter*", "_sqlite3*", "_gdbm*", "_dbm*", "_curses*", "_bz2*", "_lzma*"):
        for path in (python / "lib").glob("python*/lib-dynload/" + pattern):
            path.unlink()


def write_notices(work, appdir, lock):
    share = appdir / "usr/share/wa2-native"
    licenses = share / "LICENSES"
    licenses.mkdir(parents=True)
    for notice in (CONFIG / "licenses").glob("*.txt"):
        shutil.copy2(notice, licenses / notice.name)
    root = work / "ubuntu-20.04"
    # Retain package notices for backend libraries and their dependencies.
    system_notices = licenses / "ubuntu"
    system_notices.mkdir()
    for notice in sorted((root / "usr/share/doc").glob("*/copyright")):
        if notice.is_file():
            shutil.copy2(notice, system_notices / (notice.parent.name + ".txt"))
    shutil.copy2(work / "sources/SDL2-2.32.10/LICENSE.txt", licenses / "SDL2.txt")
    shutil.copy2(work / "sources/ffmpeg-9.0.2/COPYING.LGPLv2.1", licenses / "FFmpeg-LGPLv2.1.txt")
    source_dir = share / "sources"
    source_dir.mkdir()
    # Static FFmpeg helpers: include their corresponding source and exact
    # build recipe alongside the LGPL notice, even for an offline recipient.
    shutil.copy2(work / "downloads" / lock["sources"]["ffmpeg"]["name"], source_dir)
    shutil.copy2(CONFIG / "build_payload.sh", source_dir)
    shutil.copy2(CONFIG / "sources.lock.json", source_dir)
    (share / "THIRD-PARTY.txt").write_text(
        "SDL2: zlib license; FreeType: FreeType License; libwebp: BSD license.\n"
        "FFmpeg helpers are LGPL-2.1-or-later builds without GPL/nonfree components.\n"
        "Their corresponding source archive and build recipe are in sources/.\n"
        "Portions of this software are copyright The FreeType Project (www.freetype.org).\n"
        "Python and its bundled libraries retain their notices under python/.\n"
        "AppImage runtime: https://github.com/AppImage/type2-runtime (MIT).\n"
        "Font license notices are included beside the unchanged font files.\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-dir", type=Path, default=ROOT / "target/appimage-20.04")
    parser.add_argument("--output", type=Path, default=ROOT / "bin/wa2-native-linux-x86_64.AppImage")
    parser.add_argument("--release-tag", default="app-v0.1.0", help="independent program Release tag")
    parser.add_argument("--repack-only", action="store_true", help="reuse the already-built native payload and only rebuild the image")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_-][A-Za-z0-9_.-]*", args.release_tag):
        raise RuntimeError("Invalid program Release tag")
    if sys.version_info < (3, 12):
        raise RuntimeError("The AppImage builder requires Python 3.12+ (the player runtime already bundles Python)")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise RuntimeError("This builder targets x86_64 Linux")
    for tool in ("bwrap", "mksquashfs", "patchelf", "rustc", "cargo"):
        if not shutil.which(tool):
            raise RuntimeError("Missing build tool: " + tool)
    work = args.work_dir.resolve()
    cache = work / "downloads"
    cache.mkdir(parents=True, exist_ok=True)
    lock = json.loads((CONFIG / "sources.lock.json").read_text())
    # Reuse verified source downloads from the previous build without copying
    # binaries compiled against its newer glibc baseline.
    previous = ROOT / "target/appimage/downloads"
    for source in lock["sources"].values():
        cached = previous / source["name"]
        dest = cache / source["name"]
        if not dest.exists() and cached.is_file() and sha256(cached) == source["sha256"]:
            shutil.copy2(cached, dest)
    inputs = {name: download(source, cache) for name, source in lock["sources"].items()}
    if not args.repack_only:
        prepare_baseline(work, inputs["ubuntu"])
        print("Compile engine, SDL2 and runtime media decoders in Ubuntu 20.04", flush=True)
        recipe = sha256(CONFIG / "build_payload.sh")
        stamp = work / "payload-recipe"
        if stamp.exists() and stamp.read_text().strip() != recipe:
            for name in ("payload", "sdl-build", "ffmpeg-build"):
                shutil.rmtree(work / name, ignore_errors=True)
        run(sandbox(work, ["/bin/bash", "/project/tools/appimage/build_payload.sh"]))
        stamp.write_text(recipe + "\n")
    else:
        print("Reuse native payload; repack updated launch scripts", flush=True)

    appdir = work / "WA2.AppDir"
    if appdir.exists():
        shutil.rmtree(appdir)
    (appdir / "usr/bin").mkdir(parents=True)
    (appdir / "usr/lib").mkdir()
    share = appdir / "usr/share/wa2-native"
    share.mkdir(parents=True)
    for name in ("wa2-native", "ffmpeg", "ffprobe"):
        shutil.copy2(work / "payload/usr/bin" / name, appdir / "usr/bin" / name)
    for source in (work / "payload/usr/lib").glob("libSDL2*.so*"):
        destination = appdir / "usr/lib" / source.name
        if source.is_symlink():
            destination.symlink_to(os.readlink(source))
        else:
            shutil.copy2(source, destination)
    bundled_libraries = copy_libraries(work, appdir)
    extract(inputs["python"], appdir)
    prune_python(appdir)
    shutil.copytree(ROOT / "fonts", share / "fonts")
    (appdir / "usr/bin/fonts").symlink_to("../share/wa2-native/fonts", target_is_directory=True)
    for name in ("fetch_data.py", "check_runtime.py"):
        shutil.copy2(ROOT / "tools" / name, share / name)
    shutil.copy2(CONFIG / "launcher.py", share / "launcher.py")
    shutil.copy2(CONFIG / "AppRun", appdir / "AppRun")
    (appdir / "AppRun").chmod(0o755)
    for name in ("wa2-native.desktop", "wa2-native.svg"):
        shutil.copy2(CONFIG / name, appdir / name)
    (appdir / ".DirIcon").symlink_to("wa2-native.svg")
    shutil.copy2(work / "ubuntu-20.04/etc/ssl/certs/ca-certificates.crt", share / "ca-certificates.crt")
    write_notices(work, appdir, lock)
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    source_hash = hashlib.sha256()
    for path in sorted((ROOT / "src").rglob("*.rs")):
        source_hash.update(str(path.relative_to(ROOT)).encode() + b"\0" + path.read_bytes())
    unpacked = sum(p.stat().st_size for p in appdir.rglob("*") if p.is_file() and not p.is_symlink())
    info = {"format": "wa2-appimage/1", "source_commit": commit, "engine_source_sha256": source_hash.hexdigest(),
            "release_tag": args.release_tag, "unpacked_bytes": unpacked,
            "baseline": lock["baseline"], "includes_game_data": False, "inputs": lock["sources"],
            "bundled_libraries": bundled_libraries, "backends": ["X11", "Wayland", "PulseAudio", "ALSA"]}
    (share / "build-info.json").write_text(json.dumps(info, indent=2) + "\n")
    squash = work / "wa2.squashfs"
    print("Compress AppImage", flush=True)
    run(["mksquashfs", appdir, squash, "-noappend", "-all-root", "-no-xattrs", "-comp", "zstd", "-Xcompression-level", "10",
         "-b", "1M", "-processors", str(min(os.cpu_count() or 2, 8)), "-no-progress"])
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = work / "wa2-native.AppImage.new"
    with temporary.open("wb") as target:
        for source in (inputs["runtime"], squash):
            with source.open("rb") as stream:
                shutil.copyfileobj(stream, target)
    size = temporary.stat().st_size
    temporary.chmod(0o755)
    temporary.replace(output)
    digest = sha256(output)
    output.with_suffix(output.suffix + ".sha256").write_text(f"{digest}  {output.name}\n")
    info.update({"size": size, "sha256": digest})
    output.with_suffix(".build.json").write_text(json.dumps(info, indent=2) + "\n")
    pin = ("# Trusted program pin: copy into tools/appimage/release.conf after packaging.\n"
           "APPIMAGE_RELEASE_REPO=MuRongxin/wa2-linux_native\n"
           f"APPIMAGE_RELEASE_TAG={args.release_tag}\nAPPIMAGE_RELEASE_SHA256={digest}\n"
           f"APPIMAGE_RELEASE_SIZE={size}\nAPPIMAGE_UNPACKED_SIZE={unpacked}\n")
    (output.parent / "release.conf").write_text(pin)
    print(f"Built {output}: {size / 1024**2:.2f} MiB, SHA-256 {digest}", flush=True)


if __name__ == "__main__":
    main()
