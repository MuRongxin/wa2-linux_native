<div align="center">

<img src="launcher/wa2-native.svg" width="96" height="96" alt="wa2-native 雪花图标">

<h1>WHITE ALBUM 2 · Native</h1>

<p><strong>让白色相簿的冬天，在 Linux 原生运行。</strong></p>

<p>用 Rust 重写的《WHITE ALBUM 2》原生引擎<br>中文对白 · 原版界面 · 高清素材 · 一条命令启动</p>

<p>
  <code>Linux x86_64</code>&nbsp;
  <code>Rust · 零第三方 crate</code>&nbsp;
  <code>2x / 4x / 8x</code>&nbsp;
  <code>AppImage · 自动获取</code>
</p>

<p>
  <a href="#快速开始">快速开始</a> ·
  <a href="#游玩与操作">游玩与操作</a> ·
  <a href="#画质与设置">画质与设置</a> ·
  <a href="#常见问题">常见问题</a> ·
  <a href="#开发与打包">开发与打包</a>
</p>

</div>

---

这是《WHITE ALBUM 2》的 **Linux 原生引擎重写**。引擎直接读取游戏素材，重新实现剧本解释、画面合成、音频、影片和界面；运行时无需 Wine、原版 EXE 或原版安装目录。

| 原生体验 | 画面与文字 |
| :--- | :--- |
| **剧情与演出** — 剧本推进、分支选择、语音、BGM、音效与影片播放 | **高清素材** — 同时提供 2x、4x、8x；默认窗口化使用 4x，全屏使用 8x |
| **熟悉的界面** — 保留游戏自己的标题、设置、存档与底部指令条素材 | **中文优先** — 默认中文，可切换日文；随程序提供中文字体，支持字体与对白样式设置 |
| **特别模式** — CG、音乐、语音、电子小说与可交互的剧情时间线 | **独立存档** — 存档和玩家设置保存在外部目录，更新程序后可以继续使用 |

> 当前核心流程可玩，仍在完善转场精度、高清素材首次加载、音频边缘行为与长时间运行稳定性。后续工作见 [移植收尾清单](TODO.md)。

## 快速开始

在 Linux 桌面环境中打开终端：

~~~bash
git clone --depth 1 https://github.com/MuRongxin/wa2-linux_native.git wa2-native
cd wa2-native
./run.sh
~~~

**第一次运行会自动下载程序与资源，完成后进入标题画面。** 仓库只保存源码和启动工具；`run.sh` 从独立的程序 Release 获取 AppImage，按仓库中的固定 SHA-256 校验后使用包内环境，正常情况下无需安装系统 Python 或 Rust。

环境和数据准备好后，日常启动无需联网。

| AppImage | 首次资源下载 | 解压后的资源 | 首次安装建议空闲空间 |
| :---: | :---: | :---: | :---: |
| 约 **76.14 MiB** | 约 **45.63 GB** | 约 **50.79 GB** | 约 **100 GB** |

