#!/usr/bin/env bash
# White Album 2 原生移植版启动脚本
#
# 数据目录:默认 ./data,可用 WA2_DATA 指定别处
# 语言:默认中文(存在 data/script_cn 时),用 WA2_LANG=jp 切日文
#
# 无参数:进入标题画面;带场景号:直接跳转该场景(调试用,如 ./run.sh 2003)
# 缺少 data 时自动下载、校验并解压（Python 3 标准库，无需 pip）。
# WA2_DATA_TAG / WA2_DATA_REPO 可切换资源发布；WA2_DATA_PROXIES 可配置多个代理。
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

usage() {
  cat <<'HELP'
用法：./run.sh [场景号 | title | --check | --download-only]
  无参数             检查资源并进入标题画面
  场景号             直接进入指定场景，例如 ./run.sh 1002
  --check            检查启动依赖和 Release 附件，不下载分卷、不启动游戏
  --download-only    只准备程序与资源，不构建、不启动游戏
  --help             显示帮助

缺少 data 时自动下载 data-v1.1.0，通过 gh-proxy 加速，支持断点续传。
缺少 AppImage 时，从独立的程序 Release 下载、校验，并缓存解包目录。
下载失败保留断点；仅在程序不兼容或无法运行时安装本机环境并编译。
WA2_BUILD_FROM_SOURCE=1 使用本地构建版本。
WA2_AUTO_INSTALL=0 禁用自动安装系统软件包；--check 始终只检查。
WA2_APPIMAGE_PROXIES 可单独设置程序下载代理（默认沿用资源代理）。
WA2_DATA 指定资源目录；WA2_DATA_PROXIES 配置代理列表；WA2_DATA_PROXIES= 关闭代理。
兼容 WA2_DATA_PROXY；WA2_DATA_TAG 可切换发布标签。
HELP
}

mode=play
scene=
if [ $# -gt 1 ]; then
  usage >&2
  exit 2
fi
case "${1:-}" in
  --help|-h) usage; exit 0 ;;
  --check) mode=check ;;
  --download-only) mode=download ;;
  ''|title) ;;
  *)
    if [[ "$1" =~ ^[0-9]+$ ]]; then
      scene=$1
    else
      echo "无效参数：$1" >&2
      usage >&2
      exit 2
    fi
    ;;
esac

force_build=0
installed_environment=0
appimage_failed=0

load_cargo_environment() {
  local cargo_env=${CARGO_HOME:-"$HOME/.cargo"}/env
  if [ -f "$cargo_env" ]; then
    # shellcheck disable=SC1090
    . "$cargo_env"
  fi
}

install_environment() {
  if [ "$mode" = check ] || [ "${WA2_AUTO_INSTALL:-1}" = 0 ]; then
    echo "未自动安装软件包。直接运行 ./run.sh 可自动准备环境，或手动安装依赖。" >&2
    return 1
  fi
  local profile=play
  [ "$mode" != download ] || profile=download
  echo "正在安装本机启动所需的环境……"
  if ! bash tools/install_runtime.sh "$profile"; then
    echo "环境安装未完成，请处理上面的包管理器/权限错误后重新运行。" >&2
    return 1
  fi
  installed_environment=1
  load_cargo_environment
  if [ "$profile" = play ] && command -v rustup >/dev/null 2>&1; then
    export RUSTUP_TOOLCHAIN=stable
  fi
}

# Program and data releases have separate, pinned versions. The bootstrapper
# does not need Python: the verified AppImage supplies its own interpreter.
source tools/appimage/release.conf
appimage=./bin/wa2-native-linux-x86_64.AppImage
appimage_digest=${WA2_APPIMAGE_SHA256:-$APPIMAGE_RELEASE_SHA256}
appimage_key="$appimage_digest:$(uname -m):$(getconf GNU_LIBC_VERSION 2>/dev/null || true)"
native_marker=./target/.wa2-native-fallback
prefer_native=0
if [ -f "$native_marker" ] && [ -x ./target/release/wa2-native ] &&
   [ "$(cat "$native_marker")" = "$appimage_key" ]; then
  prefer_native=1
  echo "使用本机编译版本。"
