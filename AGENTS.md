# AGENTS.md — 项目说明（给 AI 助手 / 协作者）

> 最后更新：2026-10-15 · 中文字体修改计划见第 8 节

---

## 1. 这是什么

`wa2-native` 是 **《WHITE ALBUM 2》（白色相簿2）的 Linux 原生引擎重写**。

它**不是** Wine 封装、不是 exe 外壳、也不是模拟器。整个引擎是用 Rust 从零写的，
直接读取原版游戏的素材文件（`.pak` 解包后的松散文件），重新实现剧本解释器、
渲染、音频、影片播放和全部界面。

**来源**：原版安装位于

```
~/Downloads/WHITE ALBUM/白色相簿2 整合版/
├── WA2.exe            日文原版主程序
├── WA2_chs.exe        CK-GAL 简体中文补丁（文字是 Shift-JIS 编码的伪日文）
├── ck-gal.pak         中文补丁资源
├── grp.pak char.pak fnt.pak fon.pak bak.pak BGM.PAK SE.PAK VOICE.PAK
├── mv*.pak            影片（不是压缩包，本身就是 ASF/WMV）
├── IC/                附加剧本 + 语音
└── grp/               部分松散素材
```

**本仓库是移植工程，不含任何游戏素材。**

---

## 2. 目的（按优先级）

1. **完全脱离原安装** —— 运行时不读 `~/Downloads` 的任何东西，也不需要 `.pak`。
   素材一次性解包成 `data/` 松散文件树，之后原版目录可以删掉。
2. **中文必须正常** —— CK-GAL 补丁的文本、字形、界面全部正确显示（硬性要求）。
   日文原版作为基线保留（`WA2_LANG=jp`）。
3. **保留原版界面** —— 标题画面、系统设置、存档/读档、底部指令条都用**游戏自己的素材**
   还原，不要自己画一套替代品。
4. **最终产出**：一个自己玩的 AppImage（**已明确推迟**，先把移植做完）。

---

## 3. 硬性约束

| 约束 | 说明 |
|---|---|
| **零 crate** | `Cargo.toml` 的 `[dependencies]` 必须保持为空。标准库 + 手写 FFI。 |
| **手写 SDL2 FFI** | `src/sdl2.rs` 是唯一 `extern "C"` 的地方，用 `dlopen`/`dlsym` + `transmute` 函数指针。 |
| **素材不入库** | `data/` 在 `.gitignore` 里。游戏素材有版权，**绝不能提交或分发**。 |
| **运行时依赖** | `libSDL2-2.0.so.0`、`ffmpeg`/`ffprobe`（子进程解码）、系统 CJK 字体（`NotoSansCJK-Regular.ttc`。菜单标题和引擎侧字符串用它光栅化；对白仍是位图字体，见第 8 节）。 |
| **默认中文** | 存在 `data/script_cn/` 时默认中文；`WA2_LANG=jp` 切日文。 |

---

## 4. 构建与运行

```bash
cargo build --release          # 要求 rustc/cargo 1.9x，2021 edition
./run.sh                       # 标题画面
./run.sh 1002                  # 直接跳到某场景（调试）
```

### CLI 子命令（`./target/release/wa2-native <cmd>`）

```
title run play play-noto        # 正常游玩 / 跳场景
lscr-trace steps-info           # 打印剧本字节码执行流（逆向主力工具）
scene scene-noto scene-map      # 渲染单页剧本文本到 TGA
kcap lac extract                # 档案解包
tga tga-dump                    # TGA 解码 / 转标准 TGA
fmtinfo font-test render-test text-info sjis
movie-frame movie-test audio-test verify-window
```

### 环境变量

| 变量 | 作用 |
|---|---|
| `WA2_DATA` | 数据根目录（默认 `./data`） |
| `WA2_LANG` | `cn`（默认，若有 `script_cn`）/ `jp` |
| `WA2_INI` | 配置文件路径（默认 `<data>/wa2.ini`） |
| `WA2_SPRITES` | 覆盖立绘目录 |
| `WA2_AUTO` / `WA2_AUTO_TEXT` | 自动推进（跑流程/截图用） |
| `WA2_TITLE_AUTO=2` | 标题画面自动选第 N 项（无头测试用） |
| `WA2_DEBUG_INPUT` | 打印输入事件（含窗口尺寸、页签切换） |
| `WA2_DEBUG_AUDIO` | 每秒打印正在出声的音频流（含有效增益） |
| `WA2_MENU_DUMP=<path>` | 界面**状态变化时**原子导出一帧 TGA（抓屏不可靠时用） |

---

## 5. 代码地图

