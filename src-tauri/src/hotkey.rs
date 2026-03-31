/// hotkey.rs — Global WH_KEYBOARD_LL keyboard hook for Right Alt (VK_RMENU).
/// Suppresses the key's default behavior and emits Tauri events.
use crate::diag;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

static IS_RECORDING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::VK_RMENU;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[cfg(target_os = "windows")]
static mut HOOK_HANDLE: Option<HHOOK> = None;
#[cfg(target_os = "windows")]
static mut APP_HANDLE: Option<AppHandle> = None;

#[cfg(target_os = "windows")]
unsafe extern "system" fn keyboard_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 {
        let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
        if kb.vkCode == VK_RMENU.0 as u32 {
            let msg = w_param.0 as u32;
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

            if is_down && !IS_RECORDING.load(Ordering::SeqCst) {
                IS_RECORDING.store(true, Ordering::SeqCst);
                diag::write("hook:right_alt:down");
                if let Some(h) = APP_HANDLE.as_ref() {
                    let _ = h.emit("hotkey-press", ());
                }
                return LRESULT(1); // suppress
            }
            if is_up && IS_RECORDING.load(Ordering::SeqCst) {
                IS_RECORDING.store(false, Ordering::SeqCst);
                diag::write("hook:right_alt:up");
                if let Some(h) = APP_HANDLE.as_ref() {
                    let _ = h.emit("hotkey-release", ());
                }
                return LRESULT(1); // suppress
            }
            if IS_RECORDING.load(Ordering::SeqCst) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, n_code, w_param, l_param)
}

#[cfg(target_os = "windows")]
pub fn start_hook(app: AppHandle) -> Result<()> {
    std::thread::spawn(move || unsafe {
        diag::write("hook:thread:start");
        APP_HANDLE = Some(app);
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0)
            .expect("SetWindowsHookExW failed");
        HOOK_HANDLE = Some(hook);
        diag::write("hook:installed");

        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                diag::write(&format!("hook:message_loop_exit:ret={}", ret.0));
                break;
            }
        }
        if let Some(h) = HOOK_HANDLE.take() {
            let _ = UnhookWindowsHookEx(h);
        }
        diag::write("hook:thread:end");
    });
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn start_hook(_app: AppHandle) -> Result<()> {
    log::info!("Hotkey hook stub (non-Windows build)");
    Ok(())
}
