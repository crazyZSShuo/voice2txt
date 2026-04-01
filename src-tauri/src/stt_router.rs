use anyhow::Result;

#[cfg(not(target_os = "windows"))]
use anyhow::bail;

use crate::config::{AppConfig, SttBackend};

pub fn selected_backend(cfg: &AppConfig) -> SttBackend {
    cfg.stt_backend
}

pub fn backend_uses_local_audio_capture(backend: SttBackend) -> bool {
    matches!(backend, SttBackend::Custom)
}

pub fn ensure_backend_supported(_backend: SttBackend) -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    if _backend == SttBackend::WindowsSpeech {
        bail!("Windows SpeechRecognizer is only available on Windows builds");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, SttBackend};

    #[test]
    fn selects_custom_backend_when_requested() {
        let mut cfg = AppConfig::default();
        cfg.stt_backend = SttBackend::Custom;

        assert_eq!(selected_backend(&cfg), SttBackend::Custom);
    }

    #[test]
    fn windows_backend_does_not_use_local_audio_capture() {
        assert!(backend_uses_local_audio_capture(SttBackend::Custom));
        assert!(!backend_uses_local_audio_capture(SttBackend::WindowsSpeech));
    }

    #[test]
    fn non_windows_rejects_windows_backend() {
        #[cfg(not(target_os = "windows"))]
        {
            let err = ensure_backend_supported(SttBackend::WindowsSpeech).unwrap_err();
            assert!(err.to_string().contains("Windows SpeechRecognizer"));
        }
    }
}