```
src/
  main.rs        2296  主循环、场景驱动、事件分发、标题画面、CLI
  menu.rs         992  系统设置界面（三页，全部用游戏素材合成）
  saveload.rs     345  存档/读档界面
  save.rs         333  存档格式（存 id/名字，读档时重新解析素材）
  cmdbar.rs       217  底部指令条（保存/读档/自动/跳过…）
  vm/lscr.rs      728  LSCR 字节码解释器（舞台指令 ACT）
  vm/mod.rs       325  剧本 .txt 字符串池 + 页面解析
  render/mod.rs   418  图层合成、对话框、滤镜
  render/font.rs  316  位图字体（cell map 按 SJIS 码查表）
  render/text.rs   64  系统字体回退（中文引擎侧字符串）
  sdl2.rs         487  SDL2 FFI + 输入（含 Pointer / Drag / RightClick）
  audio/mixer.rs  325  软件混音（每流独立 ffmpeg 管道 + 分组增益）
  video/mod.rs     83  影片播放（ffmpeg 解 RGBA 帧）
  codec/          197  TGA / LZSS / SJIS（含 9374 行生成码表）
  archive/        289  KCAP / LAC 解包
  data.rs         124  数据树布局与语言切换
docs/
  LSCR舞台指令逆向报告.md    ★ 逆向结论汇总，动剧本/舞台逻辑前必读
tools/
  build_data_tree.py       从用户自己的游戏生成 data/
  gen_cell_map.py          从 exe 字体表生成 cell map TSV
  lscr_dump.py             字节码 dump
  tables/wa2_cell_map_{jp,cn}.tsv
  （另有 ~190 个一次性逆向脚本，多数已完成任务）
```

---

## 6. 数据树（`data/`，约 8.5 GB）

```
script/       379  日文剧本 .bnr + .txt（字符串池）
script_cn/    379  中文补丁剧本（同结构）
bg/          1069  背景 / 界面素材（b* 背景、v* CG、sys_* 系统界面、T* 标题）
bg_cn/         31  中文补丁覆盖的界面素材（设置页、指令条等，优先加载）
sprite/      1624  立绘（<前缀><系列:03><差分:03>.tga）
voice/      46005  语音（{场景}_{序号:04d}_{角色:02d}.OGG）
bgm/          108  BGM_%03d{_A,_B}.OGG
se/          1132  SE_%04d.WAV/.OGG
movie/         32  mv*.wmv（原 .pak 就是 ASF）
font/              位图字体 .tga + 两张 cell map TSV
save/              存档
wa2.ini            玩家设置（键名沿用原版 exe）
```

用 `python3 tools/build_data_tree.py` 从**用户自己的**游戏副本生成，仓库里不放。

---

## 7. 已逆向的关键知识（改动前务必了解）

* **立绘**：`K30 记录选脸（char_id, 差分, 系列×1000）` + `K16 记录上屏（slot, …）ACT154/155`。
  文件名 `<前缀><系列:03><差分:03>.tga`。**char_id → 前缀** 的权威表来自
  `WA2.exe` 偏移 **661252** 的 33 项前缀表，再用全剧本 11794 条立绘请求验证。
  搜索顺序必须**精确优先**（0,-1,+1,-2,+2）—— 早期从 -2 开始导致所有姿势塌缩到 `001101`。
* **背景**：`ACT146/147` 才是换背景，格式 `(模式, ID, 变体, 过渡帧, 0,0,0, 1280.0,720.0)`，
  文件 `b{ID:04}{变体:02}.tga`（缺失时 `b{ID:04}*` 兜底，191 个 ID 中 178 个命中）。
  `ID=0/-2` 黑屏。**`ACT176` 是分层叠图（道具/效果），不是背景**。
* **色调**：`ACT172` 只带 `sepia.AMP`/`nega.amp`/空（是滤镜不是背景）。
  `.AMP` = 调色曲线：256 字节=单曲线，768=RGB 三条，1280=五条取前三条。
* **音频**：`ACT158` BGM（曲号直接查 `BGM_%03d.OGG`）、`159` 停、`161` 音量、
  `164` 独立 SE、`165` 声道 SE、`166/167/168` 停止/调音量/等待。**`ACT165` 不是 BGM**。
* **语音**：`ACT137` 第二个 int = 语音场景号；`ACT138` 末尾 = 角色 ID + 语音序号；
  文件名 `{场景}_{序号:04d}_{角色:02d}.OGG`。
