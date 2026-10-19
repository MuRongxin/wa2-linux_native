#!/usr/bin/env bash
# White Album 2 原生移植版启动脚本
#
# 数据目录:默认 ./data,可用 WA2_DATA 指定别处
# 语言:默认中文(存在 data/script_cn 时),用 WA2_LANG=jp 切日文
#
# 无参数:进入标题画面;带场景号:直接跳转该场景(调试用,如 ./run.sh 2003)
cd "$(dirname "$0")" || exit 1
# 高清素材链路：引擎默认读 4x WebP 树（data/bg_4x、data/sprite_4x），解码也是 4x，
# 图源倍率与解码倍率对齐，首次解码约 0.4s。8x 树是母版，只在
# WA2_RENDER_SCALE=8（5K+ 画布）或显式 WA2_ART_SCALE=8 时读取。
# 想用母版 8x 源：WA2_ART_SCALE=8 ./run.sh；2x TGA 代理：WA2_ART_SCALE=2；原图：1。
BIN=./target/release/wa2-native
[ -x "$BIN" ] || BIN=./target/debug/wa2-native
if [ $# -eq 0 ]; then
  exec "$BIN" title
else
  exec "$BIN" run "$1"
fi
