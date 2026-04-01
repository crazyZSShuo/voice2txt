// main.rs — VoiceInput Tauri v2 entry point
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod diag;
mod hotkey;
mod inject;
mod llm;
mod stt;
mod stt_router;
mod windows_stt;

use std::sync::{Arc, Mutex};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Listener, Manager, State,
};

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SharedState {
    pub config: Arc<Mutex<config::AppConfig>>,
    pub audio: Arc<Mutex<audio::AudioCapture>>,
    pub windows_speech_session: Arc<Mutex<Option<windows_stt::WindowsSpeechSession>>>,
    recording: Arc<Mutex<RecordingState>>,
    pub capsule_phase: Arc<Mutex<String>>,
    pub capsule_loaded: Arc<Mutex<bool>>,
}

// AppState is the Tauri-managed type. AudioCapture has unsafe Send+Sync,
// so SharedState is Send+Sync as well.
pub struct AppState(pub SharedState);
pub struct TrayState(pub Mutex<Option<TrayIcon>>);

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> config::AppConfig {
    state.inner().0.config.lock().unwrap().clone()
}

#[tauri::command]
fn save_config(new_cfg: config::AppConfig, state: State<'_, AppState>) -> Result<(), String> {
    let backend = stt_router::selected_backend(&new_cfg);
    stt_router::ensure_backend_supported(backend).map_err(|e| e.to_string())?;
    config::save_config(&new_cfg).map_err(|e| e.to_string())?;
    *state.inner().0.config.lock().unwrap() = new_cfg;
    Ok(())
}

#[tauri::command]
async fn test_stt(cfg: config::AppConfig) -> Result<String, String> {
    let backend = stt_router::selected_backend(&cfg);
    stt_router::ensure_backend_supported(backend).map_err(|e| e.to_string())?;

    match backend {
        config::SttBackend::Custom => stt::test_connection(&cfg.stt)
            .await
            .map_err(|e| e.to_string()),
        config::SttBackend::WindowsSpeech => windows_stt::test_connection()
            .await
            .map_err(|e| e.to_string()),
    }
}

