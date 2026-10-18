# WA2 数据发布 · data-v1.0.0

这是 `wa2-native`（《WHITE ALBUM 2》Linux 原生引擎重写）的完整离线素材包。
解压后不需要原版游戏安装目录，也不需要任何 `.pak` 文件。

> **版权声明**：本包内的图像、音频、影片均来自《WHITE ALBUM 2》原版游戏，版权归
> AQUAPLUS / Leaf 所有。本包仅用于**已购买原版游戏的玩家在私有环境自用**，
> 不得公开分发、不得用于商业用途。发布方不提供游戏本体程序。

---

## 一、包内容

| 目录 | 文件数 | 大小 | 说明 |
|---|---:|---:|---|
| `data/script/` | 379 | 0.019 GB | 日文剧本（LSCR 字节码 + 字符串池） |
| `data/script_cn/` | 379 | 0.019 GB | CK-GAL 简体中文补丁剧本 |
| `data/bg/` | 1,232 | 2.502 GB | 背景 / 界面素材（TGA + 91 个 BMP） |
| `data/sprite/` | 1,624 | 2.279 GB | 立绘 |
| `data/bg_cn/` | 31 | 0.024 GB | 中文补丁界面覆盖素材 |
| `data/voice/` | 46,005 | 1.688 GB | 语音（OGG） |
| `data/bgm/` | 108 | 0.185 GB | BGM（OGG） |
| `data/se/` | 1,132 | 0.945 GB | 音效（WAV/OGG） |
| `data/font/` | 4 | 0.025 GB | 位图字体 + cell map |
| `data/movie/` | 32 | 2.780 GB | 原版影片（16 部内容 × 720p/480p 两版） |
| `data/movie_2x/` | 16 | 4.734 GB | **高清重制影片：1280×720 → 2560×1440，H.264 + AAC** |
| `data/bg_8x/` | 1,197 | 15.219 GB | **8x WebP 背景/界面** |
| `data/sprite_8x/` | 1,623 | 7.119 GB | **8x WebP 立绘** |
| `data/bg_cn_8x/` | 31 | 0.089 GB | **8x 中文界面素材** |
| **合计** | **53,793** | **37.63 GB** | |

说明：

- 8x 图由 Real-ESRGAN x4（anime 6B）+ waifu2x swin_unet art x2 生成，无损 WebP。
  其中 7 张图（`v200000`、`v201900`、`スポットライト`、`ライブシーン`、`湯煙`、`煙`、
  `雨640x360`）因 8x 尺寸超过 WebP 16383px 上限，实际为 4x；引擎按文件实际像素自动识别。
- `data/movie_2x/` 的 16 部影片由 Real-ESRGAN 官方动画视频模型
  `realesr-animevideov3`（x4 推理后缩放到 2x）生成；原 480p 副版本未重制。
- 原始影片 `data/movie/` 仍保留，可作备份；引擎会优先读取 `movie_2x/`。
- `data/cache/`、`data/*_2x/`（2x 代理树）属于衍生数据，**不在包内**，可用
  `tools/build_ui_cache.py`、`tools/build_timeline_cache.py` 重新生成。

## 二、下载与解压

发布物是 **21 个独立 ZIP 分卷，合计 34.14 GB**（`data-v1.0.0.part001.zip` … `part021.zip`），
**每一个分卷都是完整可解压的压缩包**，解压到同一目录就会合并成 `data/` 树——
不需要拼接、不需要 zstd/tar。

### Windows

1. 下载全部分卷到同一个文件夹
2. 全选 → 右键 → **7-Zip → 解压到 "data-v1.0.0\\"**（7-Zip 支持一次解压多个 zip 到同一目录）
3. 或逐个用资源管理器"全部解压缩"到同一目标文件夹

### macOS

1. 下载全部分卷
2. 逐个双击（归档实用工具）解压到同一文件夹；或终端：
   ```bash
   for f in data-v1.0.0.part*.zip; do unzip -oq "$f" -d .; done
   ```

### Linux / WSL

```bash
for f in data-v1.0.0.part*.zip; do unzip -oq "$f" -d .; done
# 或使用自动下载/校验/解压工具：
GITHUB_TOKEN=ghp_xxx python3 tools/fetch_data.py \
    --repo MuRongxin/wa2-linux_native --tag data-v1.0.0 --dest .
```

> `unzip` 一次只接受一个压缩包，所以要用上面的循环（或 `tools/fetch_data.py`）。

## 三、运行

依赖（Linux）：`libSDL2-2.0.so.0`、`libfreetype.so.6`、`ffmpeg`/`ffprobe`，
推荐安装 `dwebp`（libwebp-tools）以加速 8x WebP 解码。

```bash
cargo build --release
./run.sh                 # 默认 8x 素材（WA2_ART_SCALE=8）
./run.sh 1002            # 直接跳到场景 1002
WA2_ART_SCALE=2 ./run.sh # 可选：2x TGA 代理树（需先跑 tools/build_2x_proxy.py）
```

首次运行后建议重建缓存（只影响加载速度，不重建也能运行）：

```bash
python3 tools/build_ui_cache.py
python3 tools/build_timeline_cache.py
```

## 四、分卷与校验

- 共 21 个分卷，合计 34.14 GB；每卷 ≤ 1.71 GiB（GitHub Release 单文件上限 2 GiB），各自独立可解压
- `SHA256SUMS`：每个分卷的 SHA-256，下载后可用 `sha256sum -c SHA256SUMS` 校验
- `manifest.json`：版本、目录清单、文件数/字节数、分卷校验和、解压命令
- 解压后对照 `manifest.json` 的 `inventory` 检查文件数/字节数
- 旧的 tar.zst 拼接流格式已废弃（2026-10-18 起改为 ZIP 分卷）

## 五、可选：4x 素材包（data-v1.0.0-4x）

引擎默认使用 8x 树；只有设置 `WA2_ART_SCALE=4` 时才需要 4x 树：

| 目录 | 文件数 | 大小 |
|---|---:|---:|
| `data/bg_4x/` | 1,197 | 5.0 GB |
| `data/sprite_4x/` | 1,623 | 2.3 GB |
| `data/bg_cn_4x/` | 31 | 27 MB |
| 合计 | 2,851 | 约 7.3 GB |

如需要，可用 `tools/package_data.py --version data-v1.0.0-4x --with-4x` 单独打包。

## 六、构建信息

- 引擎：`wa2-native`（Rust，零 crate，手写 SDL2/FreeType FFI）
- 图片放大：Real-ESRGAN x4plus_anime_6B（PyTorch fp16）+ waifu2x nunif swin_unet art x2，
  一次请求同时返回 4x 与 8x 两段，无损 WebP
- 影片重制：Real-ESRGAN `realesr-animevideov3`（x4）→ 2x（2560×1440），
  H.264 CRF18 + AAC，4 进程并行
- 数据生成日期：2026-10-18