安装期间需要同时存放 ZIP 和解压后的文件；安装成功后会自动清理下载的 ZIP。以上资源体积对应 [`data-v1.1.0`](https://github.com/MuRongxin/wa2-linux_native/releases/tag/data-v1.1.0)。

### 启动时会发生什么

1. **下载程序**：按固定版本下载 AppImage，显示进度、速度与剩余时间；支持多代理和断点续传。只有 SHA-256 正确的完整文件才会替换本地程序。
2. **准备资源**：缺少可用数据时，自动下载 Release 分卷；终端实时显示进度、速度和预计剩余时间，支持中断续传与下载通道切换。
3. **校验并解压**：逐卷检查 SHA-256，显示解压进度，全部完成并通过文件统计检查后才启动游戏。
4. **进入游戏**：程序首次解包后按 SHA-256 缓存运行目录，后续启动复用程序、运行目录与资源，无需重复下载和解包。

AppImage 内置 Python、SDL2、FreeType、ffmpeg / ffprobe、dwebp、中文字体及 X11、Wayland、PulseAudio、ALSA 后端运行库。构建基线为 **Ubuntu 20.04 x86_64 / glibc 2.31**；窗口系统、音频服务和显卡驱动由桌面系统提供。`run.sh` 使用缓存的解包运行目录，**无需配置 FUSE**。

程序不兼容或无法运行时，脚本会自动安装必要的本机环境，再从源码编译运行。系统包使用现有的 `apt-get`、`dnf`、`pacman` 或 `zypper`，过程中可能需要输入管理员密码；Rust 安装到当前用户。具体包能否安装取决于发行版现有软件源。

首次下载仅需 `curl` 或 `wget`、`coreutils` 和 `flock`；缺失时只安装这些下载工具。网络错误、校验失败和空间不足会保留错误与断点，不会触发 Rust 或游戏运行库安装。

需要自己管理环境时，可关闭自动安装：

~~~bash
WA2_AUTO_INSTALL=0 ./run.sh
~~~

### 常用启动命令

| 命令 | 用途 |
| :--- | :--- |
| `./run.sh` | 进入标题画面 |
| `./run.sh 1002` | 直接进入指定场景，方便调试 |
| `./run.sh --download-only` | 准备 AppImage 与资源，不启动游戏、不编译引擎 |
| `./run.sh --check` | 检查运行环境和在线 Release；不安装、不构建、不下载分卷、不启动游戏 |
| `./run.sh --help` | 查看启动帮助 |

## 游玩与操作

剧情中的常用操作：

| 操作 | 功能 |
| :--- | :--- |
| 鼠标左键 / `Space` / `Enter` / `→` | 显示完整对白，再次操作推进 |
| 鼠标滚轮向下 | 推进对白 |
| `↑` / 鼠标滚轮向上 | 回退对白 |
| `Page Up` | 开启或停止连续快退 |
| 数字键 `1`–`9` / 点击选项 | 选择剧情分支 |
| 鼠标右键 | 隐藏或显示对话框 |
| `F11` | 切换窗口与全屏 |
| `Esc` / `Q` | 退出当前游玩 |

保存、读档、自动播放、跳过、已读文本和系统设置可通过底部指令条访问。特别模式中的内容从开始即可进入。

**时间线**支持滚轮缩放、拖动平移、单击选中节点，再次点击或按回车进入对应场景；右键或 `Esc` 返回。

## 画质与设置

默认按显示模式选择素材与解码倍率：**窗口化 4x，全屏或最大化 8x**。首次读取高清图片可能有短暂停顿，命中缓存后会更快。

~~~bash
WA2_ART_SCALE=2 ./run.sh           # 使用 2x 代理树，优先加载速度
WA2_ART_SCALE=8 ./run.sh           # 始终从 8x 母版读取素材
WA2_RENDER_SCALE=auto ./run.sh     # 按窗口尺寸解码，减少内存占用
WA2_LANG=jp ./run.sh               # 切换到日文
WA2_DATA="$HOME/Games/wa2-data" ./run.sh  # 自定义数据目录
~~~

中文字体随程序提供，包括 Noto Sans CJK SC 和得意黑。可在系统设置的文本设置中分别选择**对话文本、已读文本和 UI 字体**，并调整对白字号、描边、颜色、字重、字距与行距。

<details>
<summary><strong>更多环境变量</strong></summary>

| 变量 | 用途 |
| :--- | :--- |
| `WA2_DATA` | 数据目录，默认项目下的 `data/`；自动下载也写入此目录 |
| `WA2_LANG` | `cn` / `jp`；存在中文剧本时默认中文 |
| `WA2_ART_SCALE` | 选择素材树：`2` / `4` / `8`；未指定时随解码倍率选择 |
| `WA2_RENDER_SCALE` | 实际解码倍率：`auto` 或 `1` / `2` / `4` / `8` |
| `WA2_FONT` | 高级字体覆盖，支持 `字体路径#face` |
| `WA2_FONT_HOME` / `WA2_FONT_HISTORY` / `WA2_FONT_GAME` | 分别覆盖 UI、已读文本与对白字体 |
| `WA2_ART_CACHE_MB` | 素材内存缓存预算，单位 MB |
| `WA2_DATA_PROXIES` | 下载代理列表；空值关闭代理 |
| `WA2_APPIMAGE_PROXIES` | 单独指定程序下载代理；默认沿用资源代理，空值仅直连 |
| `WA2_DATA_TAG` | 资源 Release 标签，默认 `data-v1.1.0` |
| `WA2_DATA_REPO` | 资源仓库，默认 `MuRongxin/wa2-linux_native` |
| `WA2_DATA_CACHE` | 下载缓存目录，默认 `.wa2-release-parts/` |
| `WA2_BUILD_FROM_SOURCE=1` | 使用本机构建的引擎 |
| `WA2_AUTO_INSTALL=0` | 禁止自动安装系统包和构建工具链 |

`WA2_ART_SCALE` 决定读取哪套素材，`WA2_RENDER_SCALE` 决定实际解码的像素倍率。当前 Release 不包含 1x 原始背景与立绘；使用更低解码倍率时会从可用高清素材降采样。

</details>

## 程序与资源下载

程序与素材使用独立的 Release：

| 内容 | 版本 | 本地位置 |
| :--- | :--- | :--- |
| 引擎与运行环境 | [`app-v0.1.0`](https://github.com/MuRongxin/wa2-linux_native/releases/tag/app-v0.1.0) | `bin/wa2-native-linux-x86_64.AppImage` |
| 程序运行目录 | 与 AppImage 的 SHA-256 对应 | `bin/runtime/<sha256>/` |
| 游戏素材 | [`data-v1.1.0`](https://github.com/MuRongxin/wa2-linux_native/releases/tag/data-v1.1.0) | `data/` |

程序版本、大小和 SHA-256 固定在 `launcher/release.conf`。脚本不会把最新素材 Release 误当作程序，也不会每次启动自动升级；更新仓库中的程序版本后才获取对应新包，已有素材、存档与配置继续使用。

当前资源包包含 **2x / 4x / 8x 背景、立绘和中文界面**，以及运行所需的中日文剧本、语音、音乐、音效、影片和位图字体。省去了 1x 原始背景与立绘，保留必要的调色曲线。

默认依次尝试以下通道：

`gh-proxy.com` → `ghfast.top` → `ghproxy.net` → GitHub 直连

下载失败时会切换通道，并尽量保留断点；素材下载和 curl 程序下载也会在持续低速时切换。中断后重新运行 `./run.sh` 即可继续。程序临时文件保存在 `bin/.downloads/`，素材分卷保存在 `.wa2-release-parts/`；这些目录均不进入 Git。

~~~bash
# 仅使用 GitHub 直连
WA2_DATA_PROXIES= ./run.sh

# 自定义代理顺序，最后仍会尝试直连
WA2_DATA_PROXIES='https://ghfast.top/,https://ghproxy.net/' ./run.sh
~~~

<details>
<summary><strong>手动下载、已有数据与资源发布</strong></summary>

**手动安装**

从 [资源 Release](https://github.com/MuRongxin/wa2-linux_native/releases/tag/data-v1.1.0) 下载全部 **29 个 ZIP 分卷**和 `SHA256SUMS`。每个 ZIP 都是独立压缩包，必须全部解压到同一项目目录；完成后应得到项目下的 `./data/`，不要再套一层 `data/data/`。

在保存这些附件的目录中，可先校验，再解压到项目：

~~~bash
sha256sum -c SHA256SUMS
for part in data-v1.1.0.part*.zip; do
  unzip -oq "$part" -d /path/to/wa2-native
done
~~~

**已有数据**

把完整的 `data/` 放到项目根目录，或通过 `WA2_DATA` 指向它。正常启动只检查关键文件和目录，不会每次重新计算全部素材的哈希。自动安装过程中产生的未完成标记会阻止半成品被当作完整数据使用。

**发布新资源**

[资源打包工具](https://github.com/MuRongxin/wa2-linux_native/blob/951e0db12a7cd99d78f9248a0f3f9165dfbf13eb/tools/package_data.py) 生成的 `manifest.json` 和其中列出的全部 ZIP 必须放在同一个 Release；建议同时提供 `SHA256SUMS`。标签与分卷文件名前缀是独立字段，下载器按标签找到 Release，再按清单下载附件。

~~~bash
# 只检查清单和附件，不下载大文件
python3 launcher/fetch_data.py --tag data-v1.1.0 --dry-run
~~~

兼容旧的单代理变量 `WA2_DATA_PROXY`；同时设置时以 `WA2_DATA_PROXIES` 为准。访问私有素材 Release 可设置 `GITHUB_TOKEN`，此时使用 GitHub 认证直连，跳过代理。程序引导下载使用公开 Release，不会向代理发送令牌。

</details>

## 常见问题

<details>
<summary><strong>我需要先安装 Rust、Python、ffmpeg 或中文字体吗？</strong></summary>

通常无需手动安装。普通克隆不包含 AppImage，首次运行由脚本下载，并使用其中的 Python、媒体工具、运行库和字体。只有本机编译兜底才需要 Rust；缺少下载工具时只安装轻量下载依赖。`./run.sh --check` 可以先查看情况，且不会安装软件。

</details>

<details>
<summary><strong>为什么程序包很小，首次下载却有几十 GB？</strong></summary>

AppImage 只包含引擎与运行组件，不随 Git 克隆下载。游戏素材单独保存在 Release 中，包含 2x、4x、8x 多套画面、语音和影片；首次安装建议预留约 100 GB 空间。切换到 2x 运行只改变读取的素材树，当前安装器仍会下载整套资源。

</details>

<details>
<summary><strong>下载失败、校验失败或空间不足怎么办？</strong></summary>

根据终端提示处理后重新运行 `./run.sh`。正常中断会保留下载断点，已完成并通过校验的分卷可复用；无需重新克隆仓库。若所有分卷已缓存，网络故障时仍可继续校验与解压。

</details>

<details>
<summary><strong>存档和设置在哪里？更新程序会覆盖吗？</strong></summary>

从仓库启动时，存档在 `data/save/`，配置在 `data/wa2.ini`，缓存在 `data/cache/`；设置了 `WA2_DATA` 时位于对应目录。资源安装不会覆盖存档和玩家配置。引擎使用自己的存档格式，不兼容原版 EXE 的二进制存档。

单独运行 AppImage 时，默认在其旁边使用 `data/`；程序所在位置不可写时回退到用户数据目录。AppImage 本身不存放玩家数据。

</details>

<details>
<summary><strong>支持 Windows、macOS 或其他 CPU 架构吗？</strong></summary>

当前 AppImage 的打包目标为 Linux x86_64。Windows、macOS 和其他架构尚未完成支持验证；不能直接使用此 AppImage。

</details>

## 开发与打包

引擎使用 Rust 标准库与手写 FFI，`Cargo.toml` 的 `[dependencies]` 保持为空。SDL2 与 FreeType 通过动态加载使用，音视频与图片解码由外部工具提供。

~~~text
wa2-native/
├── run.sh              启动、资源准备与本机环境回退
├── bin/                下载的 AppImage、解包缓存与本地打包产物（不入库）
├── src/                剧本、渲染、音视频和界面实现
├── fonts/              内置中文字体与许可证
├── launcher/           启动检查、程序与资源下载支持文件
└── data/               外部游戏资源、存档、设置和缓存
~~~

<details>
<summary><strong>从源码运行</strong></summary>

需要 Python 3.8+、Rust/Cargo 1.90+、C 链接工具、SDL2、FreeType、ffmpeg / ffprobe；推荐安装 `dwebp` 加快 WebP 解码。缺少环境时，`run.sh` 默认尝试自动安装。

~~~bash
WA2_BUILD_FROM_SOURCE=1 ./run.sh
~~~

手动构建与运行：

~~~bash
cargo build --release
WA2_BUILD_FROM_SOURCE=1 ./run.sh
~~~

开发检查命令：

~~~bash
cargo test --release
~~~

</details>

<details>
<summary><strong>AppImage 打包与发布</strong></summary>

启动所需文件位于 `launcher/`；本地辅助目录 `tools/` 已忽略，不随仓库提交。完整的打包器、构建配置和测试保留在[打包工具历史版本](https://github.com/MuRongxin/wa2-linux_native/tree/951e0db12a7cd99d78f9248a0f3f9165dfbf13eb/tools)。需要重新打包时，可从该版本取回 `build_appimage.py` 与 `appimage/` 中的构建文件。

构建机需要 Python 3.12+、Rust、`bwrap`、`mksquashfs`、`patchelf`，以及可用的 Linux 用户命名空间。构建基线为隔离的 Ubuntu 20.04，不修改宿主系统软件包。

发布时：

1. 使用独立的程序标签，如 `app-v0.1.0`，上传 AppImage、校验文件与构建信息。
2. 将生成的 `release.conf` 同步到 `launcher/release.conf`，随源码更新。
3. 保持 `data-v1.1.0` 素材 Release 独立，无需重新打包素材。

`bin/`、`bin_/` 和任意目录下的 `*.AppImage` 均已加入 Git 忽略规则。AppImage 不包含游戏素材；运行组件许可证、FFmpeg 对应源码和构建配方随包提供。

</details>

---

本项目为非官方引擎重写。《WHITE ALBUM 2》的名称与游戏素材权利归原权利方所有。随程序提供的字体许可证见 [fonts/LICENSES](fonts/LICENSES)，其他运行组件保留各自的许可证与声明。