* **定时**：`ACT194(n)` = 等 n 帧 @60fps。
* **字体**：exe 字体表 → TSV，`cell = 139 + 表内序号`，**按 SJIS 码查表**（不是按字符，避免编码歧义）。对白仍走这条路。菜单文字正在改成矢量字体，规则见第 8 节。
* **剧本**：`.txt` 是逗号分隔字符串池，字节码只存下标；**字节码位置 = 生效时机**
  （舞台指令插在第 N 页 `ACT132` 与第 N+1 页 `ACT144` 之间）。
* **界面**：设置/存档界面素材像素对齐，`sys_0310X`=底图（银框）、`sys_0320X`=金框（当前值）、
  `sys_0321X`=青框（悬停）；**控件矩形是从图像里连通域检测出来的**，不需要 exe 坐标表。
  中文补丁自带中文页面素材，`bg_dirs()` 优先查 `bg_cn/` 即可。

---

## 8. 中文字体修改计划

目标：界面和以后的对白改用真正的字体文件绘制，全屏放大时字仍然清晰，玩家以后可以换字体。原版把界面短语和对话都做成图片，放大整幅 1280×720 画面时字会被拉糊。

### 已定的画法

逻辑坐标保持 1280×720。先把界面图缩放到当前窗口（整数倍就逐像素复制，否则双线性；铺满窗口，不留黑边）。**字在缩放之后画**，字号 = 布局像素 × 窗口高度 / 720，这样笔画落在窗口的真实像素上。窗口和全屏都要保留，都铺满屏幕。

不要把字先画进 1280×720 的表面再交给 SDL 拉伸。整幅画面改成最近邻放大会出锯齿，已经否掉。

中文模式（加载了 `bg_cn`）才替换贴图上的字。日文模式继续用原版图片和位图字体。

矢量字走 `render/text.rs`：一次 ffmpeg `drawtext`，字体 `NotoSansCJK-Regular.ttc`，白色、alpha 为覆盖率，贴上界面时再着色。不要加阴影或描边。

### 盖掉贴图上的旧字

矢量字叠在没擦干净的旧字上，会看成重影或阴影。盖字要连笔画光晕一起去掉，再画新字。

- 按钮里的旧标题用按钮底色铺平文字所在的中部，左右雪花和边框留下。
- 页签不要按亮度抠像素，渐变会被抠出波浪。用页签边缘一列干净颜色把标题带整行铺回去。
- 底栏旧字用底色铺掉。`sys_03020` 上半是向左的箭头，下半是向右的箭头。
- 标志、版权、金框/青框、虚线、分隔线仍是原图。设置页顶部 “Option / WHITE ALBUM 2” 横幅也保持原图。

短中文（2 到 6 个汉字、没有英文）字与字之间加一个空格，避免挤成一团。按钮里中文在上、英文在下，两行之间留空隙。720p 下的字号大约是：行标签 22、分组标题 18、按钮主标题 18、附注 14，再按窗口高度放大。

### 进度

| 范围 | 状态 |
|---|---|
| 标题菜单各项、特别模式菜单（含「时间线」） | 已改成矢量字 |
| 系统设置三页的标签、按钮、页签、底栏、默认按钮、向导 | 已改成矢量字 |
| 存档/读档、底部指令条上的短语 | 仍是贴图，还没改 |
| 对白 | 仍是位图字体，还没改 |
| 玩家自选字体文件 | 还没做。等矢量这条路稳定后再接 |

### 对白以后怎么换（现在不要做）

不需要 OCR。中文：SJIS 码 → `tools/tables/wa2_cell_map_cn.tsv` 的格子 → `tools/tables/wa2.cht.txt` 里 `cell - 748` 的字。格子号小于 748 的标点保持解码出的符号。日文模式不用这张对照。用户没有再次要求之前，不要改对白字体。

改这一节时核对一次就够：编译，外加一条断言或一张截图。像素已经说明旧字盖掉了，就不要为了截图描述再开一轮。

---

## 9. 当前状态

**可用**：脱离原安装运行 · 中文显示（字形覆盖率 100%）· 背景/立绘/slot 站位 ·
BGM/SE/语音（真的出声，音量分组可控）· 影片播放 · 标题画面 · 系统设置（三页全部可交互）·
存档/读档 · 底部指令条 · 鼠标（悬停/点击/拖动/右键呼出设置）。

**已知差距**（按重要性）：

- **点击没有音效反馈**；标题界面进入设置时没有 mixer，♪ 试听是哑的
- 没有自定义鼠标光标
- 文本显示速度 / 画面特效 / 三种透明度：只存进 `wa2.ini`，**没有接到渲染**
  （引擎还没有逐字打字机、也没有半透明对话框）
