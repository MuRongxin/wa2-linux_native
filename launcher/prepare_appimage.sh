#!/usr/bin/env bash
# Bootstrap the pinned player using only Bash, curl/wget and coreutils.
# Exit 69: missing bootstrap tools; 75: download/storage error; 78: runtime
# incompatible. A download failure must never trigger a Rust installation.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
mode=${1:---ensure}
[[ "$mode" = --ensure || "$mode" = --check ]] || exit 2
# This file is part of the source checkout, never downloaded or evaluated.
source launcher/release.conf
repo=${WA2_APPIMAGE_REPO:-$APPIMAGE_RELEASE_REPO}
tag=${WA2_APPIMAGE_TAG:-$APPIMAGE_RELEASE_TAG}
digest=${WA2_APPIMAGE_SHA256:-$APPIMAGE_RELEASE_SHA256}
size=${WA2_APPIMAGE_SIZE:-$APPIMAGE_RELEASE_SIZE}
unpacked=${APPIMAGE_UNPACKED_SIZE:-536870912}
name=wa2-native-linux-x86_64.AppImage
if [[ ! "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ||
      ! "$tag" =~ ^[A-Za-z0-9_-][A-Za-z0-9_.-]*$ ||
      ! "$digest" =~ ^[0-9a-f]{64}$ || ! "$size" =~ ^[1-9][0-9]*$ ]]; then
  echo "AppImage 版本、大小或 SHA-256 配置无效。" >&2
  exit 75
fi
image="$PWD/bin/$name"
runtime="$PWD/bin/runtime/$digest"
url="https://github.com/$repo/releases/download/$tag/$name"
verify() {
  [ -f "$image" ] && [ "$(stat -Lc %s "$image")" = "$size" ] &&
    [ "$(sha256sum "$image" | cut -d ' ' -f 1)" = "$digest" ]
}
for tool in sha256sum stat cut; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "缺少 AppImage 准备工具：$tool" >&2
    exit 69
  fi
done

