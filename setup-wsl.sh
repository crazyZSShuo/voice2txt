#!/usr/bin/env bash
# =============================================================================
# setup-wsl.sh — VoiceInput WSL Ubuntu 完整环境搭建 + 编译脚本
#
# 使用方法：
#   chmod +x setup-wsl.sh
#   ./setup-wsl.sh          # 完整安装环境并编译
#   ./setup-wsl.sh --build  # 仅编译（环境已装好时）
#   ./setup-wsl.sh --clean  # 清理编译产物
# =============================================================================

set -euo pipefail

# ── 颜色输出 ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

step()    { echo -e "\n${CYAN}${BOLD}▶ $1${NC}"; }
ok()      { echo -e "  ${GREEN}✓ $1${NC}"; }
warn()    { echo -e "  ${YELLOW}⚠ $1${NC}"; }
die()     { echo -e "\n${RED}✗ 错误: $1${NC}\n"; exit 1; }

# ── 目标架构 ──────────────────────────────────────────────────────────────────
WINDOWS_TARGET="x86_64-pc-windows-msvc"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR"

# =============================================================================
# 0. 解析参数
# =============================================================================
MODE="setup"
[[ "${1:-}" == "--build" ]] && MODE="build"
[[ "${1:-}" == "--clean" ]] && MODE="clean"

# =============================================================================
# CLEAN 模式
# =============================================================================
if [[ "$MODE" == "clean" ]]; then
    step "清理编译产物"
    rm -rf "$PROJECT_DIR/dist"
    rm -rf "$PROJECT_DIR/src-tauri/target"
    rm -rf "$PROJECT_DIR/node_modules/.cache"
    ok "清理完成"
    exit 0
fi

# =============================================================================
# 1. 系统依赖
# =============================================================================
step "1/8  安装系统依赖"

sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends \
    curl wget git unzip \
    file \
    build-essential pkg-config \
    clang lld llvm \
    libssl-dev \
    libwebkit2gtk-4.1-dev \
    libxdo-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    mingw-w64 \
    nsis \
    python3 python3-pip

ok "系统依赖安装完成"

# Tauri CLI 在 Linux 主机上即使交叉编译 Windows，也会探测 Linux 托盘依赖。
if ! pkg-config --exists ayatana-appindicator3-0.1 && ! pkg-config --exists appindicator3-0.1; then
    die "未检测到 appindicator 开发库（pkg-config 条目缺失）。请确认 libayatana-appindicator3-dev 已成功安装。"
fi

if ! command -v makensis &>/dev/null; then
    die "未检测到 NSIS（makensis）。在 WSL/Linux 上交叉打包 Windows 安装器需要先安装 nsis。"
fi

# =============================================================================
# 2. Rust 工具链
# =============================================================================
step "2/8  安装 Rust 工具链"

if ! command -v rustc &>/dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # 加载 cargo 环境到当前 shell
    source "$HOME/.cargo/env"
    ok "Rust 已安装: $(rustc --version)"
else
    source "$HOME/.cargo/env" 2>/dev/null || true
    ok "Rust 已存在: $(rustc --version)"
    rustup update stable --no-self-update 2>/dev/null || true
fi

# =============================================================================
# 3. 添加 Windows 编译目标
# =============================================================================
step "3/8  添加 Windows 交叉编译目标"

rustup target add "$WINDOWS_TARGET"
ok "目标已添加: $WINDOWS_TARGET"

# =============================================================================
# 4. 安装 cargo-xwin（下载 MSVC SDK，无需 Windows）
# =============================================================================
step "4/8  安装 cargo-xwin（MSVC SDK 跨平台编译器）"

if ! command -v cargo-xwin &>/dev/null; then
    cargo install cargo-xwin --locked
    ok "cargo-xwin 安装完成"
else
    ok "cargo-xwin 已存在: $(cargo xwin --version 2>/dev/null || echo 'ok')"
fi

# cargo-xwin 首次运行时会自动下载 MSVC SDK 到 ~/.xwin-cache/
# 如需手动预下载：
#   cargo xwin --version  (触发下载)

# =============================================================================
# 5. 安装 Tauri CLI
# =============================================================================
step "5/8  安装 Tauri CLI v2"

if ! command -v cargo-tauri &>/dev/null; then
    cargo install tauri-cli --version "^2.1" --locked
    ok "Tauri CLI 安装完成"
else
    ok "Tauri CLI 已存在: $(cargo tauri --version 2>/dev/null || echo 'ok')"
fi

# =============================================================================
# 6. Node.js 依赖
# =============================================================================
step "6/8  安装 Node.js 依赖"

if ! command -v node &>/dev/null; then
    # 安装 nvm + Node 20 LTS
    curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
    export NVM_DIR="$HOME/.nvm"
    source "$NVM_DIR/nvm.sh"
    nvm install 20
    nvm use 20
    ok "Node.js $(node --version) 已安装"
else
    ok "Node.js 已存在: $(node --version)"
fi

# 安装前端依赖
cd "$PROJECT_DIR"
npm install --silent
ok "npm 依赖安装完成"

# =============================================================================
# 7. 生成占位图标
# =============================================================================
step "7/8  生成应用图标"

if [[ ! -f "$PROJECT_DIR/src-tauri/icons/icon.ico" ]]; then
    python3 "$PROJECT_DIR/scripts/gen_icons.py"
    ok "占位图标已生成（正式发布前替换为真实图标）"
else
    ok "图标文件已存在，跳过"
fi

# =============================================================================
# 8. 配置 Cargo 跨编译环境
# =============================================================================
step "8/8  写入 Cargo 交叉编译配置"

