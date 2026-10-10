#!/usr/bin/env bash
# White Album 2 原生移植版启动脚本
# 用法: ./run.sh [起始场景号]   （默认 1001，即游戏开场）
cd "$(dirname "$0")" || exit 1
exec ./target/debug/wa2-native run \
  assets/script assets/font/jp_body80.tga assets/font/charset.txt assets/bg \
  "${1:-1001}"