- 场景切换是瞬切，`ACT146/147` 的 `frames` 过渡时长还没做成淡入淡出
- `ACT154` vs `ACT155` 目前等价；立绘退场指令载体尚未找到
- `ACT170`（收窗/换景时隐藏对话框）未实现
- 性能：每行文本都整屏重新合成（释放版约 0.3~1 s/行），没有分层缓存

---

## 10. 给 AI 助手的工作约定

### 验证纪律（本项目最重要的经验）

1. **不要只看日志就宣称功能可用。** 必须截到画面 / 听到声音 / 读到持久化文件。
   典型例子：日志显示"背景已加载"，实际因为少了一次 `present()` 屏幕全黑。
2. **改完必须实机跑一遍相关场景**，用 `WA2_AUTO_TEXT=1 ./run.sh <场景号>`。
3. **批量改动后断言替换成功。** 用脚本改代码时，先 `assert old in s` 再替换 ——
   曾经有一次 `str.replace` 静默匹配失败，问题潜伏了好几步才发现。
4. **提交前跑 `cargo test`**（目前 4 个测试：剧本解析、前缀剥离、字体调色板 alpha、指令条渲染）。
5. 不要动 `data/`；不要 `git add data/`。

### 无头测试配方

```bash
Xvfb :99 -screen 0 1280x720x24 &          # 一次性
DISPLAY=:99 SDL_VIDEODRIVER=x11 ./target/release/wa2-native title
DISPLAY=:99 import -window root /tmp/shot.png     # 抓帧（:99 上可靠）
```

真机（XWayland + 多显示器）上 `import` 和 `ffmpeg -f x11grab` **可能抓不到内容或直接卡死**，
优先用程序自带的导出：

```bash
WA2_MENU_DUMP=/tmp/frame.tga ./run.sh title       # 界面状态变化时原子写一帧
./target/release/wa2-native tga-dump /tmp/frame.tga /tmp/frame.tga  # 转标准 TGA
```

**合成鼠标输入**（XTEST，脚本放在 `/tmp/mouse.py`）：

```python
import ctypes, sys, time
X11=ctypes.CDLL("libX11.so.6"); XT=ctypes.CDLL("libXtst.so.6")
X11.XOpenDisplay.restype=ctypes.c_void_p
d=ctypes.c_void_p(X11.XOpenDisplay(sys.argv[1].encode()))
x,y=int(sys.argv[2]),int(sys.argv[3]); act=sys.argv[4] if len(sys.argv)>4 else 'move'
BTN={'click':1,'rclick':3}.get(act)
XT.XTestFakeMotionEvent(d,0,x,y,0); X11.XFlush(d); time.sleep(0.15)
if BTN:
    XT.XTestFakeButtonEvent(d,BTN,1,0); X11.XFlush(d); time.sleep(0.06)
    XT.XTestFakeButtonEvent(d,BTN,0,0); X11.XFlush(d)
```

### 交互测试的坐标陷阱

真机上游戏窗口在窗口管理器边框**内部**，X11 报的绝对坐标是**边框**的：

```
边框  1330x807 @ (1375,995)      ← xwininfo -name 看到的
客户区 1280x720 @ (1400,1057)     ← SDL 实际窗口，游戏收到的是客户区坐标
偏移 = (25, 62)
```

点屏幕坐标 = 客户区坐标 + `(1400,1057)`。搞错这个会出现"点了没反应"的假象。

### 参考

- `docs/LSCR舞台指令逆向报告.md` —— 剧本指令、界面素材、音频指令的完整逆向结论，
  动这些地方之前先读它。
- 原版行为有疑问时，去 `WA2.exe` 里找表（立绘前缀表 @661252、配置键表 @666336、
  素材名表 @662912）比猜快得多。

---

## 11. 工作区状态（2026-10-13）

最后一次提交是 `b036b01`，之后 **8 个文件已改 + 6 项新增都还没提交**：

```
已修改  src/audio/mixer.rs  src/codec/tga.rs   src/main.rs  src/render/font.rs
        src/render/mod.rs   src/sdl2.rs        src/vm/lscr.rs
        tools/build_data_tree.py
未跟踪  AGENTS.md  docs/  src/cmdbar.rs  src/menu.rs  src/save.rs
        src/saveload.rs  tools/lscr_dump.py
```

未提交内容大致对应这几件事：字体抗锯齿与描边、音频分组增益与生命周期修复、
BGM/SE/语音指令修正、背景与色调系统、立绘 slot 站位、设置/存档/指令条三套界面。

**接手时建议**：先 `cargo build --release && cargo test`，再 `./run.sh` 实机确认，
然后按第 9 节的差距清单挑一项做。提交前注意 `data/` 与 `tools/samples/` 不进版本库。
