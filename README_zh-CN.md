<p align="center">
  <img src="logo.svg" alt="CLI Music Player Logo" width="256">
</p>

<h1 align="center">CLI 音乐播放器</h1>

<p align="center">
  <a href="README.md">English</a>
  &nbsp;&nbsp;&nbsp;|&nbsp;&nbsp;&nbsp;
  <a href="README_zh-CN.md">简体中文</a>
</p>

<p align="center" style="color:gray;">
  基于 Rust 的 Linux 终端（TUI）音乐播放器，带频谱可视化。
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-orange" alt="Rust">
  <img src="https://img.shields.io/badge/Platform-Linux-informational" alt="Platform">
  <img src="https://img.shields.io/badge/License-AGPL--3.0-blue" alt="License">
  <img src="https://img.shields.io/github/stars/professor-lee/cli-music-player?style=flat&label=Stars&color=FFC700&logo=github&logoColor=white" alt="Stars">
</p>

<h2 align="center">项目概述</h2>

这是一个运行在 Linux 终端中的 TUI 音乐播放器。
支持本地播放与系统播放监控（MPRIS），并提供频谱可视化。

<p align="center">
  <img src="overview.png" alt="CLI 音乐播放器总览">
</p>

<h2 align="center">已有功能</h2>

- 本地音频播放
- 本地音频播放模式更改（列表循环/单曲循环/顺序播放/随机播放）
- 本地音频均衡器支持
- 系统播放监控（MPRIS）
- 播放列表侧边栏
- 专辑封面渲染：默认 ASCII 字符封面；如终端支持可启用 Kitty 图片封面
- Settings 弹窗（主题、透明背景、专辑边框、可视化模式、Bar 设置、Kitty 开关、封面质量、歌词/封面获取与下载、音频指纹识别、AcoustID API Key）
- 歌词显示
- 歌词获取：优先读取内嵌或本地 LRC（含同名 .lrc 与 lrc/ 目录），无则异步调用 LRCLIB
- 封面获取：优先读取内嵌或本地封面（含 cover/ 目录），无则异步使用 MusicBrainz + Cover Art Archive
- 无元数据时可选用 Chromaprint 生成指纹，通过 AcoustID 补全信息
- 可视化：频谱 Bars / 示波器（Oscilloscope，Braille 点阵叠加左右声道；优先使用 `cava` 数值，不可用时回退内部 FFT）

<h2 align="center">技术栈</h2>

- Rust 2021
- TUI：ratatui + crossterm
- 播放：rodio（本地）、MPRIS（系统）
- 可视化：`cava`（外部 bars）或内部 FFT 回退

<h2 align="center">开发与运行</h2>

### 终端字体（⚠️需要Nerd Font）

本项目 UI 控制区使用 Nerd Font 图标字符。如果你的终端字体不包含 Nerd Font 补丁字形，相关位置可能显示为方块/乱码。

建议使用任一 Nerd Font（例如：JetBrainsMono Nerd Font、FiraCode Nerd Font、Hack Nerd Font），并在终端/模拟器里将字体设置为该字体。

图标对照：

- 播放：
- 暂停：
- 上一首：
- 下一首：
- 随机播放：
- 单曲循环：
- 列表循环：
- 顺序播放：
> Oops,GitHub好像不支持显示nerdfont,可以去https://www.nerdfonts.com/cheat-sheet 查看图标。

### 依赖（Linux）

按发行版安装构建依赖（包名可能略有差异）：

```bash
sudo apt update
sudo apt install -y pkg-config libasound2-dev libdbus-1-dev libchromaprint-dev
```

### 运行

```bash
cargo run
```

### Release 构建

```bash
cargo build --release
./target/release/cli-music-player
```

首次运行时，程序会在系统配置目录下自动创建（若缺失）配置与主题文件：

- Linux：`$XDG_CONFIG_HOME/cli-music-player`（通常是 `~/.config/cli-music-player`）

也可以用环境变量 `CLI_MUSIC_PLAYER_ASSET_DIR` 覆盖根目录（该目录下仍使用 `config/` 与 `themes/` 子目录）。

<h2 align="center">频谱可视化（cava）</h2>

程序优先使用 `cava` 生成系统级频谱 bars（本项目仅负责渲染样式，`cava` 只输出数值 bars）。
如果 `cava` 不可用，会自动回退到内部 FFT 管线。

`cava` 可执行文件查找顺序：

1. 环境变量 `CLI_MUSIC_PLAYER_CAVA`（可为绝对/相对路径）
2. 与程序可执行文件同目录：`./cava`
3. 与程序可执行文件同目录：`./third_party/cava/cava`
4. `PATH` 中的 `cava`

如果以上都不可用，并且构建时启用了 `--features bundle-cava`，程序会把内置的 `cava` 解压到系统临时目录，仅在本次运行中使用，退出时自动删除。

可选安装 `cava`（推荐）：

```bash
# Debian/Ubuntu
sudo apt install -y cava

# Arch
sudo pacman -S cava
```

### 从源码自带 `cava`（可选）

如果你希望项目在构建时自动下载并编译 `cava`（而不是依赖系统已安装的 `cava`），可以使用：

