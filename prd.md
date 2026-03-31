# Windows 平台语音转文字开发需求（Tauri v2 · Rust + TypeScript）

请实现一个 **Windows 系统托盘语音输入法应用**，技术栈为 Tauri v2 + Rust + TypeScript（React 18 + Vite），优先支持 Windows 11，兼容 Windows 10 22H2+。

---

## 1. 按键触发与录音

按住`右 Alt 键`（VK_RMENU）录音，松开后将转录文字注入当前聚焦的输入框。优先使用流式转录模式逐字输出。

按键监听通过 Rust 端调用 `windows-rs` 的 `SetWindowsHookEx(WH_KEYBOARD_LL)` 实现全局低级键盘钩子，在钩子回调中通过返回非零值（`CallNextHookEx` 不传递）抑制右 Alt 的系统默认行为。钩子运行在独立线程中，通过 Tauri 的 `AppHandle.emit()` 将按键事件推送到前端及业务逻辑层。

```rust
// Cargo.toml 关键依赖
[dependencies]
tauri = { version = "2", features = ["tray-icon", "image-png"] }
windows = { version = "0.58", features = [
  "Win32_UI_Input_Ime",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_DataExchange",
  "Win32_Foundation",
] }
cpal = "0.15"
reqwest = { version = "0.12", features = ["json", "stream"] }
arboard = "3"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

---

## 2. 语音识别方案

采用分层策略，优先保证中文开箱可用：

**主方案（推荐）：HTTP STT 接口**，通过 Rust `reqwest` 异步调用 OpenAI 兼容的 `/v1/audio/transcriptions` 接口（可配置指向本地 Whisper 服务、OpenAI、或其他兼容服务）。录音结束后将 WAV 数据 multipart 上传，支持流式返回逐字追加到悬浮窗。默认语言参数设为 `zh`，确保中文开箱可用。

**备选方案：本地 Whisper**，通过 `whisper-rs` crate 绑定 `whisper.cpp`，完全离线运行，需在 Settings 中配置模型文件路径（推荐 `ggml-medium.bin` 或 `ggml-large-v3.bin`）。

音频采集统一使用 `cpal` crate，通过 WASAPI 后端捕获麦克风输入，采样率 16000Hz、单声道、f32 格式，每帧同时：①计算 RMS 值通过 `emit("rms-level", rms)` 推送前端驱动波形动画；②累积 PCM 数据用于最终识别。

---

## 3. 语言切换

在托盘菜单提供语言切换选项：英语（`en`）、简体中文（`zh`）、繁体中文（`zh-TW`）、日语（`ja`）、韩语（`ko`）。语言选择作为 STT 接口的 `language` 参数传递，同时存储到应用配置文件（`AppData\Roaming\VoiceInput\config.json`），下次启动自动恢复。默认为简体中文。

---

## 4. 录音状态悬浮窗

录音时在屏幕底部居中显示一个精致的无边框胶囊状悬浮窗，完全由 TypeScript + HTML/CSS 渲染，具体要求：

**窗口层**（Rust / Tauri 配置）：

```json
// tauri.conf.json 悬浮窗配置
{
  "label": "capsule",
  "decorations": false,
  "transparent": true,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "focus": false,
  "visible": false,
  "width": 220,
  "height": 56
}
```

通过 `window-vibrancy` crate 对悬浮窗应用 Acrylic 毛玻璃效果：

```rust
use window_vibrancy::apply_acrylic;
apply_acrylic(&capsule_window, Some((18, 18, 28, 180)))?;
```

通过 `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` 扩展样式确保窗口不抢焦点、不出现在任务栏。窗口圆角通过 `DwmSetWindowAttribute` + `DWMWCP_ROUND` 实现系统级圆角（Windows 11），Windows 10 回退到 CSS `border-radius`。

**UI 层**（TypeScript + CSS）：

```
胶囊尺寸：高度 56px，圆角 28px，最小宽度 200px，最大宽度 600px
背景：rgba(18, 18, 28, 0.82)，依赖 Acrylic 毛玻璃衬底
边框：0.5px solid rgba(255,255,255,0.12)
```

胶囊内部从左到右排列：

- **录音状态指示点**（8px 红色圆点，呼吸动画）
- **波形动画区域**（44×32px）：5 根竖条，必须由 Rust 端实时推送的 RMS 电平驱动。通过 `listen("rms-level", handler)` 接收每帧 RMS 值（0.0–1.0），各竖条权重为 `[0.5, 0.8, 1.0, 0.75, 0.55]`，平滑包络（attack 40%、release 15%），每帧叠加 ±4% 随机抖动。使用 `requestAnimationFrame` 驱动渲染，绝对不使用写死的假动画。
- **转录文字标签**（弹性宽度 160–520px）：实时追加转录文本，胶囊宽度随文字平滑弹性变宽，超出宽度上限后文字滚动显示。Rust 端每收到一段转录结果即通过 `emit("transcript-chunk", text)` 推送，前端追加显示。

**动画**：入场弹簧动画 0.35s（CSS `cubic-bezier(0.34, 1.56, 0.64, 1)`）；胶囊宽度变化平滑过渡 0.25s；退场缩放淡出 0.22s。窗口尺寸变化通过 `window.setSize()` Tauri API 同步调整，与 CSS 过渡保持一致。

---

## 5. 文字注入

文字注入使用**剪贴板 + 模拟 Ctrl+V 粘贴**方式，全部在 Rust 端完成：

```rust
use arboard::Clipboard;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::WindowsAndMessaging::*;