mkdir -p "$PROJECT_DIR/.cargo"
cat > "$PROJECT_DIR/.cargo/config.toml" << 'CARGO_CONFIG'
# 为 Windows MSVC 目标使用 xwin 提供的 MSVC SDK
[target.x86_64-pc-windows-msvc]
# cargo-xwin 会在编译时自动注入正确的 linker 和 lib 路径
# 无需手动指定 linker

[env]
# 告知 windows-rs 使用 xwin 的 SDK
XWIN_ARCH = "x86_64"
CARGO_CONFIG
ok ".cargo/config.toml 已写入"

# =============================================================================
# 编译（仅在 --build 或完整 setup 时执行）
# =============================================================================
step "开始编译 Windows 可执行文件"
echo ""
echo -e "  ${YELLOW}注意：首次编译会下载 MSVC SDK (~3GB) 并编译所有依赖，耗时较长${NC}"
echo -e "  ${YELLOW}后续增量编译通常只需 1-2 分钟${NC}"
echo ""

cd "$PROJECT_DIR"

# 先构建前端
echo -e "  ${CYAN}→ 构建 React 前端...${NC}"
npm run build

# 使用 cargo xwin 构建 Rust 后端（带 Windows MSVC SDK）
echo -e "  ${CYAN}→ 交叉编译 Rust 后端 (target: $WINDOWS_TARGET)...${NC}"
echo -e "  ${YELLOW}  首次运行会自动下载 MSVC SDK，请耐心等待...${NC}"

# Tauri 官方文档说明：Linux/macOS 交叉编译 Windows 时仅支持 NSIS；
# MSI 只能在 Windows 主机上创建。
# 这里不能用 `--bundles nsis`，因为该 CLI 参数按“宿主平台 bundle 类型”解析，
# 在 Linux 上只接受 deb/rpm/appimage。改用 --config 覆盖 bundle.targets。
TAURI_BUILD_ARGS=(
    --runner cargo-xwin
    --target "$WINDOWS_TARGET"
    --config '{"bundle":{"targets":["nsis"]}}'
)

# 通过 Tauri CLI 显式指定 cargo-xwin 作为 runner。
# 仅设置 CARGO="cargo xwin" 并不会让 tauri CLI 切换到底层构建器，
# 会导致它继续调用默认 cargo，最终在 WSL 中报 `link.exe not found`。
if ! cargo tauri build "${TAURI_BUILD_ARGS[@]}" 2>&1; then
    echo ""
    warn "Tauri 构建失败。"
    warn "如果错误里包含 \`link.exe not found\`，通常表示没有通过 --runner cargo-xwin 调用构建器，或 cargo-xwin/xwin SDK 未正确安装。"
    warn "如果错误里包含 appindicator / tray-icon，通常表示当前 WSL 缺少 Tauri Linux 侧依赖。"
    warn "可单独执行以下命令复查："
    echo -e "    ${BOLD}cargo tauri build --runner cargo-xwin --target $WINDOWS_TARGET --config '{\"bundle\":{\"targets\":[\"nsis\"]}}'${NC}"
    exit 1
fi

# =============================================================================
# 输出结果
# =============================================================================
echo ""
step "编译结果"

BUNDLE_DIR="$PROJECT_DIR/src-tauri/target/$WINDOWS_TARGET/release/bundle"

if [[ -d "$BUNDLE_DIR" ]]; then
    echo ""
    echo -e "  ${GREEN}${BOLD}🎉 编译成功！${NC}"
    echo ""

    # MSI 安装包
    MSI_FILES=$(find "$BUNDLE_DIR/msi" -name "*.msi" 2>/dev/null || true)
    if [[ -n "$MSI_FILES" ]]; then
        while IFS= read -r f; do
            SIZE=$(du -sh "$f" | cut -f1)
            echo -e "  ${GREEN}MSI  : $f ($SIZE)${NC}"
        done <<< "$MSI_FILES"
    fi

    # NSIS 安装程序
    NSIS_FILES=$(find "$BUNDLE_DIR/nsis" -name "*.exe" 2>/dev/null || true)
    if [[ -n "$NSIS_FILES" ]]; then
        while IFS= read -r f; do
            SIZE=$(du -sh "$f" | cut -f1)
            echo -e "  ${GREEN}NSIS : $f ($SIZE)${NC}"
        done <<< "$NSIS_FILES"
    fi

    # 裸可执行文件
    EXE="$PROJECT_DIR/src-tauri/target/$WINDOWS_TARGET/release/voice-input.exe"
    if [[ -f "$EXE" ]]; then
        SIZE=$(du -sh "$EXE" | cut -f1)
        echo -e "  ${CYAN}EXE  : $EXE ($SIZE)${NC}"
    fi

    echo ""
    echo -e "  ${YELLOW}WSL 中的文件可通过 Windows 资源管理器访问：${NC}"
    echo -e "  ${YELLOW}\\\\wsl\$\\Ubuntu\\$(echo $BUNDLE_DIR | sed 's|^/||' | sed 's|/|\\|g')${NC}"
else
    warn "bundle 目录未找到，可能仅生成了 .exe（安装包需在 Windows 上构建）"
    EXE="$PROJECT_DIR/src-tauri/target/$WINDOWS_TARGET/release/voice-input.exe"
    if [[ -f "$EXE" ]]; then
        SIZE=$(du -sh "$EXE" | cut -f1)
        echo -e "  ${GREEN}EXE  : $EXE ($SIZE)${NC}"
    fi
fi

echo ""
echo -e "  ${CYAN}下次编译只需运行：${NC}"
echo -e "  ${BOLD}  ./setup-wsl.sh --build${NC}"
echo ""