```bash
cargo build --release --features bundle-cava
```

该模式会在 Cargo 构建过程中下载并编译上游 `cava`，并将其内置到程序中；运行时如果系统未安装 `cava`，会临时解压到系统临时目录使用，退出后自动删除。

注意：

- 构建需要联网。
- 为了兼容部分发行版的 autotools/宏版本差异，构建脚本会通过 `ACLOCAL_PATH` 注入一个最小 `AX_CHECK_GL` 覆盖宏，避免 `autogen.sh` 因 `_AX_CHECK_GL_MANUAL_LIBS_GENERIC: argument must not be empty` 失败；这只影响可选的 SDL/OpenGL 输出路径，不影响本项目所需的 raw bars 输出。
- 需要安装 `cava` 的构建依赖（Ubuntu/Debian 示例）：

```bash
sudo apt update
sudo apt install -y \
  build-essential autoconf automake libtool pkgconf \
  libfftw3-dev libiniparser-dev \
  libasound2-dev libpulse-dev libpipewire-0.3-dev
```

你也可以覆盖 build script 使用的版本/URL：

```bash
# 覆盖 tag
CLI_MUSIC_PLAYER_CAVA_BUNDLE_VERSION=0.10.6 cargo build --release --features bundle-cava

# 覆盖 tar.gz URL
CLI_MUSIC_PLAYER_CAVA_BUNDLE_URL=https://github.com/karlstav/cava/archive/refs/tags/0.10.6.tar.gz \
  cargo build --release --features bundle-cava
```

如果你只想启用 feature 但跳过 bundling（例如在 CI/发行版打包时），可以：

```bash
CLI_MUSIC_PLAYER_CAVA_BUNDLE_SKIP=1 cargo build --release --features bundle-cava
```

### Windows（构建说明 / 功能受限）

本项目主要面向 Linux。Windows 原生环境可以尝试“仅构建/本地播放”，但以下能力不保证可用：

- 系统播放监控（MPRIS）
- 系统音量控制（`Up`/`Down`）
- `bundle-cava`（构建链依赖 autotools，且上游 `cava` 的捕获后端在 Windows 上不一定可用）

推荐在 Windows 使用 WSL2，按 Linux 方式构建与运行。

原生 Windows（MSVC）构建参考：

1. 安装 Rust（`rustup`）
2. 安装 Visual Studio Build Tools（C++ 工具链）
3. 在仓库根目录执行：

```bash
cargo build --release
```

<h2 align="center">配置</h2>

- `config/default.toml`：UI/频谱/MPRIS + 均衡器（EQ）等配置
- `themes/*.toml`：主题定义

与 Kitty 封面相关的配置项（位于 `config/default.toml`）：

- `kitty_graphics`：启用 Kitty 图形协议渲染（默认：`false`）
- `kitty_cover_scale_percent`：封面质量百分比（默认：`50`；`100` 表示不下采样）

Bars 相关配置项（位于 `config/default.toml`，仅 Bars 模式生效）：

- `super_smooth_bar`：更细的高度分级字符（默认：`false`）
- `bars_gap`：柱状间隔（默认：`false`）

歌词/封面与指纹相关配置项（位于 `config/default.toml`）：

- `lyrics_cover_fetch`：启用歌词/封面异步获取（默认：`false`）
- `lyrics_cover_download`：将获取到的歌词/封面保存到本地（默认：`false`）
- `audio_fingerprint`：启用音频指纹识别（默认：`false`，需先设置 AcoustID API Key）
- `acoustid_api_key`：AcoustID API Key（在 Settings 弹窗内填写）

歌词与封面保存位置（启用下载时）：

- 歌词：与音频同目录的 lrc/ 文件夹，文件名与歌曲名相同（.lrc）
- 封面：与音频同目录的 cover/ 文件夹，文件名与歌曲名相同（.jpg/.png）

默认位置（Linux）：

- `~/.config/cli-music-player/config/default.toml`
- `~/.config/cli-music-player/themes/*.toml`

<h2 align="center">快捷键</h2>

随时按 `Ctrl+K` 打开应用内快捷键提示。

| 按键 | 功能 |
|---|---|
| `Ctrl+F` | 打开文件夹输入 |
| `P` | 打开/关闭播放列表 |
| `Space` | 播放/暂停 |
| `Left` / `Right` | 上一首 / 下一首 |
| `Up` / `Down` | 音量加 / 减 |
| `E` | 打开均衡器（仅本地） |
| `Alt+R` | 重置均衡器为默认值（在 EQ 弹窗内） |
| `M` | 切换重复模式（仅本地） |
| `T` | 打开 Settings |
| `Ctrl+K` | 打开 Keys（帮助） |
| `Enter` | 确认（文件夹输入 / 播放列表） |
| `Q` | 退出 |
| `Esc` | 关闭弹窗/侧边栏 |

播放列表打开时：

| 按键 | 功能 |
|---|---|
| `Ctrl+Up` / `Ctrl+Down` | 将选中项上移 / 下移 |
| `Ctrl+Left` / `Ctrl+Right` | 上一个 / 下一个专辑（MultiAlbum） |

---

### 许可证

[AGPL-3.0 license](LICENSE)

第三方声明： [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