async fn inject_text(text: &str) -> Result<()> {
    let hwnd = unsafe { GetForegroundWindow() };

    // 1. 保存原剪贴板内容
    let mut clipboard = Clipboard::new()?;
    let original = clipboard.get_text().ok();

    // 2. 检测并临时关闭 IME
    let himc = unsafe { ImmGetContext(hwnd) };
    let ime_open = unsafe { ImmGetOpenStatus(himc) }.as_bool();
    if ime_open {
        unsafe { ImmSetOpenStatus(himc, false) };
    }

    // 3. 写入目标文本到剪贴板
    clipboard.set_text(text)?;
    tokio::time::sleep(Duration::from_millis(30)).await;

    // 4. 模拟 Ctrl+V
    send_key_combo(VK_CONTROL, VK_V);
    tokio::time::sleep(Duration::from_millis(80)).await;

    // 5. 恢复 IME 状态
    if ime_open {
        unsafe { ImmSetOpenStatus(himc, true) };
    }
    unsafe { ImmReleaseContext(hwnd, himc) };

    // 6. 恢复原剪贴板
    tokio::time::sleep(Duration::from_millis(100)).await;
    if let Some(orig) = original {
        clipboard.set_text(&orig)?;
    }
    Ok(())
}
```

---

## 6. LLM 转录纠错

通过 Rust `reqwest` 调用 OpenAI 兼容 API（`/v1/chat/completions`）对转录文本进行保守纠错，支持流式响应（`stream: true`），边纠错边更新悬浮窗文字。

System prompt 要求极度保守：只修复明显的语音识别错误，例如中文谐音错误、英文技术术语被误转为中文（「配森」→ `Python`、「杰森」→ `JSON`、「歌图恩」→ `GetToken`、「锐克特」→ `React`）；绝对不改写、润色或删除任何看起来正确的内容；输入如果已经正确则必须原样返回。

松开右 Alt 后若 LLM 已启用，悬浮窗状态变为 *Refining...*（文字变灰 + 轻微脉冲动画），LLM 流式返回时逐字更新为最终文本，完成后执行注入。

---

## 7. 系统托盘与设置界面

**托盘图标**（Tauri v2 内置 `tray-icon` feature）右键菜单包含：

- **LLM Refinement** 子菜单：启用/禁用开关（勾选状态）、Settings 入口
- **语言切换** 子菜单（五种语言，当前选中带勾）
- **开机自启** 开关（写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`）
- **Quit** 退出

**Settings 窗口**（独立 Tauri 窗口，TypeScript + React 渲染）包含：

- STT 配置区：API Base URL、API Key（`<input type="password">`，支持完全清空）、Model 名称
- LLM 配置区：独立的 API Base URL、API Key、Model
- **Test** 按钮：分别测试 STT 和 LLM 连通性，显示成功/失败状态
- **Save** 按钮：序列化为 JSON 写入 `AppData\Roaming\VoiceInput\config.json`，敏感字段（API Key）使用 Windows `CryptProtectData` 加密存储

---

## 8. 运行模式与构建

应用以**仅系统托盘模式**启动，主窗口不存在，`tauri.conf.json` 中不配置 `mainWindow`，启动时仅显示托盘图标。

**项目结构：**

```
voice-input/
├── src-tauri/          # Rust 后端
│   ├── src/
│   │   ├── main.rs
│   │   ├── hotkey.rs       # 全局键盘钩子
│   │   ├── audio.rs        # cpal 音频采集 + RMS
│   │   ├── stt.rs          # 语音识别接口
│   │   ├── llm.rs          # LLM 纠错
│   │   ├── inject.rs       # 文字注入 + IME 控制
│   │   └── config.rs       # 配置读写
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                # TypeScript + React 前端
│   ├── capsule/        # 悬浮窗 UI
│   └── settings/       # 设置窗口 UI
├── package.json
└── build.ps1           # 构建脚本
```

**`build.ps1` 提供以下目标：**

```powershell
# 开发模式（热重载）
.\build.ps1 dev

# 生产构建（生成 .msi + .exe installer）
.\build.ps1 build

# 仅构建前端
.\build.ps1 frontend

# 清理构建产物
.\build.ps1 clean
```

生产构建通过 `tauri build` 生成：`src-tauri/target/release/bundle/msi/*.msi`（NSIS 安装包）和 `bundle/nsis/*.exe`（单文件安装程序），最终包体积约 10–18MB，无需用户预装任何运行时。

---

## 关键 crate 版本锁定

```toml
[dependencies]
tauri = "2.1"
windows = "0.58"
cpal = "0.15"
arboard = "3.4"
reqwest = "0.12"
window-vibrancy = "0.5"
whisper-rs = { version = "0.11", optional = true }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
anyhow = "1"

[features]
local-whisper = ["whisper-rs"]
```