sources=()
setting=${WA2_APPIMAGE_PROXIES-${WA2_DATA_PROXIES-${WA2_DATA_PROXY-https://gh-proxy.com/,https://ghfast.top/,https://ghproxy.net/}}}
setting=${setting//$'\n'/ }
IFS=$',; \t' read -r -a prefixes <<< "$setting"
for prefix in "${prefixes[@]}"; do
  [[ -n "$prefix" ]] || continue
  if [[ ! "$prefix" =~ ^https://[A-Za-z0-9.-]+(:[0-9]+)?(/[^?\#[:space:]]*)?$ ]]; then
    echo "AppImage 代理必须是 HTTPS 地址，不能含认证信息、查询或片段。" >&2
    exit 75
  fi
  sources+=("${prefix%/}/$url")
done
sources+=("$url")
downloader=
command -v curl >/dev/null 2>&1 && downloader=curl
if [ -z "$downloader" ] && command -v wget >/dev/null 2>&1; then downloader=wget; fi

if [ "$mode" = --check ]; then
  echo "程序 Release：$repo @ $tag"
  echo "构建基线：glibc 2.31；本机：$(getconf GNU_LIBC_VERSION 2>/dev/null || echo unknown)"
  if verify; then
    echo "AppImage：本地文件 SHA-256 正确。"
    if [ -x "$runtime/squashfs-root/AppRun" ] && [ -x "$runtime/squashfs-root/usr/bin/wa2-native" ] &&
       [ "$(cat "$runtime/.complete" 2>/dev/null || true)" = "$digest" ]; then
      echo "运行目录：已缓存。"
    else
      echo "运行目录：正常启动时解包（本次不写入文件）。"
    fi
    exit 0
  fi
  if [ -z "$downloader" ]; then
    echo "缺少 curl/wget，无法检查程序下载地址；本次未安装软件。" >&2
    exit 69
  fi
  for source in "${sources[@]}"; do
    if [ "$downloader" = curl ]; then
      if curl --proto '=https' --proto-redir '=https' --fail --silent --show-error --location --head --connect-timeout 10 --max-time 20 "$source" >/dev/null; then
        echo "AppImage：Release 地址可访问，尚未下载或本地版本需要替换。"
        exit 0
      fi
    elif wget --https-only --spider --quiet --timeout=20 --tries=1 "$source"; then
      echo "AppImage：Release 地址可访问，尚未下载或本地版本需要替换。"
      exit 0
    fi
  done
  echo "AppImage Release 暂不可访问；未下载程序，也未安装本机环境。" >&2
  exit 75
fi

glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null || true)
if [[ ! "$glibc" =~ ^glibc\ ([0-9]+)\.([0-9]+) ]] ||
   (( BASH_REMATCH[1] < 2 || (BASH_REMATCH[1] == 2 && BASH_REMATCH[2] < 31) )); then
  echo "AppImage 需要 glibc 2.31+，将使用本机编译版本。" >&2
  exit 78
fi
if ! command -v flock >/dev/null 2>&1; then
  echo "缺少 flock（util-linux）。" >&2
  exit 69
fi
mkdir -p bin/.downloads bin/runtime
exec 9>bin/.appimage.lock
if ! flock -w 30 9; then
  echo "另一个进程正在准备 AppImage，请稍后重试。" >&2
  exit 75
fi
if ! verify; then
  if [ -z "$downloader" ]; then
    echo "下载 AppImage 需要 curl 或 wget。" >&2
    exit 69
  fi
  partial="$PWD/bin/.downloads/$tag-$digest.AppImage.part"
  echo "下载 AppImage：$repo @ $tag（$((size / 1024 / 1024)) MiB）"
  if [ -f "$partial" ] && [ "$(stat -Lc %s "$partial")" -gt "$size" ]; then rm -f -- "$partial"; fi
  complete=0
  # A complete .part may have been left by cancellation just before rename.
  if [ -f "$partial" ] && [ "$(stat -Lc %s "$partial")" = "$size" ]; then
    if [ "$(sha256sum "$partial" | cut -d ' ' -f 1)" = "$digest" ]; then complete=1; else rm -f -- "$partial"; fi
  fi
  for source in "${sources[@]}"; do
    [ "$complete" = 0 ] || break
    echo "下载通道：${source%%/https://*}"
    result=0
    if [ "$downloader" = curl ]; then
      # curl's standard meter shows bytes, percent, speed and remaining time.
      curl --proto '=https' --proto-redir '=https' --fail --show-error --location --connect-timeout 15 --retry 1 --retry-delay 1 \
        --speed-time 20 --speed-limit 131072 --continue-at - --output "$partial" "$source" || result=$?
      if [ "$result" = 33 ] && [ "$source" = "$url" ]; then
        echo "直连服务器不支持断点，重新下载当前程序包。"
        rm -f -- "$partial"
        curl --proto '=https' --proto-redir '=https' --fail --show-error --location --connect-timeout 15 --retry 1 \
          --speed-time 20 --speed-limit 131072 --output "$partial" "$source" || result=$?
        # Recheck below regardless of the preceding transport status.
      fi
    else
      wget --https-only --continue --timeout=30 --tries=2 --progress=bar:force \
        --output-document="$partial" "$source" || result=$?
    fi
    if [ -f "$partial" ] && [ "$(stat -Lc %s "$partial")" = "$size" ]; then
      echo "校验 AppImage SHA-256……"
      if [ "$(sha256sum "$partial" | cut -d ' ' -f 1)" = "$digest" ]; then
        complete=1
        break
      fi
      echo "校验失败，丢弃当前下载内容并切换通道。" >&2
      rm -f -- "$partial"
    else
      echo "下载未完成（状态 $result），保留断点并切换通道。" >&2
    fi
  done
  if [ "$complete" = 0 ]; then
    echo "AppImage 下载失败，断点已保留；重新运行 ./run.sh 可继续。未安装编译环境。" >&2
    exit 75
  fi
  chmod u+x "$partial"
  mv -f -- "$partial" "$image"
fi

if [ -x "$runtime/squashfs-root/AppRun" ] && [ -x "$runtime/squashfs-root/usr/bin/wa2-native" ] &&
   [ "$(cat "$runtime/.complete" 2>/dev/null || true)" = "$digest" ]; then
  echo "复用 AppImage 运行目录。"
  exit 0
fi
available=$(df -Pk bin | awk 'NR==2 {printf "%.0f", $4 * 1024}')
if [ -n "$available" ] && (( ${available%.*} < unpacked + 67108864 )); then
  echo "空间不足，解包 AppImage 需要约 $((unpacked / 1024 / 1024 + 64)) MiB。" >&2
  exit 75
fi
stage=$(mktemp -d "$PWD/bin/runtime/.unpack.XXXXXXXX")
trap '[ -z "$stage" ] || rm -rf -- "$stage"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
echo "首次解包 AppImage，后续启动将复用此版本……"
chmod u+x "$image"
log="$PWD/bin/.downloads/extract-$digest.log"
if ! (cd "$stage" && env -u APPIMAGE_EXTRACT_AND_RUN -u NO_CLEANUP "$image" --appimage-extract >"$log" 2>&1); then
  tail -n 12 "$log" >&2
  echo "AppImage 无法解包，将改用本机环境。" >&2
  exit 78
fi
if [ ! -x "$stage/squashfs-root/AppRun" ] || [ ! -x "$stage/squashfs-root/usr/bin/wa2-native" ]; then
  echo "AppImage 缺少可运行的 AppRun 或引擎。" >&2
  exit 78
fi
printf '%s\n' "$digest" > "$stage/.complete"
rm -rf -- "$runtime"
mv -- "$stage" "$runtime"
stage=
echo "AppImage 已准备完成。"
