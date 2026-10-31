#!/usr/bin/env python3
"""Download, verify and restore independent ZIP parts from a GitHub Release.

    python3 launcher/fetch_data.py --tag data-v1.1.0 --dest .

run.sh uses --ensure to leave existing playable data alone. Downloads use
several proxies by default; --proxy '' disables them. GITHUB_TOKEN is supported for
private releases and is sent directly to GitHub, never to the proxy.
The release needs manifest.json and every ZIP listed in it. SHA256SUMS is useful
for manual installation; the manifest already contains the same hashes.
Only the Python standard library is required.
"""

from __future__ import annotations

import argparse
from concurrent.futures import CancelledError, FIRST_COMPLETED, ThreadPoolExecutor, wait
import fcntl
import hashlib
import http.client
import json
import os
import re
import shutil
import stat
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath

API = "https://api.github.com"
PARTS_DIR_NAME = ".wa2-release-parts"
INSTALLING = ".wa2-installing.json"
UNPACK_DIR = ".wa2-unpack"
CHUNK = 1024 * 1024
DEFAULT_PROXIES = ("https://gh-proxy.com/", "https://ghfast.top/", "https://ghproxy.net/")
PROXY_SPEED_WINDOW = 20.0
PROXY_MIN_SPEED = 128 * 1024  # bytes/s; only triggers a switch away from the proxy


class FetchError(Exception):
    pass


class TryNextSource(FetchError):
    pass


class SlowProxy(TryNextSource):
    pass


def say(message: str) -> None:
    print(message, flush=True)


def duration(seconds: float) -> str:
    seconds = max(0, int(seconds))
    return f"{seconds // 3600:02d}:{seconds // 60 % 60:02d}:{seconds % 60:02d}"


class Progress:
    """Refresh a terminal line once a second; keep redirected logs readable."""

    def __init__(self, label: str, total: int, offset: int = 0,
                 overall: tuple[int, int] | None = None):
        self.label, self.total, self.offset = label, total, offset
        self.overall = overall
        self.started = time.monotonic()
        self.last = 0.0
        self.tty = sys.stdout.isatty()

    def update(self, done: int, final: bool = False) -> None:
        now = time.monotonic()
        if not final and self.last and now - self.last < (1 if self.tty else 5):
            return
        self.last = now
        speed = (done - self.offset) / max(now - self.started, 0.001)
        rate = f"{speed / 1e6:.1f} MB/s" if speed >= 1e6 else f"{speed / 1024:.1f} KB/s"
        message = (f"{self.label} {done * 100 / max(self.total, 1):5.1f}% | "
                   f"{done / 1e9:.3f}/{self.total / 1e9:.3f} GB | {rate}")
        remaining = self.total - done
        if self.overall:
            completed, total = self.overall
            message += f" | 总进度 {(completed + done) * 100 / max(total, 1):.1f}%"
            remaining = total - completed - done
        message += f" | 剩余 {duration(remaining / speed) if speed > 0 else '--:--:--'}"
        print(("\r\033[K" if self.tty else "") + message,
              end="\n" if final or not self.tty else "", flush=True)


def network_error(exc: Exception) -> str:
    if isinstance(exc, urllib.error.HTTPError):
        return f"HTTP {exc.code}"
    return str(exc)


def api_get(url: str, token: str, accept: str = "application/vnd.github+json") -> bytes:
    headers = {"User-Agent": "wa2-fetch-data", "Accept": accept}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as resp:
        return resp.read()


def api_json(url: str, token: str):
    return json.loads(api_get(url, token))


def find_release(repo: str, tag: str, token: str) -> dict:
    try:
        release = api_json(f"{API}/repos/{repo}/releases/tags/{urllib.parse.quote(tag, safe='')}", token)
        # The embedded list can omit unfinished uploads. Fetch every asset page.
        if release.get("assets_url"):
            assets = []
            for page in range(1, 101):
                batch = api_json(f"{release['assets_url']}?per_page=100&page={page}", token)
                assets.extend(batch)
                if len(batch) < 100:
                    break
            release["assets"] = assets
        return release
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            raise FetchError(f"Release {repo} @ {tag} 尚未发布或无访问权限；资源可能还在准备。") from exc
        if exc.code in (401, 403):
            raise FetchError("GitHub 拒绝访问（认证失败或 API 额度已用完）。可设置 GITHUB_TOKEN 后重试。") from exc
        raise


