#!/usr/bin/env bash
# Install the native fallback environment; invoked only after a launch failure
# or missing native dependencies. Do not run this script with sudo: only the
# distribution package manager needs root, while Rust belongs to the player.
set -euo pipefail

profile=${1:-play}
if [[ "$profile" != play && "$profile" != download ]]; then
  echo "用法：bash tools/install_runtime.sh [play|download]" >&2
  exit 2
fi

root_command=()
if [ "$EUID" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    root_command=(sudo)
  elif command -v doas >/dev/null 2>&1; then
    root_command=(doas)
  else
    echo "安装系统依赖需要管理员权限，请安装 sudo/doas 或由管理员安装运行依赖。" >&2
    exit 1
  fi
  echo "将安装必要的系统软件包；系统可能要求输入管理员密码。"
fi

if command -v apt-get >/dev/null 2>&1; then
  packages=(python3 ca-certificates)
  command -v curl >/dev/null 2>&1 || packages+=(curl)
  [ "$profile" = download ] || packages+=(build-essential pkg-config libsdl2-2.0-0 libfreetype6 ffmpeg)
  "${root_command[@]}" apt-get update
  "${root_command[@]}" apt-get install -y --no-install-recommends "${packages[@]}"
elif command -v dnf >/dev/null 2>&1; then
  packages=(python3 ca-certificates)
  command -v curl >/dev/null 2>&1 || packages+=(curl)
  [ "$profile" = download ] || packages+=(gcc gcc-c++ make pkgconf-pkg-config SDL2 freetype ffmpeg)
  "${root_command[@]}" dnf install -y "${packages[@]}"
elif command -v pacman >/dev/null 2>&1; then
  packages=(python ca-certificates)
  command -v curl >/dev/null 2>&1 || packages+=(curl)
  [ "$profile" = download ] || packages+=(base-devel pkgconf sdl2 freetype2 ffmpeg)
  "${root_command[@]}" pacman -S --needed --noconfirm "${packages[@]}"
elif command -v zypper >/dev/null 2>&1; then
  packages=(python3 ca-certificates)
  command -v curl >/dev/null 2>&1 || packages+=(curl)
  [ "$profile" = download ] || packages+=(gcc gcc-c++ make pkg-config libSDL2-2_0-0 libfreetype6 ffmpeg)
  "${root_command[@]}" zypper --non-interactive install --no-recommends "${packages[@]}"
else
  echo "暂不支持此发行版的自动安装。需要 Python 3.8+、C 编译器、SDL2、FreeType、ffmpeg/ffprobe 和 Rust。" >&2
  exit 1
fi

[ "$profile" = play ] || exit 0

cargo_env=${CARGO_HOME:-"$HOME/.cargo"}/env
if [ -f "$cargo_env" ]; then
  # shellcheck disable=SC1090
  . "$cargo_env"
fi

modern_rust() {
  local version
  version=$("$@" --version 2>/dev/null) || return 1
  [[ "$version" =~ rustc\ ([0-9]+)\.([0-9]+) ]] || return 1
  (( BASH_REMATCH[1] > 1 || (BASH_REMATCH[1] == 1 && BASH_REMATCH[2] >= 90) ))
}

if command -v rustup >/dev/null 2>&1; then
  if ! modern_rust rustup run stable rustc; then
    rustup toolchain install stable --profile minimal --no-self-update
  fi
elif ! modern_rust rustc; then
  echo "安装当前用户的 Rust stable 工具链……"
  installer=$(mktemp -t wa2-rustup.XXXXXXXX)
  trap 'rm -f -- "$installer"' EXIT
  curl --proto '=https' --tlsv1.2 -fL --retry 3 --connect-timeout 15 https://sh.rustup.rs -o "$installer"
  sh "$installer" -y --profile minimal --default-toolchain stable --no-modify-path
fi
