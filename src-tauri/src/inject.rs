/// inject.rs — Inject text via clipboard + Ctrl+V simulation.
/// Uses spawn_blocking so Windows HWND (non-Send) never crosses an await point.
use crate::diag;
use anyhow::Result;

pub async fn inject_text(text: &str) -> Result<()> {
    if text.is_empty() {
        diag::write("inject:skip_empty_text");
        return Ok(());
    }
    let text = text.to_string();
    diag::write(&format!("inject:spawn_blocking:chars={}", text.len()));
    // Run all Windows API calls on a blocking thread — avoids HWND Send requirement
    tokio::task::spawn_blocking(move || inject_sync(&text))
        .await
        .map_err(|e| anyhow::anyhow!("inject task panicked: {}", e))??;
    diag::write("inject:spawn_blocking:done");
    Ok(())
}

// ── Synchronous implementation (runs on blocking thread) ─────────────────────

#[cfg(target_os = "windows")]
fn inject_sync(text: &str) -> Result<()> {
    use std::thread::sleep;
    use std::time::Duration;

    use arboard::Clipboard;
    use windows::Win32::UI::Input::Ime::{
        ImmGetContext, ImmGetOpenStatus, ImmReleaseContext, ImmSetOpenStatus,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let hwnd = unsafe { GetForegroundWindow() };
    diag::write(&format!("inject:sync:start:chars={}", text.len()));

    // 1. Save original clipboard
    let mut clipboard = Clipboard::new()?;
    let original = clipboard.get_text().ok();

    // 2. Temporarily close IME to avoid interference
    let (ime_open, himc) = unsafe {
        let himc = ImmGetContext(hwnd);
        let open = ImmGetOpenStatus(himc).as_bool();
        if open {
            ImmSetOpenStatus(himc, false);
        }
        (open, himc)
    };

    // 3. Write text to clipboard
    clipboard.set_text(text)?;
    diag::write("inject:clipboard:set_text");
    sleep(Duration::from_millis(30));

    // 4. Simulate Ctrl+V
    let inputs: [INPUT; 4] = [
        make_key(VK_CONTROL.0, false),
        make_key(VK_V.0, false),
        make_key(VK_V.0, true),
        make_key(VK_CONTROL.0, true),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    diag::write("inject:sendinput:ctrl_v");
    sleep(Duration::from_millis(80));

    // 5. Restore IME
    unsafe {
        if ime_open {
            ImmSetOpenStatus(himc, true);
        }
        ImmReleaseContext(hwnd, himc);
    }

    // 6. Restore original clipboard
    sleep(Duration::from_millis(120));
    match original {
        Some(orig) => {
            let _ = clipboard.set_text(orig);
        }
        None => {
            let _ = clipboard.clear();
        }
    }

    diag::write("inject:sync:done");

    Ok(())
}

#[cfg(target_os = "windows")]
fn make_key(vk: u16, key_up: bool) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(not(target_os = "windows"))]
fn inject_sync(text: &str) -> Result<()> {
    log::info!("inject_sync stub: {}", text);
    Ok(())
}
