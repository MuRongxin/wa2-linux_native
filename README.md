# wa2-native

《WHITE ALBUM 2》的 Linux 原生引擎。

## 启动

准备 Python 3.8+、SDL2、FreeType、`ffmpeg` / `ffprobe`。推荐安装 `dwebp`
（Debian/Ubuntu 软件包名为 `webp`），可加快 WebP 解码。中文字体随仓库的
`fonts/` 提供。

从源码 clone 后还需要 Rust/Cargo；缺少游戏可执行文件时，启动脚本会执行
`cargo build --release`。已有 `target/release/wa2-native` 或 debug 版本时直接使用。

```bash
./run.sh           # 标题画面
./run.sh 1002      # 跳转场景
./run.sh --check   # 检查依赖和在线 Release，不下载分卷、不构建、不启动
./run.sh --download-only  # 只准备资源，适合先下载再游玩
```

第一次运行没有可用 `data/` 时，脚本会自动从 GitHub Release **`data-v1.1.0`**
下载资源，默认按以下顺序尝试下载通道：

1. `https://gh-proxy.com/`
2. `https://ghfast.top/`
3. `https://ghproxy.net/`
4. GitHub 直连

失败时自动切换；代理持续 20 秒低于 128 KiB/s 时也会切换，并保留当前分卷断点。
下载前检查所有分卷是否上传完成；终端显示实时下载进度、总进度、速度、剩余时间，
随后显示 SHA-256 校验和解压进度。Release 仍在准备、缺少清单或分卷时会明确报错。

中断后重新运行即可续传。只有全部校验、解压和文件统计检查通过才启动游戏。
安装完成后清理下载的 ZIP，保留资源；存档和玩家配置不会被资源包覆盖。
已有可用数据时无需联网；如果分卷已全部缓存，网络故障时仍可继续校验和解压。
日常启动只检查关键文件和目录，不逐文件重新校验全部素材。

启动前先检查 SDL2、FreeType、ffmpeg / ffprobe 和必要的 Rust 工具链，缺失时给出提示。
`--download-only` 只需要 Python，不要求游戏运行库或 Rust。
安装期间需要同时存放压缩包和解压后的资源，首次安装建议预留约 100 GB，脚本会先检查可用空间。
下载断点保存在 `.wa2-release-parts/`，该目录已忽略。

## 可选设置

```bash
WA2_DATA=/path/to/data ./run.sh       # 自定义资源目录，也适用于自动安装
WA2_DATA_PROXIES= ./run.sh            # 关闭全部加速代理
WA2_DATA_PROXIES='https://ghfast.top/,https://ghproxy.net/' ./run.sh  # 自定义代理顺序
WA2_DATA_TAG=data-v1.1.0 ./run.sh      # 指定 GitHub Release 标签
WA2_ART_SCALE=2 ./run.sh              # 使用 2x 快速素材
WA2_LANG=jp ./run.sh                  # 日文
```

`WA2_DATA_REPO` 可覆盖默认仓库 `MuRongxin/wa2-linux_native`；`WA2_DATA_CACHE`
可指定下载缓存目录。私有 Release 可设置 `GITHUB_TOKEN`，此时使用 GitHub 认证直连。
兼容旧的 `WA2_DATA_PROXY` 单代理变量；同时设置时以 `WA2_DATA_PROXIES` 为准。

## 资源发布要求

`tools/package_data.py` 生成的 `manifest.json` 和其中列出的**全部 ZIP 分卷**必须
放在同一个 Release 下。建议同时上传 `SHA256SUMS`，方便手动安装时校验。
Release 标签与分卷文件名前缀是两个独立字段；启动脚本按 `WA2_DATA_TAG` 定位 Release，
再按清单中的名称下载分卷。

下载工具可单独使用；`--dry-run` 只读取清单、检查附件，不下载大文件：

```bash
python3 tools/fetch_data.py --tag data-v1.1.0 --dry-run
python3 -m unittest discover -s tools -p test_fetch_data.py
```
