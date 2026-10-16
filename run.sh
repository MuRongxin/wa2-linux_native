#!/usr/bin/env bash
# White Album 2 原生移植版启动脚本
#
# 数据目录:默认 ./data,可用 WA2_DATA 指定别处
# 语言:默认中文(存在 data/script_cn 时),用 WA2_LANG=jp 切日文
#
# 无参数:进入标题画面;带场景号:直接跳转该场景(调试用,如 ./run.sh 2003)
cd "$(dirname "$0")" || exit 1
# 游戏引擎优先使用完整的 *_8x WebP 素材树；目录不完整时自动回退原图。
export WA2_ART_SCALE="${WA2_ART_SCALE:-8}"
BIN=./target/release/wa2-native
[ -x "$BIN" ] || BIN=./target/debug/wa2-native
if [ $# -eq 0 ]; then
  exec "$BIN" title
else
  exec "$BIN" run "$1"
fi
