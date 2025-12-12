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
</p>

<h2 align="center">项目概述</h2>

这是一个运行在 Linux 终端中的 TUI 音乐播放器。
支持本地播放与系统播放监控（MPRIS），并提供频谱可视化。

<h2 align="center">已有功能</h2>

- 本地音频播放
- 系统播放监控（MPRIS）
- 播放列表侧边栏
- Settings 弹窗（主题切换 + 开关项）
- 频谱可视化（优先使用 `cava` bars；不可用时回退内部 FFT）

<h2 align="center">技术栈</h2>

- Rust 2021
- TUI：ratatui + crossterm
- 播放：rodio（本地）、MPRIS（系统）
- 可视化：`cava`（外部 bars）或内部 FFT 回退

<h2 align="center">开发与运行</h2>

### 依赖（Linux）

按发行版安装构建依赖（包名可能略有差异）：

```bash
sudo apt update
sudo apt install -y pkg-config libasound2-dev libdbus-1-dev
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

如果你把可执行文件移出仓库目录运行（或做成发行包），请确保程序能找到资源目录：`themes/` 与 `config/`。
你可以设置环境变量 `CLI_MUSIC_PLAYER_ASSET_DIR` 指向包含它们的目录（通常是项目根目录）。

<h2 align="center">频谱可视化（cava）</h2>

程序优先使用 `cava` 生成系统级频谱 bars（本项目仅负责渲染样式，`cava` 只输出数值 bars）。
如果 `cava` 不可用，会自动回退到内部 FFT 管线。

`cava` 可执行文件查找顺序：

1. 环境变量 `CLI_MUSIC_PLAYER_CAVA`（可为绝对/相对路径）
2. 与程序可执行文件同目录：`./cava`
3. 与程序可执行文件同目录：`./third_party/cava/cava`
4. `PATH` 中的 `cava`

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

该模式会在 Cargo 构建过程中下载并编译上游 `cava`，然后把生成的 `cava` 可执行文件复制到本项目产物旁边（例如 `target/release/cava`），运行时会优先使用它。

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

- `config/default.toml`：UI/频谱/MPRIS 等配置
- `themes/*.toml`：主题定义（Catppuccin）

<h2 align="center">快捷键</h2>

随时按 `Ctrl+K` 打开应用内快捷键提示。

| 按键 | 功能 |
|---|---|
| `Ctrl+F` | 打开文件夹输入 |
| `P` | 打开/关闭播放列表 |
| `Space` | 播放/暂停 |
| `Left` / `Right` | 上一首 / 下一首 |
| `Up` / `Down` | 音量加 / 减 |
| `M` | 切换重复模式 |
| `T` | 打开 Settings |
| `Ctrl+K` | 打开 Keys（帮助） |
| `Q` | 退出 |
| `Esc` | 关闭弹窗/侧边栏 |

---

### 许可证

[AGPL-3.0 license](LICENSE)

第三方声明： [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
