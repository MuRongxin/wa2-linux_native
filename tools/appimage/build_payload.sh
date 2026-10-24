#!/bin/bash
# Runs inside the isolated Ubuntu baseline prepared by build_appimage.py.
set -euo pipefail
export PATH=/opt/rust/bin:/usr/bin:/bin
export CARGO_HOME=/build/cargo-home
export CARGO_TARGET_DIR=/build/cargo-target
export RUSTC=/opt/rust/bin/rustc
export RUSTFLAGS='-C target-cpu=x86-64 -C strip=symbols --remap-path-prefix=/project=.'
cd /project
cargo build --release --locked --offline

mkdir -p /build/sources /build/payload/usr/bin /build/payload/usr/lib
if [ ! -d /build/sources/SDL2-2.32.10 ]; then
  tar --no-same-owner -xf /build/downloads/SDL2-2.32.10.tar.gz -C /build/sources
fi
if [ ! -f /build/payload/usr/lib/libSDL2-2.0.so.0 ]; then
  mkdir -p /build/sdl-build
  cd /build/sdl-build
  /build/sources/SDL2-2.32.10/configure --prefix=/usr --libdir=/usr/lib \
    --disable-static --enable-shared --disable-video-kmsdrm --disable-video-directfb \
    --enable-x11-shared --enable-wayland-shared --enable-alsa-shared \
    --enable-pulseaudio-shared --disable-ibus --disable-fcitx
  make -j"${WA2_BUILD_JOBS:-8}"
  make DESTDIR=/build/payload install
fi

if [ ! -d /build/sources/ffmpeg-9.0.2 ]; then
  tar --no-same-owner -xf /build/downloads/ffmpeg-9.0.2.tar.xz -C /build/sources
fi
if [ ! -x /build/payload/usr/bin/ffmpeg ]; then
  mkdir -p /build/ffmpeg-build
  cd /build/ffmpeg-build
  /build/sources/ffmpeg-9.0.2/configure --prefix=/usr \
    --disable-everything --disable-autodetect --disable-network --disable-doc --disable-debug \
    --disable-ffplay --disable-avdevice --disable-shared --enable-static --enable-small \
    --enable-ffmpeg --enable-ffprobe --enable-avfilter --enable-swscale --enable-swresample --enable-zlib \
    --enable-protocol=file,pipe \
    --enable-demuxer=asf,ogg,wav,mov,matroska,avi,image2,image2pipe,webp_pipe,png_pipe,bmp_pipe,mp3,flac \
    --enable-decoder=wmv3,wmv2,vc1,wmav1,wmav2,wmapro,wmalossless,h264,hevc,aac,mp3,vorbis,flac,pcm_s16le,pcm_s16be,pcm_u8,pcm_s24le,pcm_s32le,pcm_f32le,pcm_f64le,adpcm_ms,adpcm_ima_wav,bmp,targa,webp,vp8,png,mjpeg \
    --enable-parser=h264,hevc,aac,mpegaudio,vorbis,vc1,flac,png \
    --enable-muxer=pcm_s16le,rawvideo,image2,image2pipe,wav,null \
    --enable-encoder=pcm_s16le,rawvideo,pam,targa,wrapped_avframe \
    --enable-filter=aresample,scale,format,aformat,anull,null,buffer,buffersink,abuffer,abuffersink
  make -j"${WA2_BUILD_JOBS:-8}"
  make DESTDIR=/build/payload install
fi
cp /build/cargo-target/release/wa2-native /build/payload/usr/bin/wa2-native
strip /build/payload/usr/bin/{wa2-native,ffmpeg,ffprobe}