def proxy_list(value: str | list[str]) -> list[str]:
    proxies = []
    for setting in ([value] if isinstance(value, str) else value):
        for prefix in re.split(r"[,;\s]+", setting.strip()):
            if not prefix:
                continue
            parsed = urllib.parse.urlsplit(prefix)
            if (parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password
                    or parsed.query or parsed.fragment):
                raise FetchError("代理地址必须使用 HTTPS；--proxy '' 可禁用代理。")
            normalized = prefix.rstrip("/") + "/"
            if normalized not in proxies:
                proxies.append(normalized)
    return proxies


def asset_sources(asset: dict, token: str, proxy: str | list[str]) -> list[tuple[str, dict]]:
    headers = {"User-Agent": "wa2-fetch-data", "Accept": "application/octet-stream",
               "Accept-Encoding": "identity"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
        return [(asset["url"], headers)]
    url = asset["browser_download_url"]
    return [(prefix + url, headers) for prefix in proxy_list(proxy)] + [(url, headers)]


def asset_content(asset: dict, token: str, proxy: str | list[str] = "") -> bytes:
    sources = asset_sources(asset, token, proxy)
    for index, (url, headers) in enumerate(sources):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as resp:
                content = resp.read(4 * CHUNK + 1)
            if len(content) > 4 * CHUNK or len(content) != int(asset["size"]):
                raise FetchError("清单大小不符，下载地址可能返回了错误页面。")
            json.loads(content)  # Detect HTML error pages returned with HTTP 200.
            return content
        except (OSError, ValueError, FetchError) as exc:
            if index + 1 == len(sources):
                raise FetchError(f"读取 manifest.json 失败：{network_error(exc)}") from exc
            say("当前通道未能读取资源清单，尝试下一个代理或 GitHub 直连……")
    raise FetchError("无法读取资源清单。")


def validate_manifest(manifest: dict) -> None:
    if not isinstance(manifest, dict):
        raise FetchError("资源清单必须是 JSON 对象。")
    if manifest.get("container") != "zip-parts":
        raise FetchError("仅支持独立 ZIP 分卷资源包。")
    trees = manifest.get("inventory", {}).get("trees", {})
    if not trees or any(not re.fullmatch(r"[A-Za-z0-9_-]+", t) or t in ("save", "cache") for t in trees):
        raise FetchError("资源清单中的数据目录无效。")
    names = set()
    for part in manifest.get("parts", []):
        name = part.get("name", "")
        if (not re.fullmatch(r"[A-Za-z0-9_.-]+\.part[0-9]+\.zip", name)
                or name in names or not re.fullmatch(r"[0-9a-f]{64}", part.get("sha256", ""))
                or not isinstance(part.get("size"), int) or part["size"] <= 0):
            raise FetchError("资源清单中的分卷文件名、大小或 SHA-256 无效。")
        names.add(name)
    if not names:
        raise FetchError("资源清单没有分卷。")
    for info in trees.values():
        if any(not isinstance(info.get(k), int) or info[k] < 0 for k in ("files", "bytes")):
            raise FetchError("资源清单的文件统计无效。")
    for key in ("files", "bytes"):
        if manifest["inventory"].get(key) != sum(info[key] for info in trees.values()):
            raise FetchError("资源清单的文件统计不一致。")


def check_assets(parts: list[dict], assets: dict) -> None:
    problems = []
    for part in parts:
        asset = assets.get(part["name"])
        if not asset or asset.get("state", "uploaded") != "uploaded":
            problems.append(part["name"] + "（缺失或尚未上传完成）")
        elif int(asset["size"]) != part["size"]:
            problems.append(part["name"] + "（大小与清单不符）")
        elif asset.get("digest") and asset["digest"] != "sha256:" + part["sha256"]:
            problems.append(part["name"] + "（SHA-256 与清单不符）")
    if problems:
        raise FetchError("Release 资源尚未准备齐全，未开始下载分卷：\n  " + "\n  ".join(problems))


def sha256_file(path: Path, progress: Progress | None = None) -> str:
    digest = hashlib.sha256()
    done = 0
    with path.open("rb") as stream:
        while chunk := stream.read(CHUNK):
            digest.update(chunk)
            done += len(chunk)
            if progress:
                progress.update(done)
    if progress:
        progress.update(done, final=True)
    return digest.hexdigest()


def download_asset(asset: dict, dest: Path, token: str, expected_sha: str,
                   proxy: str | list[str] = "", completed: int = 0, total: int = 0,
                   label: str = "下载", stop: threading.Event | None = None) -> None:
    def check_cancelled() -> None:
        if stop is not None and stop.is_set():
            raise CancelledError("download cancelled; partial file retained")

    check_cancelled()
    expected_size = int(asset["size"])
    partial = dest.with_suffix(dest.suffix + ".part")

    def verified(path: Path) -> bool:
        return (path.is_file() and path.stat().st_size == expected_size
                and sha256_file(path, Progress(f"校验 {dest.name}", expected_size)) == expected_sha)

    if verified(dest):
        say(f"复用已校验分卷：{dest.name}")
        return
    if dest.exists():
        say(f"缓存损坏，重新下载：{dest.name}")
        dest.unlink()
    if partial.exists() and partial.stat().st_size >= expected_size:
        if verified(partial):
            partial.replace(dest)
            return
        partial.unlink()

    last_error = None
    sources = asset_sources(asset, token, proxy)
    proxy_count = len(sources) - 1
    for index, (url, base_headers) in enumerate(sources):
        route = urllib.parse.urlsplit(url).hostname if index < proxy_count else "GitHub 直连"
        if index:
            say(f"切换下载通道：{route}，保留断点……")
        else:
            say(f"下载通道：{route}")
        for attempt in range(3):
            check_cancelled()
            offset = partial.stat().st_size if partial.exists() else 0
            if offset == expected_size:
                if verified(partial):
                    partial.replace(dest)
                    return
                partial.unlink()
                offset = 0
            headers = dict(base_headers)
            if offset:
                headers["Range"] = f"bytes={offset}-"
                say(f"续传 {dest.name}：已下载 {offset / 1e9:.3f} GB")
            try:
                with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as resp:
                    if resp.status == 206:
                        match = re.fullmatch(r"bytes (\d+)-(\d+)/(\d+)", resp.headers.get("Content-Range", ""))
                        if (not match or int(match[1]) != offset or int(match[3]) != expected_size
                                or not offset <= int(match[2]) < expected_size):
                            raise FetchError("服务器返回了无效的断点范围。")
                    elif resp.status == 200:
                        if offset:
                            if index < proxy_count:
                                raise TryNextSource("当前代理不支持续传，保留断点并尝试下一通道。")
                            say("服务器不支持续传，重新下载当前分卷。")
                        offset = 0  # Never append a full response to a partial file.
                    else:
                        raise FetchError(f"服务器返回 HTTP {resp.status}。")
                    done = offset
                    progress = Progress(label, expected_size, offset, (completed, total or expected_size))
                    progress.update(done)
                    window_started, window_done = time.monotonic(), done
                    read = getattr(resp, "read1", resp.read)
                    with partial.open("ab" if offset else "wb") as output:
                        while chunk := read(CHUNK):
                            check_cancelled()
                            if done + len(chunk) > expected_size:
                                raise FetchError("下载内容超过清单大小。")
                            output.write(chunk)
                            done += len(chunk)
                            progress.update(done)
                            now = time.monotonic()
                            elapsed = now - window_started
                            if index < proxy_count and done < expected_size and elapsed >= PROXY_SPEED_WINDOW:
                                if (done - window_done) / max(elapsed, 0.001) < PROXY_MIN_SPEED:
                                    raise SlowProxy("代理下载持续过慢，保留断点并尝试下一通道。")
                                window_started, window_done = now, done
                    progress.update(done, final=True)
                if done != expected_size:
                    raise FetchError(f"连接提前结束：{done}/{expected_size} 字节。")
                if not verified(partial):
                    partial.unlink(missing_ok=True)
                    raise FetchError("SHA-256 校验失败，重新下载当前分卷。")
                partial.replace(dest)
                return
            except (OSError, http.client.HTTPException, FetchError) as exc:
                last_error = exc
                if isinstance(exc, urllib.error.HTTPError) and exc.code == 416:
                    partial.unlink(missing_ok=True)
                say(f"\n{dest.name} 下载失败（{attempt + 1}/3）：{network_error(exc)}")
                if isinstance(exc, TryNextSource):
                    break
                if isinstance(exc, urllib.error.HTTPError) and exc.code in (401, 403, 404):
                    break
                if attempt < 2:
                    time.sleep(attempt + 1)
    raise FetchError(f"{dest.name} 下载失败：{network_error(last_error)}。断点已保留，重新运行 ./run.sh 可继续。")


def download_parts(parts: list[dict], assets: dict, parts_dir: Path, token: str,
                   proxies: list[str], jobs: int) -> None:
    """Download verified parts with bounded scheduling; never release live workers."""
    if not 1 <= jobs <= 4:
        raise ValueError("jobs must be between 1 and 4")
    stop = threading.Event()
    pool = ThreadPoolExecutor(max_workers=jobs)
    pending = set()
    remaining = iter(enumerate(parts, 1))

    def submit_next() -> bool:
        item = next(remaining, None)
        if item is None:
            return False
        index, part = item
        say(f"下载 [{index}/{len(parts)}] {part['name']}")
        # Parallel progress is per part, not an inaccurate serial total.
        pending.add(pool.submit(
            download_asset, assets[part["name"]], parts_dir / part["name"], token,
            part["sha256"], proxies, 0, part["size"],
            f"下载 [{index}/{len(parts)}] {part['name']}", stop=stop))
        return True

    try:
        for _ in range(jobs):
            if not submit_next():
                break
        while pending:
            done, pending = wait(pending, return_when=FIRST_COMPLETED)
            # Observe every completed result before scheduling more work.
            for future in done:
                future.result()
            for _ in done:
                submit_next()
    finally:
        # Also handles KeyboardInterrupt: running workers retain partial files,
        # and queued work is cancelled before the installer lock is released.
        stop.set()
        for future in pending:
            future.cancel()
        pool.shutdown(wait=True, cancel_futures=True)


def runtime_ready(data: Path, ignore_installing: bool = False) -> bool:
    if not ignore_installing and (data / INSTALLING).exists():
        return False
    if not all((data / "script" / name).is_file() for name in ("1001.bnr", "1001.txt")):
        return False
    if (data / "script_cn").is_dir() and not all(
            (data / "script_cn" / name).is_file() for name in ("1001.bnr", "1001.txt")):
        return False
    for tree in ("voice", "bgm", "se", "movie", "font"):
        root = data / tree
        if not root.is_dir() or not any(p.is_file() for p in root.iterdir()):
            return False
    for suffix in ("_4x", "_8x", "_2x", ""):
        trees = ["bg" + suffix, "sprite" + suffix]
        if (data / "script_cn").is_dir():
            trees.append("bg_cn" + suffix)
        if all((data / t).is_dir() and any(p.is_file() for p in (data / t).iterdir()) for t in trees):
            return True
    return False


def tree_totals(data: Path, tree: str, manifest: dict) -> tuple[int, int]:
    suffixes = set(manifest.get("tree_filters", {}).get(tree, {}).get("suffixes", []))
    sizes = [p.stat().st_size for p in (data / tree).rglob("*")
             if p.is_file() and (not suffixes or p.suffix.lower() in suffixes)]
    return len(sizes), sum(sizes)


def verify_manifest_files(dest: Path, manifest: dict, data_dir: Path | None = None) -> bool:
    data = data_dir if data_dir is not None else dest / "data"
    ok = True
    for tree, info in manifest["inventory"]["trees"].items():
        files, size = tree_totals(data, tree, manifest)
        good = files == info["files"] and size == info["bytes"]
        say(f"  {tree:12s} 文件 {files}/{info['files']}，字节 {size}/{info['bytes']} {'OK' if good else '不符'}")
        ok = ok and good
    return ok


def check_space(data: Path, parts_dir: Path, manifest: dict) -> None:
    needed_download = 0
    for part in manifest["parts"]:
        target = parts_dir / part["name"]
        partial = target.with_suffix(target.suffix + ".part")
        have = target.stat().st_size if target.is_file() else partial.stat().st_size if partial.is_file() else 0
        needed_download += part["size"] - min(have, part["size"])
    existing = sum(min(tree_totals(data, tree, manifest)[1], info["bytes"])
                   for tree, info in manifest["inventory"]["trees"].items())
    # Atomic replacement may temporarily keep both copies of the largest file.
    workspace = max([512 * CHUNK] + [p.get("bytes", 0) for p in manifest["parts"]])
    needed_extract = manifest["inventory"]["bytes"] - existing + workspace
    if data.stat().st_dev == parts_dir.stat().st_dev:
        checks = [(data, needed_download + needed_extract)]
    else:
        checks = [(parts_dir, needed_download), (data, needed_extract)]
    for path, needed in checks:
        free = shutil.disk_usage(path).free
        say(f"磁盘空间 {path}：可用 {free / 1e9:.2f} GB，预计还需 {needed / 1e9:.2f} GB")
        if free < needed:
            raise FetchError("磁盘空间不足（安装期间需要同时存放压缩包和解压后的资源）。")


def filesystem_case_sensitive(data: Path) -> bool:
    """Probe the destination itself; remove only our uniquely created empty file."""
    with tempfile.NamedTemporaryFile(dir=data, prefix=".wa2-case-probe-", suffix="-a") as probe:
        original = Path(probe.name)
        alternate = original.with_name(original.name.upper())
        try:
            return not original.samefile(alternate)
        except FileNotFoundError:
            return True


def check_archive_case_collisions(parts_dir: Path, data: Path, manifest: dict) -> None:
    """Reject case aliases across all parts before writing any archive member."""
    if filesystem_case_sensitive(data):
        return
    seen: dict[str, tuple[str, str]] = {}
    for part in manifest["parts"]:
        with zipfile.ZipFile(parts_dir / part["name"]) as archive:
            for info in archive.infolist():
                # Include directory components: Art/a and art/b also alias on
                # a case-insensitive volume. Retain the ZIP spelling in errors.
                components = info.filename.rstrip("/").split("/")
                for count in range(1, len(components) + 1):
                    name = "/".join(components[:count])
                    key = name.casefold()
                    previous = seen.get(key)
                    if previous is not None and previous[0] != name:
                        raise FetchError(
                            f"目标目录 {data} 所在文件系统不区分大小写，资源文件名冲突：\n"
                            f"  {previous[1]}\n  {info.filename}\n"
                            "尚未解压任何分卷；不能重命名或忽略这些资源。请在 macOS 创建并挂载"
                            "区分大小写的 APFS 卷或 APFSX 磁盘映像，再用 --data-dir 指向该卷上的"
                            "数据目录（运行游戏时设置 WA2_DATA 为同一路径）。"
                            "已下载并校验的分卷已保留，可用原 --parts-dir 和 --check-only 重试。")
                    seen.setdefault(key, (name, info.filename))


def extract_parts(parts_dir: Path, data: Path, manifest: dict) -> None:
    check_archive_case_collisions(parts_dir, data, manifest)
    # Keep interrupted writes outside the inventoried trees. A SIGKILL must not
    # leave random temporary files that make the next inventory check fail.
    staging = data / UNPACK_DIR
    if staging.is_symlink():
        raise FetchError("解压临时目录不能是符号链接。")
    staging.mkdir(exist_ok=True)
    for orphan in staging.glob("file-*"):
        orphan.unlink()
    progress = Progress("解压资源", manifest["inventory"]["bytes"])
    done = 0
    for index, part in enumerate(manifest["parts"], 1):
        say(f"解压 [{index}/{len(manifest['parts'])}] {part['name']}")
        with zipfile.ZipFile(parts_dir / part["name"]) as archive:
            # Validate the entire part before writing any of its members.
            entries = []
            for info in archive.infolist():
                path = PurePosixPath(info.filename)
                if (path.is_absolute() or ".." in path.parts or "\\" in info.filename
                        or len(path.parts) < 2 or path.parts[0] != "data"
                        or path.parts[1] not in manifest["inventory"]["trees"]
                        or stat.S_ISLNK(info.external_attr >> 16)):
                    raise FetchError(f"压缩包包含不安全的路径：{info.filename}")
                target = data.joinpath(*path.parts[1:])
                if data not in target.resolve().parents:
                    raise FetchError(f"压缩包路径超出数据目录：{info.filename}")
                cursor = target
                while cursor != data:
                    if cursor.is_symlink():
                        raise FetchError(f"解压路径包含符号链接：{info.filename}")
                    cursor = cursor.parent
                entries.append((info, target))
            for info, target in entries:
                if info.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                temporary = None
                try:
                    with archive.open(info) as source, tempfile.NamedTemporaryFile(
                            dir=staging, prefix="file-", delete=False) as output:
                        temporary = Path(output.name)
                        while chunk := source.read(CHUNK):
                            output.write(chunk)
                            done += len(chunk)
                            progress.update(done)
                    temporary.replace(target)
                finally:
                    if temporary is not None:
                        temporary.unlink(missing_ok=True)
    progress.update(done, final=True)
    staging.rmdir()


def cached_manifest(parts_dir: Path) -> dict | None:
    path = parts_dir / "manifest.json"
    if not path.is_file():
        return None
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
        validate_manifest(manifest)
        if all((parts_dir / p["name"]).is_file() and (parts_dir / p["name"]).stat().st_size == p["size"]
               for p in manifest["parts"]):
            return manifest
    except (OSError, ValueError, FetchError, TypeError, KeyError, AttributeError):
        pass
    return None


def write_json(path: Path, value: dict) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default=os.environ.get("WA2_DATA_REPO", "MuRongxin/wa2-linux_native"))
    parser.add_argument("--tag", default=os.environ.get("WA2_DATA_TAG", "data-v1.1.0"))
    parser.add_argument("--dest", default=".", help="project directory (default data root: <dest>/data)")
    parser.add_argument("--data-dir", help="override the data root, including its directory name")
    parser.add_argument("--token", default=os.environ.get("GITHUB_TOKEN", ""))
    parser.add_argument("--proxy", "--proxies", action="append",
                        help="ordered HTTPS proxy prefixes, comma separated or repeated; '' for direct only")
    parser.add_argument("--parts-dir", help="download cache (default <dest>/.wa2-release-parts/<tag>)")
    parser.add_argument("--jobs", type=int, choices=range(1, 5), default=1,
                        help="parallel part downloads (1-4; default: 1)")
    parser.add_argument("--ensure", action="store_true", help="skip networking if existing runtime data is usable")
    parser.add_argument("--remove-parts", action="store_true", help="remove ZIP cache after successful installation")
    parser.add_argument("--check-only", action="store_true", help="verify/extract cached parts without networking")
    parser.add_argument("--dry-run", action="store_true", help="check release completeness without downloading ZIPs")
    args = parser.parse_args(argv)

    dest = Path(args.dest).expanduser().resolve()
    data = Path(args.data_dir).expanduser().resolve() if args.data_dir else dest / "data"
    if args.ensure and runtime_ready(data):
        return 0
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
        raise FetchError("仓库名必须是 owner/repo。")
    proxies = proxy_list(args.proxy if args.proxy is not None else os.environ.get(
        "WA2_DATA_PROXIES", os.environ.get("WA2_DATA_PROXY", ",".join(DEFAULT_PROXIES))))
    cache_tag = urllib.parse.quote(args.tag, safe="")
    if cache_tag in ("", ".", ".."):
        raise FetchError("Release 标签不能为空或点路径。")
    parts_dir = Path(args.parts_dir).expanduser().resolve() if args.parts_dir else dest / PARTS_DIR_NAME / cache_tag
    say(f"资源缺失或安装未完成，准备自动安装到 {data}")
    say(f"Release：{args.repo} @ {args.tag}")
    offline = args.check_only
    if offline:
        path = parts_dir / "manifest.json"
        if not path.is_file():
            raise FetchError(f"找不到已缓存的清单：{path}")
        manifest = json.loads(path.read_text(encoding="utf-8"))
        assets = {}
    else:
        if args.token and proxies:
            say("已设置 GitHub 认证，资源使用 GitHub 直连。")
        try:
            release = find_release(args.repo, args.tag, args.token)
            assets = {a["name"]: a for a in release.get("assets", [])}
            if "manifest.json" not in assets or assets["manifest.json"].get("state", "uploaded") != "uploaded":
                raise FetchError("Release 缺少已上传完成的 manifest.json，资源还在准备；未开始下载分卷。")
            manifest = json.loads(asset_content(assets["manifest.json"], args.token, proxies))
        except (OSError, ValueError, FetchError) as exc:
            manifest = None if args.dry_run else cached_manifest(parts_dir)
            if manifest is None:
                raise
            say(f"无法读取在线清单（{network_error(exc)}），使用完整的本地缓存继续校验、解压。")
            offline = True
            assets = {}
    validate_manifest(manifest)
    parts = manifest["parts"]
    if not offline:
        check_assets(parts, assets)
    total = sum(p["size"] for p in parts)
    say(f"资源 {manifest.get('version', args.tag)}：{len(parts)} 卷，下载 {total / 1e9:.2f} GB，"
        f"解压后 {manifest['inventory']['bytes'] / 1e9:.2f} GB")
    if args.dry_run:
        say("检查通过；未下载分卷、未修改 data。")
        return 0

    parts_dir.mkdir(parents=True, exist_ok=True)
    data.mkdir(parents=True, exist_ok=True)
    with (parts_dir / ".install.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise FetchError("另一个启动脚本正在安装这些资源，请等待它完成。") from exc
        check_space(data, parts_dir, manifest)
        write_json(data / INSTALLING, {"repo": args.repo, "tag": args.tag})
        write_json(parts_dir / "manifest.json", manifest)
        if not offline and args.jobs > 1:
            download_parts(parts, assets, parts_dir, args.token, proxies, args.jobs)
        else:
            completed = 0
            for index, part in enumerate(parts, 1):
                target = parts_dir / part["name"]
                if offline:
                    if (not target.is_file() or target.stat().st_size != part["size"]
                            or sha256_file(target, Progress(f"校验 [{index}/{len(parts)}]", part["size"])) != part["sha256"]):
                        raise FetchError(f"缓存分卷缺失或校验失败：{target.name}")
                else:
                    say(f"下载 [{index}/{len(parts)}] {part['name']}")
                    download_asset(assets[part["name"]], target, args.token, part["sha256"], proxies,
                                   completed, total, f"下载 [{index}/{len(parts)}]")
                completed += part["size"]
        say("所有分卷 SHA-256 校验通过，开始解压……")
        extract_parts(parts_dir, data, manifest)
        say("检查解压后的资源目录……")
        if not verify_manifest_files(dest, manifest, data):
            raise FetchError("解压后的文件统计与清单不符，保留安装状态；重新运行可重试。")
        if args.ensure and not runtime_ready(data, ignore_installing=True):
            raise FetchError("清单中的资源不满足运行要求，请检查 Release 内容。")
        write_json(data / ".wa2-release.json", manifest)
        (data / INSTALLING).unlink()
        if args.remove_parts:
            for part in parts:
                (parts_dir / part["name"]).unlink()
            say("已清理下载的 ZIP 分卷。")
    say("资源准备完成，可以启动游戏。")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        say("\n安装已中断，已保留下载断点；重新运行 ./run.sh 可继续。")
        raise SystemExit(130)
    except (FetchError, OSError, ValueError, KeyError, zipfile.BadZipFile) as exc:
        print(f"资源安装失败：{network_error(exc)}", file=sys.stderr)
        raise SystemExit(1)
