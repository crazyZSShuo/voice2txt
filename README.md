# VoiceInput

Windows system-tray voice input application. Hold **Right Alt** to record; release to transcribe and inject text into the focused window.

## Features

- 🎙️ **Hold Right Alt** — records audio while held, transcribes on release
- 🌊 **Live waveform** — RMS-driven bar animation during recording
- 🪟 **Windows-native speech recognition** — default backend on Windows, with manual fallback to OpenAI-compatible Custom STT
- 🌐 **OpenAI-compatible Custom STT** — works with OpenAI Whisper, local whisper.cpp servers, or any `/v1/audio/transcriptions` compatible endpoint
- 🧠 **Optional LLM correction** — fixes ASR homophones (`配森` → `Python`) via streaming GPT
- 💬 **Floating capsule HUD** — elegant no-border overlay at screen bottom
- 🔒 **DPAPI-encrypted keys** — API keys stored encrypted via Windows DPAPI
- 🌏 **Language switching** — zh / zh-TW / en / ja / ko from tray menu
- 🚀 **Auto-start** — optional registry entry for launch at login
- 📦 **10–18 MB installer** — no user runtime dependencies

---

## Requirements

| Tool | Version |
|---|---|
| Rust | 1.77+ (stable) |
| Node.js | 18+ |
| Windows | 10 22H2+ / 11 |
| WebView2 | Auto-installed if missing |

Install Tauri CLI:
```powershell
cargo install tauri-cli --version "^2.1"
```

---

## Quick Start

```powershell
# 1. Clone and enter project
cd voice-input

# 2. Generate placeholder icons (one-time)
python scripts\gen_icons.py

# 3. Install JS dependencies
npm install

# 4. Start development server (hot reload)
.\build.ps1 dev
```

Then open **Settings** from the tray icon and configure your STT API key.

---

## Build

```powershell
# Production installer (.msi + .exe)
.\build.ps1 build

# Output:
#   src-tauri\target\release\bundle\msi\VoiceInput_*.msi
#   src-tauri\target\release\bundle\nsis\VoiceInput_*-setup.exe
```

---

## Configuration

Settings are stored at `%APPDATA%\VoiceInput\config.json`. API keys are encrypted with Windows DPAPI — they're tied to your Windows user account.

### STT (Speech-to-Text)

VoiceInput supports two STT backends:

| Backend | Default on Windows | Notes |
|---|---|---|
| `Windows SpeechRecognizer` | Yes | Uses Windows speech services and ignores the app language setting. |
| `Custom STT` | No | Uses the configured `/v1/audio/transcriptions` endpoint and the app language setting. |

| Field | Default | Notes |
|---|---|---|
| API Base URL | `https://api.openai.com` | Used by `Custom STT` |
| API Key | _(required)_ | `sk-…` for OpenAI |
| Model | `whisper-1` | Used by `Custom STT`; `large-v3` works for local Whisper |

**Windows SpeechRecognizer**: uses Windows speech services and ignores the app language setting.

**Custom STT**: run [whisper.cpp](https://github.com/ggerganov/whisper.cpp) with its built-in server, then set Base URL to `http://localhost:8080` or another OpenAI-compatible `/v1/audio/transcriptions` endpoint. This backend uses the app language setting.

### LLM Correction (optional)

| Field | Default | Notes |
|---|---|---|
| Enabled | `false` | Toggle from tray menu or settings |
| API Base URL | `https://api.openai.com` | Can differ from STT endpoint |
| Model | `gpt-4o-mini` | Fast + cheap for short corrections |

The LLM system prompt is extremely conservative — it only fixes obvious ASR errors (Chinese homophones for tech terms) and never rewrites intentional content.

---

## Project Structure

```
voice-input/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs        # App entry, tray, event orchestration
│   │   ├── hotkey.rs      # WH_KEYBOARD_LL global hook (Right Alt)
│   │   ├── audio.rs       # cpal/WASAPI capture + WAV encoder + RMS
│   │   ├── stt.rs         # OpenAI-compatible /v1/audio/transcriptions
│   │   ├── llm.rs         # /v1/chat/completions streaming correction
│   │   ├── inject.rs      # Clipboard + Ctrl+V injection, IME control
│   │   └── config.rs      # JSON config + DPAPI encryption + auto-start
│   ├── icons/             # App icons (generate with scripts/gen_icons.py)
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── capsule/
│   │   ├── Capsule.tsx    # Floating HUD component + waveform
│   │   ├── capsule.css    # Capsule styles + spring animations
│   │   └── main.tsx
│   └── settings/
│       ├── Settings.tsx   # Full settings UI
│       └── main.tsx
├── capsule.html           # Entry HTML for capsule window
├── settings.html          # Entry HTML for settings window
├── scripts/
│   └── gen_icons.py       # Placeholder icon generator
├── build.ps1              # Build script (dev / build / frontend / clean)
├── package.json
├── vite.config.ts
└── tsconfig.json
```

---

## Key Dependencies

| Crate | Purpose |
|---|---|
| `tauri 2.1` | App framework, tray, multi-window |
| `windows 0.58` | WH_KEYBOARD_LL hook, IME control, DPAPI, registry |
| `cpal 0.15` | Cross-platform audio (WASAPI on Windows) |
| `arboard 3.4` | Clipboard read/write |
| `reqwest 0.12` | Async HTTP for STT + LLM APIs |
| `window-vibrancy 0.5` | Acrylic blur effect on capsule window |
| `serde / serde_json` | Config serialization |
| `tokio 1` | Async runtime |

---

## Troubleshooting

**Right Alt triggers AltGr on some keyboards**
AltGr (ISO keyboards) sends `VK_RMENU` simultaneously with `VK_LCONTROL`. The hook suppresses `VK_RMENU` so this shouldn't type characters, but if you have issues, check your keyboard layout settings.

**Blank capsule window**
WebView2 may need reinstalling. Run: `winget install Microsoft.EdgeWebView2Runtime`

**STT returns empty**
Check your microphone permissions in Windows Settings → Privacy → Microphone.

**Acrylic blur not showing (Windows 10)**
Windows 10 acrylic support is limited. The CSS `backdrop-filter` fallback provides a subtle blur effect.