fi
if [ "${WA2_BUILD_FROM_SOURCE:-0}" != 1 ] && [ "$prefer_native" = 0 ] && [ "$(uname -m)" = x86_64 ]; then
  prepare_mode=--ensure
  [ "$mode" != check ] || prepare_mode=--check
  prepare_status=0
  bash tools/prepare_appimage.sh "$prepare_mode" || prepare_status=$?
  if [ "$prepare_status" = 69 ] && [ "$mode" != check ] && [ "${WA2_AUTO_INSTALL:-1}" != 0 ]; then
    # Only the downloader/coreutils are needed here, never Rust or game libs.
    bash tools/install_runtime.sh bootstrap || exit 1
    prepare_status=0
    bash tools/prepare_appimage.sh "$prepare_mode" || prepare_status=$?
  fi
  case "$prepare_status" in
    0) ;;
    78) appimage_failed=1; force_build=1 ;;
    *) exit "$prepare_status" ;;
  esac
  if [ "$prepare_status" = 0 ]; then
    appdir="$PWD/bin/runtime/$appimage_digest/squashfs-root"
    if [ "$mode" = check ] && { [ ! -x "$appdir/AppRun" ] || [ ! -x "$appdir/usr/bin/wa2-native" ] ||
       [ "$(cat "${appdir%/squashfs-root}/.complete" 2>/dev/null || true)" != "$appimage_digest" ]; }; then
      # A check must not extract the image or install Python just to inspect
      # data. Use host Python when available, otherwise explain the omission.
      if command -v python3 >/dev/null 2>&1; then
        python3 -u tools/fetch_data.py --dry-run --dest "$PWD" \
          --data-dir "${WA2_DATA:-$PWD/data}" \
          --repo "${WA2_DATA_REPO:-MuRongxin/wa2-linux_native}" \
          --tag "${WA2_DATA_TAG:-data-v1.1.0}"
      else
        echo "未检查素材清单：当前没有系统 Python；正常启动将使用 AppImage 内置 Python。"
      fi
      exit 0
    fi
    if env APPDIR="$appdir" APPIMAGE="$PWD/${appimage#./}" WA2_PROJECT_ROOT="$PWD" "$appdir/AppRun" "$@"; then
      exit 0
    else
      image_status=$?
    fi
    case "$image_status" in
      2|75|130|137|143) exit "$image_status" ;;
    esac
    echo "AppImage 无法运行（退出码 $image_status），改用本机环境。" >&2
    appimage_failed=1
    force_build=1
  fi
  if [ "$mode" != check ]; then
    install_environment || exit 1
  fi
fi

load_cargo_environment
BIN=./target/release/wa2-native
[ -x "$BIN" ] || BIN=./target/debug/wa2-native

native_environment_ready() {
  if ! command -v python3 >/dev/null 2>&1; then
    echo "缺少 Python 3，准备安装。" >&2
    return 1
  fi
  python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 8) else "需要 Python 3.8 或更新版本。")' || return 1
  if [ "$mode" != download ]; then
    local runtime_args=()
    if [ ! -x "$BIN" ] || [ "$force_build" = 1 ]; then
      runtime_args+=(--need-build)
    fi
    if [ "$mode" = check ]; then
      echo "检查启动环境……"
      runtime_args+=(--verbose)
    fi
    python3 tools/check_runtime.py "${runtime_args[@]}" || return 1
  fi
}

if ! native_environment_ready; then
  [ "$installed_environment" = 0 ] || exit 1
  install_environment || exit 1
  native_environment_ready || exit 1
fi
if [ "$mode" = play ] && { [ ! -x "$BIN" ] || [ "$force_build" = 1 ]; }; then
  echo "正在为这台机器构建游戏……"
  cargo build --release --target-dir "$PWD/target"
  BIN=./target/release/wa2-native
  if [ "$appimage_failed" = 1 ]; then
    printf '%s\n' "$appimage_key" > "$native_marker"
  fi
fi
WA2_DATA="$(python3 -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).expanduser().resolve())' "${WA2_DATA:-./data}")"
export WA2_DATA

fetch_args=(
  --repo "${WA2_DATA_REPO:-MuRongxin/wa2-linux_native}"
  --tag "${WA2_DATA_TAG:-data-v1.1.0}"
  --dest "$PWD" --data-dir "$WA2_DATA"
)
if [ "$mode" = check ]; then
  fetch_args+=(--dry-run)
else
  fetch_args+=(--ensure --remove-parts)
fi
if [ -n "${WA2_DATA_CACHE:-}" ]; then
  fetch_args+=(--parts-dir "$WA2_DATA_CACHE")
fi
python3 -u tools/fetch_data.py "${fetch_args[@]}"
if [ "$mode" != play ]; then
  exit 0
fi

# 默认窗口读 4x 树、全屏读 8x 母版；WA2_ART_SCALE=2 可用 2x 快速路径。
if [ -z "$scene" ]; then
  exec "$BIN" title
else
  exec "$BIN" run "$scene"
fi
