#!/usr/bin/env bash
# White Album 2 原生移植版启动脚本
#
# 数据目录:默认 ./data,可用 WA2_DATA 指定别处
# 语言:默认中文(存在 data/script_cn 时),用 WA2_LANG=jp 切日文
#
# 无参数:进入标题画面;带场景号:直接跳转该场景(调试用,如 ./run.sh 2003)
cd "$(dirname "$0")" || exit 1
# 高清素材链路：8x WebP 是唯一资源来源（data/bg_8x、data/sprite_8x）。
# 窗口大小决定实际解码倍率（WA2_RENDER_SCALE）；UI 贴图加载时归一化到 1280x720
# 逻辑尺寸。data/cache/ 下的 UI/时间线缓存由 tools/ 脚本生成，可加速加载。
# 加载最快：WA2_ART_SCALE=2 ./run.sh（用 2x TGA 代理树）；原图：WA2_ART_SCALE=1。
export WA2_ART_SCALE="${WA2_ART_SCALE:-8}"
BIN=./target/release/wa2-native
[ -x "$BIN" ] || BIN=./target/debug/wa2-native
if [ $# -eq 0 ]; then
  exec "$BIN" title
else
  exec "$BIN" run "$1"
fi
