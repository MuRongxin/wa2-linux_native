#!/usr/bin/env bash
set -euo pipefail

# 云端 Real-ESRGAN x4 + Waifu2x x2 API 启动脚本。
# 放在 upscale_api.py、两个 ncnn 目录同级的位置运行。
cd "$(dirname "$0")"

export WA2_GPU="${WA2_GPU:-0}"
export WA2_TILE="${WA2_TILE:-128}"
export WA2_REAL_JOBS="${WA2_REAL_JOBS:-1:1:1}"
export WA2_WAIFU_JOBS="${WA2_WAIFU_JOBS:-1:1:1}"
export WA2_WORKERS="${WA2_WORKERS:-1}"
export WA2_API_TOKEN="${WA2_API_TOKEN:-wa2-local-5090}"
export WA2_TMP="${WA2_TMP:-$PWD/wa2-api-tmp}"

mkdir -p "$WA2_TMP"

PIDFILE="${WA2_API_PIDFILE:-$PWD/upscale_api.pid}"
LOGFILE="${WA2_API_LOGFILE:-$PWD/api.log}"
PORT="${WA2_API_PORT:-6006}"

if [[ -f "$PIDFILE" ]]; then
    old_pid="$(cat "$PIDFILE" 2>/dev/null || true)"
    if [[ "$old_pid" =~ ^[0-9]+$ ]] && kill -0 "$old_pid" 2>/dev/null; then
        echo "upscale API already running: PID $old_pid"
        exit 0
    fi
    rm -f "$PIDFILE"
fi

nohup python3 -m uvicorn upscale_api:app \
    --host 0.0.0.0 --port "$PORT" \
    >>"$LOGFILE" 2>&1 < /dev/null &
pid=$!
echo "$pid" > "$PIDFILE"
sleep 1
if ! kill -0 "$pid" 2>/dev/null; then
    echo "failed to start; see $LOGFILE" >&2
    exit 1
fi
echo "upscale API started: PID $pid, port $PORT"
echo "log: $LOGFILE"