#[tauri::command]
async fn test_llm(cfg: config::LlmConfig) -> Result<String, String> {
    llm::test_connection(&cfg).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn get_capsule_state(state: State<'_, AppState>) -> String {
    state.inner().0.capsule_phase.lock().unwrap().clone()
}

#[tauri::command]
fn capsule_frontend_log(message: String) {
    diag::write(&format!("capsule:frontend:{}", message));
}

#[tauri::command]
fn sync_capsule_window(width: u32, app: AppHandle) -> Result<(), String> {
    let width = width.clamp(260, 620);
    if let Some(win) = app.get_webview_window("capsule") {
        win.set_size(tauri::PhysicalSize::new(width, 56))
            .map_err(|e| e.to_string())?;
        position_capsule(&win);
        apply_capsule_shape(&win);
    }
    Ok(())
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn set_language(lang: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut cfg = state.inner().0.config.lock().unwrap();
    cfg.language = lang;
    config::save_config(&cfg).map_err(|e| e.to_string())
}

fn set_capsule_phase(shared: &SharedState, phase: &str) {
    *shared.capsule_phase.lock().unwrap() = phase.to_string();
}

fn emit_capsule_event<S>(app: &AppHandle, event: &str, payload: S)
where
    S: serde::Serialize + Clone,
{
    if let Some(win) = app.get_webview_window("capsule") {
        let _ = win.emit(event, payload.clone());
    }
    let _ = app.emit(event, payload);
}

// ── Recording flow ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingState {
    Idle,
    CustomRecording,
    WindowsStarting,
    WindowsStopRequested,
    WindowsRecording,
    WindowsStopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartAction {
    IgnoreAlreadyRecording,
    StartCustom,
    StartWindows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopAction {
    IgnoreNotRecording,
    StopCustomSynchronously,
    AwaitWindowsStartupThenStop,
    StopWindowsSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsStartAction {
    EnterRecording,
    StopImmediately,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingUiContract {
    ShowCapsuleAndEmitStarted,
}

fn begin_recording(state: &mut RecordingState, backend: config::SttBackend) -> StartAction {
    match *state {
        RecordingState::Idle => match backend {
            config::SttBackend::Custom => {
                *state = RecordingState::CustomRecording;
                StartAction::StartCustom
            }
            config::SttBackend::WindowsSpeech => {
                *state = RecordingState::WindowsStarting;
                StartAction::StartWindows
            }
        },
        _ => StartAction::IgnoreAlreadyRecording,
    }
}

fn reset_recording_state(state: &mut RecordingState) {
    *state = RecordingState::Idle;
}

fn request_stop(state: &mut RecordingState) -> StopAction {
    match *state {
        RecordingState::Idle | RecordingState::WindowsStopping => StopAction::IgnoreNotRecording,
        RecordingState::CustomRecording => StopAction::StopCustomSynchronously,
        RecordingState::WindowsStarting => {
            *state = RecordingState::WindowsStopRequested;
            StopAction::AwaitWindowsStartupThenStop
        }
        RecordingState::WindowsStopRequested => StopAction::IgnoreNotRecording,
        RecordingState::WindowsRecording => {
            *state = RecordingState::WindowsStopping;
            StopAction::StopWindowsSession
        }
    }
}

fn finish_custom_stop(state: &mut RecordingState) {
    if matches!(*state, RecordingState::CustomRecording) {
        *state = RecordingState::Idle;
    }
}

fn resolve_windows_start_success(state: &mut RecordingState) -> WindowsStartAction {
    match *state {
        RecordingState::WindowsStarting => {
            *state = RecordingState::WindowsRecording;
            WindowsStartAction::EnterRecording
        }
        RecordingState::WindowsStopRequested => {
            *state = RecordingState::WindowsStopping;
            WindowsStartAction::StopImmediately
        }
        _ => {
            *state = RecordingState::WindowsStopping;
            WindowsStartAction::StopImmediately
        }
    }
}

fn finish_windows_stop(state: &mut RecordingState) {
    if matches!(*state, RecordingState::WindowsStopping) {
        *state = RecordingState::Idle;
    }
}

fn recording_ui_contract(
    backend: config::SttBackend,
    windows_start_action: WindowsStartAction,
) -> RecordingUiContract {
    match backend {
        config::SttBackend::Custom => RecordingUiContract::ShowCapsuleAndEmitStarted,
        config::SttBackend::WindowsSpeech => match windows_start_action {
            WindowsStartAction::EnterRecording | WindowsStartAction::StopImmediately => {
                RecordingUiContract::ShowCapsuleAndEmitStarted
            }
        },
    }
}

fn apply_recording_ui_contract(
    contract: RecordingUiContract,
    shared: &SharedState,
    app: &AppHandle,
) {
    match contract {
        RecordingUiContract::ShowCapsuleAndEmitStarted => {
            set_capsule_phase(shared, "recording");
            if let Some(win) = app.get_webview_window("capsule") {
                position_capsule(&win);
                let _ = win.show();
            }
            emit_capsule_event(app, "recording-started", ());
        }
    }
}

fn announce_processing(shared: &SharedState, app: &AppHandle) {
    diag::write("event:stop_recording:task_started");
    set_capsule_phase(shared, "processing");
    emit_capsule_event(app, "processing-started", ());
}

async fn finalize_recording(
    app: AppHandle,
    shared: SharedState,
    cfg: config::AppConfig,
    transcript: anyhow::Result<String>,
) {
    let transcript = match transcript {
        Ok(t) => t,
        Err(e) => {
            set_capsule_phase(&shared, "error");
            diag::write(&format!("event:stt:error:{}", e));
            log::error!("STT: {}", e);
            emit_capsule_event(&app, "recording-error", e.to_string());
            hide_capsule(&app);
            return;
        }
    };
    diag::write(&format!("event:stt:done:chars={}", transcript.len()));

    if transcript.trim().is_empty() {
        diag::write("event:stt:empty_transcript");
        set_capsule_phase(&shared, "idle");
        hide_capsule(&app);
        return;
    }

    let final_text = if cfg.llm.enabled && !cfg.llm.api_key.is_empty() {
        diag::write("event:llm:start");
        set_capsule_phase(&shared, "refining");
        emit_capsule_event(&app, "refining-started", ());
        emit_capsule_event(&app, "transcript-clear", ());
        match llm::refine_transcript(&transcript, &cfg.llm, &app).await {
            Ok(r) if !r.trim().is_empty() => {
                diag::write(&format!("event:llm:done:chars={}", r.len()));
                r
            }
            _ => {
                diag::write("event:llm:fallback_to_transcript");
                transcript
            }
        }
    } else {
        diag::write("event:llm:disabled");
        transcript
    };

    emit_capsule_event(&app, "injecting", final_text.clone());
    diag::write(&format!("event:inject:start:chars={}", final_text.len()));
    if let Err(e) = inject::inject_text(&final_text).await {
        set_capsule_phase(&shared, "error");
        diag::write(&format!("event:inject:error:{}", e));
        log::error!("Inject: {}", e);
        emit_capsule_event(&app, "recording-error", e.to_string());
        tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
    } else {
        diag::write("event:inject:done");
        set_capsule_phase(&shared, "done");
        emit_capsule_event(&app, "recording-done", final_text.clone());
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    hide_capsule(&app);
    diag::write("event:stop_recording:task_finished");
    set_capsule_phase(&shared, "idle");
}

fn start_recording(app: AppHandle, shared: SharedState) {
    diag::write("event:start_recording:enter");
    let cfg = shared.config.lock().unwrap().clone();
    let backend = stt_router::selected_backend(&cfg);
    if let Err(e) = stt_router::ensure_backend_supported(backend) {
        set_capsule_phase(&shared, "error");
        emit_capsule_event(&app, "recording-error", e.to_string());
        return;
    }

    let start_action = {
        let mut rec = shared.recording.lock().unwrap();
        let action = begin_recording(&mut rec, backend);
        if matches!(action, StartAction::IgnoreAlreadyRecording) {
            diag::write("event:start_recording:ignored_already_recording");
        }
        action
    };
    if matches!(start_action, StartAction::IgnoreAlreadyRecording) {
        return;
    }

    if matches!(start_action, StartAction::StartCustom) {
        let mut audio = shared.audio.lock().unwrap();
        match audio.start(app.clone()) {
            Ok(_) => {
                diag::write("event:start_recording:audio_started");
                apply_recording_ui_contract(
                    RecordingUiContract::ShowCapsuleAndEmitStarted,
                    &shared,
                    &app,
                );
                diag::write("event:start_recording:emitted_recording_started");
            }
            Err(e) => {
                reset_recording_state(&mut shared.recording.lock().unwrap());
                set_capsule_phase(&shared, "error");
                diag::write(&format!("event:start_recording:error:{}", e));
                log::error!("Audio start: {}", e);
                if let Some(win) = app.get_webview_window("capsule") {
                    position_capsule(&win);
                    let _ = win.show();
                }
                emit_capsule_event(&app, "recording-error", e.to_string());
            }
        }
        return;
    }

    let app2 = app.clone();
    let shared2 = shared.clone();
    let cfg2 = cfg.clone();
    tauri::async_runtime::spawn(async move {
        match windows_stt::start_recognition(&app2).await {
            Ok(session) => {
                let start_action = {
                    let mut recording = shared2.recording.lock().unwrap();
                    resolve_windows_start_success(&mut recording)
                };

                match start_action {
                    WindowsStartAction::EnterRecording => {
                        *shared2.windows_speech_session.lock().unwrap() = Some(session);
                        apply_recording_ui_contract(
                            recording_ui_contract(
                                config::SttBackend::WindowsSpeech,
                                WindowsStartAction::EnterRecording,
                            ),
                            &shared2,
                            &app2,
                        );
                        diag::write("event:start_recording:windows_started");
                    }
                    WindowsStartAction::StopImmediately => {
                        diag::write("event:start_recording:windows_quick_release");
                        apply_recording_ui_contract(
                            recording_ui_contract(
                                config::SttBackend::WindowsSpeech,
                                WindowsStartAction::StopImmediately,
                            ),
                            &shared2,
                            &app2,
                        );
                        announce_processing(&shared2, &app2);
                        let transcript = windows_stt::stop_recognition(session, &app2).await;
                        finish_windows_stop(&mut shared2.recording.lock().unwrap());
                        finalize_recording(app2, shared2, cfg2, transcript).await;
                    }
                }
            }
            Err(e) => {
                reset_recording_state(&mut shared2.recording.lock().unwrap());
                set_capsule_phase(&shared2, "error");
                diag::write(&format!("event:start_recording:error:{}", e));
                log::error!("Windows speech start: {}", e);
                if let Some(win) = app2.get_webview_window("capsule") {
                    position_capsule(&win);
                    let _ = win.show();
                }
                emit_capsule_event(&app2, "recording-error", e.to_string());
            }
        }
    });
}

fn stop_recording(app: AppHandle, shared: SharedState) {
    diag::write("event:stop_recording:enter");
    let stop_action = {
        let mut rec = shared.recording.lock().unwrap();
        let action = request_stop(&mut rec);
        if matches!(action, StopAction::IgnoreNotRecording) {
            diag::write("event:stop_recording:ignored_not_recording");
        }
        action
    };
    if matches!(stop_action, StopAction::IgnoreNotRecording) {
        return;
    }

    let cfg = shared.config.lock().unwrap().clone();
    match stop_action {
        StopAction::IgnoreNotRecording => {}
        StopAction::AwaitWindowsStartupThenStop => {
            diag::write("event:stop_recording:waiting_for_windows_start");
        }
        StopAction::StopCustomSynchronously => {
            let samples = shared.audio.lock().unwrap().stop();
            diag::write(&format!("event:stop_recording:samples={}", samples.len()));
            finish_custom_stop(&mut shared.recording.lock().unwrap());

            let app2 = app.clone();
            let shared2 = shared.clone();
            tauri::async_runtime::spawn(async move {
                announce_processing(&shared2, &app2);
                let wav = audio::pcm_to_wav(&samples);
                diag::write(&format!("event:stt:start:wav_bytes={}", wav.len()));
                let transcript =
                    stt::transcribe_streaming(wav, &cfg.stt, &cfg.language, &app2).await;
                finalize_recording(app2, shared2, cfg, transcript).await;
            });
        }
        StopAction::StopWindowsSession => {
            let session = shared
                .windows_speech_session
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| anyhow::anyhow!("Windows speech session was not started"));
            let app2 = app.clone();
            let shared2 = shared.clone();
            tauri::async_runtime::spawn(async move {
                announce_processing(&shared2, &app2);
                let transcript = match session {
                    Ok(session) => windows_stt::stop_recognition(session, &app2).await,
                    Err(err) => Err(err),
                };
                finish_windows_stop(&mut shared2.recording.lock().unwrap());
                finalize_recording(app2, shared2, cfg, transcript).await;
            });
        }
    }
}

fn hide_capsule(app: &AppHandle) {
    diag::write("event:capsule:hide");
    if let Some(win) = app.get_webview_window("capsule") {
        let _ = win.hide();
    }
}

fn position_capsule(win: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    if let Ok(Some(m)) = win.primary_monitor() {
        use tauri::PhysicalPosition;
        let area = m.work_area();
        let size = win
            .outer_size()
            .unwrap_or_else(|_| tauri::PhysicalSize::new(220, 56));
        let x = area.position.x + ((area.size.width as i32 - size.width as i32) / 2).max(0);
        let y = area.position.y + (area.size.height as i32 - size.height as i32 - 24).max(0);
        diag::write(&format!("event:capsule:position:x={},y={}", x, y));
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

// ── Tray menu ─────────────────────────────────────────────────────────────────

fn build_tray(app: &AppHandle, cfg: &config::AppConfig) -> tauri::Result<TrayIcon> {
    let lang_en = CheckMenuItem::with_id(
        app,
        "lang-en",
        "English",
        true,
        cfg.language == "en",
        None::<&str>,
    )?;
    let lang_zh = CheckMenuItem::with_id(
        app,
        "lang-zh",
        "简体中文",
        true,
        cfg.language == "zh",
        None::<&str>,
    )?;
    let lang_tw = CheckMenuItem::with_id(
        app,
        "lang-tw",
        "繁體中文",
        true,
        cfg.language == "zh-TW",
        None::<&str>,
    )?;
    let lang_ja = CheckMenuItem::with_id(
        app,
        "lang-ja",
        "日本語",
        true,
        cfg.language == "ja",
        None::<&str>,
    )?;
    let lang_ko = CheckMenuItem::with_id(
        app,
        "lang-ko",
        "한국어",
        true,
        cfg.language == "ko",
        None::<&str>,
    )?;
    let lang_sub = Submenu::with_id_and_items(
        app,
        "lang-sub",
        "Language / 语言",
        true,
        &[&lang_en, &lang_zh, &lang_tw, &lang_ja, &lang_ko],
    )?;

    let llm_toggle = CheckMenuItem::with_id(
        app,
        "llm-toggle",
        "Enable LLM Refinement",
        true,
        cfg.llm.enabled,
        None::<&str>,
    )?;
    let llm_settings = MenuItem::with_id(app, "open-settings", "Settings…", true, None::<&str>)?;
    let llm_sub = Submenu::with_id_and_items(
        app,
        "llm-sub",
        "LLM Refinement",
        true,
        &[&llm_toggle, &llm_settings],
    )?;

    let auto_start = CheckMenuItem::with_id(
        app,
        "auto-start",
        "Launch at Login",
        true,
        cfg.auto_start,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit VoiceInput", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&lang_sub, &llm_sub, &auto_start, &sep, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .tooltip("VoiceInput — Hold Right Alt to record")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| handle_menu_event(app, ev.id().as_ref()))
        .build(app)
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    diag::write(&format!("event:tray_menu:{}", id));
    let shared = app.state::<AppState>().inner().0.clone();
    match id {
        "quit" => app.exit(0),
        "open-settings" => open_settings(app.clone()),
        "llm-toggle" => {
            let mut cfg = shared.config.lock().unwrap();
            cfg.llm.enabled = !cfg.llm.enabled;
            let _ = config::save_config(&cfg);
        }
        "auto-start" => {
            let mut cfg = shared.config.lock().unwrap();
            cfg.auto_start = !cfg.auto_start;
            let enabled = cfg.auto_start;
            let _ = config::save_config(&cfg);
            drop(cfg);
            if let Ok(exe) = std::env::current_exe() {
                let _ = config::set_auto_start(enabled, &exe.to_string_lossy());
            }
        }
        lang_id if lang_id.starts_with("lang-") => {
            let lang = match lang_id {
                "lang-en" => "en",
                "lang-zh" => "zh",
                "lang-tw" => "zh-TW",
                "lang-ja" => "ja",
                "lang-ko" => "ko",
                _ => return,
            };
            let mut cfg = shared.config.lock().unwrap();
            cfg.language = lang.to_string();
            let _ = config::save_config(&cfg);
        }
        _ => {}
    }
}

// ── Windows capsule chrome ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn apply_capsule_shape(win: &tauri::WebviewWindow) {
    use std::mem::size_of;
    use windows::Win32::Foundation::{BOOL, HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    if let Ok(raw) = win.hwnd() {
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        unsafe {
            let corner_pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as _,
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );

            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let width = (rect.right - rect.left).max(1);
                let height = (rect.bottom - rect.top).max(1);
                let radius = 28.min(height / 2).max(1);
                let region =
                    CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
                let _ = SetWindowRgn(hwnd, region, BOOL(1));
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_capsule_shape(_win: &tauri::WebviewWindow) {}

#[cfg(target_os = "windows")]
fn apply_capsule_chrome(win: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    if let Ok(raw) = win.hwnd() {
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                ex | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize,
            );
        }
        if let Err(e) = window_vibrancy::apply_acrylic(win, Some((18, 18, 28, 180))) {
            log::warn!("Acrylic not available: {}", e);
        }
        apply_capsule_shape(win);
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_capsule_chrome(_win: &tauri::WebviewWindow) {}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    diag::install_panic_hook();
    diag::write(&format!("app:start:version={}", env!("CARGO_PKG_VERSION")));
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let initial_cfg = config::load_config();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState(SharedState {
            config: Arc::new(Mutex::new(initial_cfg.clone())),
            audio: Arc::new(Mutex::new(audio::AudioCapture::new())),
            windows_speech_session: Arc::new(Mutex::new(None)),
            recording: Arc::new(Mutex::new(RecordingState::Idle)),
            capsule_phase: Arc::new(Mutex::new("idle".to_string())),
            capsule_loaded: Arc::new(Mutex::new(false)),
        }))
        .manage(TrayState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            test_stt,
            test_llm,
            open_settings,
            set_language,
            get_capsule_state,
            capsule_frontend_log,
            sync_capsule_window,
        ])
        .on_page_load(|webview, payload| {
            if webview.label() != "capsule" {
                return;
            }

            let event = match payload.event() {
                tauri::webview::PageLoadEvent::Started => "started",
                tauri::webview::PageLoadEvent::Finished => "finished",
            };
            diag::write(&format!("page_load:capsule:{}:{}", event, payload.url()));

            if let tauri::webview::PageLoadEvent::Finished = payload.event() {
                let shared = webview.state::<AppState>().inner().0.clone();
                *shared.capsule_loaded.lock().unwrap() = true;

                if let Some(win) = webview.app_handle().get_webview_window("capsule") {
                    let _ = win.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
                    if shared.capsule_phase.lock().unwrap().as_str() != "idle" {
                        diag::write("event:capsule:show_after_page_load");
                        position_capsule(&win);
                        let _ = win.show();
                    }
                }
            }
        })
        .setup(move |app| {
            diag::write("app:setup:enter");
            let handle = app.handle().clone();

            if let Some(capsule) = app.get_webview_window("capsule") {
                diag::write("app:setup:capsule_window_found");
                let _ = capsule.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
                apply_capsule_chrome(&capsule);
            }

            let tray = build_tray(app.handle(), &initial_cfg)?;
            *app.state::<TrayState>().0.lock().unwrap() = Some(tray);
            diag::write("app:setup:tray_created");
            hotkey::start_hook(handle.clone())?;
            diag::write("app:setup:hotkey_hook_started");

            // Wire hotkey events → recording
            let shared = app.state::<AppState>().inner().0.clone();
            let (sh1, sh2) = (shared.clone(), shared.clone());
            let (h1, h2) = (handle.clone(), handle.clone());

            app.listen("hotkey-press", move |_| {
                diag::write("event:hotkey_press:listener");
                start_recording(h1.clone(), sh1.clone())
            });
            app.listen("hotkey-release", move |_| {
                diag::write("event:hotkey_release:listener");
                stop_recording(h2.clone(), sh2.clone())
            });

            diag::write("app:setup:done");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri runtime error");
}

#[cfg(test)]
mod recording_state_tests {
    use super::*;

    #[test]
    fn custom_stop_stays_busy_until_sync_harvest_finishes() {
        let mut state = RecordingState::CustomRecording;

        assert_eq!(
            request_stop(&mut state),
            StopAction::StopCustomSynchronously
        );
        assert_eq!(state, RecordingState::CustomRecording);

        finish_custom_stop(&mut state);

        assert_eq!(state, RecordingState::Idle);
    }

    #[test]
    fn windows_quick_release_waits_for_start_and_then_stops() {
        let mut state = RecordingState::WindowsStarting;

        assert_eq!(
            request_stop(&mut state),
            StopAction::AwaitWindowsStartupThenStop
        );
        assert_eq!(state, RecordingState::WindowsStopRequested);

        assert_eq!(
            resolve_windows_start_success(&mut state),
            WindowsStartAction::StopImmediately
        );
        assert_eq!(state, RecordingState::WindowsStopping);
    }

    #[test]
    fn start_requests_only_begin_from_idle() {
        let mut idle = RecordingState::Idle;
        let mut busy = RecordingState::WindowsRecording;

        assert_eq!(
            begin_recording(&mut idle, config::SttBackend::Custom),
            StartAction::StartCustom
        );
        assert_eq!(idle, RecordingState::CustomRecording);

        assert_eq!(
            begin_recording(&mut busy, config::SttBackend::Custom),
            StartAction::IgnoreAlreadyRecording
        );
        assert_eq!(busy, RecordingState::WindowsRecording);
    }

    #[test]
    fn quick_release_windows_path_still_establishes_recording_ui_contract() {
        assert_eq!(
            recording_ui_contract(
                config::SttBackend::WindowsSpeech,
                WindowsStartAction::StopImmediately
            ),
            RecordingUiContract::ShowCapsuleAndEmitStarted
        );
    }
}
