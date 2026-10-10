#!/usr/bin/env bash
# White Album 2 原生移植版启动脚本
# 无参数：进入开始界面（标题画面）
# 带场景号：直接跳转该场景（调试用，如 ./run.sh 2003）
cd "$(dirname "$0")" || exit 1
if [ $# -eq 0 ]; then
  exec ./target/debug/wa2-native title \
    assets/script assets/font/jp_body80.tga assets/font/charset.txt assets/bg
else
  exec ./target/debug/wa2-native run \
    assets/script assets/font/jp_body80.tga assets/font/charset.txt assets/bg "$1"
fi