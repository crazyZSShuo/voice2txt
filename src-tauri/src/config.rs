/// config.rs — App configuration with DPAPI-encrypted API keys.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub auto_start: bool,
    pub stt: SttConfig,
    pub llm: LlmConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: "zh".to_string(),
            auto_start: false,
            stt: SttConfig {
                base_url: "https://api.openai.com".to_string(),
                api_key: String::new(),
                model: "whisper-1".to_string(),
            },
            llm: LlmConfig {
                enabled: false,
                base_url: "https://api.openai.com".to_string(),
                api_key: String::new(),
                model: "gpt-4o-mini".to_string(),
            },
        }
    }
}

pub fn config_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata)
        .join("VoiceInput")
        .join("config.json")
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    if !path.exists() {
        return AppConfig::default();
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut cfg: AppConfig = serde_json::from_str(&content).unwrap_or_default();
    cfg.stt.api_key = decrypt_field(&cfg.stt.api_key);
    cfg.llm.api_key = decrypt_field(&cfg.llm.api_key);
    cfg
}

pub fn save_config(cfg: &AppConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create config dir")?;
    }
    let mut to_save = cfg.clone();
    to_save.stt.api_key = encrypt_field(&cfg.stt.api_key);
    to_save.llm.api_key = encrypt_field(&cfg.llm.api_key);
    let content = serde_json::to_string_pretty(&to_save)?;
    std::fs::write(&path, content).context("write config")?;
    Ok(())
}

// ── DPAPI encryption (Windows) ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn encrypt_field(plain: &str) -> String {
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
    };

    if plain.is_empty() {
        return String::new();
    }
    let data = plain.as_bytes();
    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    unsafe {
        let ok = CryptProtectData(&in_blob, None, None, None, None, 0, &mut out_blob);
        if ok.is_ok() && !out_blob.pbData.is_null() {
            let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
            let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, slice);
            // Free the buffer allocated by CryptProtectData using raw Windows heap free
            // We use VirtualFree-equivalent: just use LocalFree via raw pointer arithmetic.
            // Since LocalFree may not be available in all windows-rs versions,
            // we use the Win32 LocalFree via the windows-sys approach as a raw extern.
            free_local(out_blob.pbData);
            return encoded;
        }
    }
    // Fallback: store as-is
    plain.to_string()
}

#[cfg(target_os = "windows")]
fn decrypt_field(encoded: &str) -> String {
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    if encoded.is_empty() {
        return String::new();
    }
    let Ok(data) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
    else {
        return encoded.to_string(); // stored as plaintext (legacy)
    };
    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    unsafe {
        let ok = CryptUnprotectData(&in_blob, None, None, None, None, 0, &mut out_blob);
        if ok.is_ok() && !out_blob.pbData.is_null() {
            let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
            let text = String::from_utf8_lossy(slice).to_string();
            free_local(out_blob.pbData);
            return text;
        }
    }
    encoded.to_string()
}

/// Free a buffer allocated by CryptProtectData / CryptUnprotectData.
/// These functions use LocalAlloc internally. We call LocalFree via a raw extern.
#[cfg(target_os = "windows")]
unsafe fn free_local(ptr: *mut u8) {
    // Declare LocalFree manually to avoid windows-rs version-specific import issues
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }
    LocalFree(ptr as *mut core::ffi::c_void);
}

#[cfg(not(target_os = "windows"))]
fn encrypt_field(plain: &str) -> String {
    plain.to_string()
}

#[cfg(not(target_os = "windows"))]
fn decrypt_field(encoded: &str) -> String {
    encoded.to_string()
}

// ── Auto-start registry ───────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub fn set_auto_start(enabled: bool, exe_path: &str) -> Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };

    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "VoiceInput\0".encode_utf16().collect();

    unsafe {
        let mut hkey = windows::Win32::System::Registry::HKEY::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
        .ok()?;

        if enabled {
            let path: Vec<u16> = format!("{}\0", exe_path).encode_utf16().collect();
            RegSetValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    path.as_ptr() as *const u8,
                    path.len() * 2,
                )),
            )
            .ok()?;
        } else {
            let _ = RegDeleteValueW(hkey, PCWSTR(value_name.as_ptr()));
        }
        RegCloseKey(hkey).ok()?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn set_auto_start(_enabled: bool, _exe_path: &str) -> Result<()> {
    Ok(())
}
