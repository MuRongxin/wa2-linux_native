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
  --download-only    只下载、校验和解压资源，不构建、不启动游戏
  --help             显示帮助

缺少 data 时自动下载 data-v1.1.0，通过 gh-proxy 加速，支持断点续传。
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

BIN=./target/release/wa2-native
[ -x "$BIN" ] || BIN=./target/debug/wa2-native
if ! command -v python3 >/dev/null 2>&1; then
  echo "启动时检查/下载资源需要 Python 3，请先安装 python3。" >&2
  exit 1
fi
python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 8) else "需要 Python 3.8 或更新版本。")'
if [ "$mode" != download ]; then
  runtime_args=()
  [ -x "$BIN" ] || runtime_args+=(--need-build)
  if [ "$mode" = check ]; then
    echo "检查启动环境……"
    runtime_args+=(--verbose)
  fi
  python3 tools/check_runtime.py "${runtime_args[@]}"
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

if [ ! -x "$BIN" ]; then
  echo "首次启动：正在构建游戏……"
  cargo build --release
  BIN=./target/release/wa2-native
fi
# 默认窗口读 4x 树、全屏读 8x 母版；WA2_ART_SCALE=2 可用 2x 快速路径。
if [ -z "$scene" ]; then
  exec "$BIN" title
else
  exec "$BIN" run "$scene"
fi
