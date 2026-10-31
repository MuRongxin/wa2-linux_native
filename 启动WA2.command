#!/bin/bash
# Finder-friendly entry point; uses the checked native-source launcher.
set -u
export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH}"
ROOT="$(cd -- "$(dirname -- "$0")" && pwd)" || exit 1
"$ROOT/run-macos.sh" "$@"
status=$?
if [ "$status" -ne 0 ]; then
    printf '\n启动失败（退出码 %s）。请查看上面的错误。按回车关闭窗口。\n' "$status"
    read -r _
fi
exit "$status"
