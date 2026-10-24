"""AppImage entry point. Resource/config/save paths always stay outside AppDir."""

import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_runtime import check_runtime
from fetch_data import main as fetch_data
from fetch_data import FetchError, network_error


def project_root():
    if os.environ.get("WA2_PROJECT_ROOT"):
        return Path(os.environ["WA2_PROJECT_ROOT"]).expanduser().resolve()
    image = os.environ.get("APPIMAGE")
    root = Path(image).resolve().parent if image else Path.cwd()
    if root.name == "bin" and (root.parent / "run.sh").is_file():
        root = root.parent
    if os.access(root, os.W_OK):
        return root
    return Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share"))) / "wa2-native"


def main():
    args = sys.argv[1:]
    if args in (["--help"], ["-h"]):
        print("用法：wa2-native.AppImage [场景号 | title | --check | --download-only]\n"
              "缺少 data 时自动下载、校验、解压；资源和存档保存在程序外部。\n"
              "--check 检查运行环境和在线资源；--download-only 只准备资源。\n"
              "WA2_DATA 指定资源目录；WA2_DATA_PROXIES 自定义多个代理。")
        return 0
    internal = args and args[0] in ("--engine", "--check-runtime")
    if not internal and (len(args) > 1 or args and args[0] not in
                         ("title", "--check", "--download-only") and not args[0].isdigit()):
        print("无效参数；使用 --help 查看用法。", file=sys.stderr)
        return 2
    appdir = Path(os.environ["WA2_APPDIR"])
    root = project_root()
    data = Path(os.environ["WA2_DATA"]).expanduser().resolve() if os.environ.get("WA2_DATA") else root / "data"
    os.environ["WA2_DATA"] = str(data)
    engine = appdir / "usr/bin/wa2-native"
    if args and args[0] == "--engine":
        os.execv(str(engine), [str(engine), *args[1:]])
    mode = args[0] if args else "title"
    if mode != "--download-only":
        if not check_runtime(verbose=mode in ("--check", "--check-runtime")):
            return 78
    if mode == "--check-runtime":
        print(f"  Python: {sys.version.split()[0]} (内置)")
        print(f"  程序目录: {appdir}")
        print(f"  数据目录: {data}")
        return 0
    options = ["--dest", str(root), "--data-dir", str(data)]
    options += ["--dry-run"] if mode == "--check" else ["--ensure", "--remove-parts"]
    if os.environ.get("WA2_DATA_CACHE"):
        options += ["--parts-dir", os.environ["WA2_DATA_CACHE"]]
    try:
        result = fetch_data(options)
    except Exception as exc:
        # Do not turn a download, checksum or disk-space problem into a request
        # to install unrelated system packages in the outer run.sh fallback.
        print(f"资源准备失败：{network_error(exc)}", file=sys.stderr)
        return 75
    if result or mode in ("--check", "--download-only"):
        return result
    try:
        root.mkdir(parents=True, exist_ok=True)
        os.chdir(root)
    except OSError as exc:
        print(f"无法使用数据目录：{exc}", file=sys.stderr)
        return 75
    command = ["run", mode] if mode.isdigit() else ["title"]
    os.execv(str(engine), [str(engine), *command])


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\n安装已中断，下载断点已保留；重新运行可继续。", file=sys.stderr)
        raise SystemExit(130)
    except (FetchError, OSError, ValueError, KeyError) as exc:
        print(f"启动失败：{network_error(exc)}", file=sys.stderr)
        raise SystemExit(1)
