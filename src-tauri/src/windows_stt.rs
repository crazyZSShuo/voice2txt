use anyhow::{bail, Result};
use tauri::AppHandle;

#[cfg(target_os = "windows")]
use tauri::Emitter;

#[cfg(target_os = "windows")]
use windows::Media::SpeechRecognition::SpeechRecognizer;

#[cfg(target_os = "windows")]
pub struct WindowsSpeechSession {
    recognizer: SpeechRecognizer,
}

#[cfg(not(target_os = "windows"))]
pub struct WindowsSpeechSession;

#[cfg(target_os = "windows")]
async fn create_recognizer() -> Result<SpeechRecognizer> {
    let recognizer = SpeechRecognizer::new()?;
    recognizer.CompileConstraintsAsync()?.await?;
    Ok(recognizer)
}

#[cfg(target_os = "windows")]
pub async fn start_recognition(_app: &AppHandle) -> Result<WindowsSpeechSession> {
    let recognizer = create_recognizer().await?;
    recognizer
        .ContinuousRecognitionSession()?
        .StartAsync()?
        .await?;
    Ok(WindowsSpeechSession { recognizer })
}

#[cfg(not(target_os = "windows"))]
pub async fn start_recognition(_app: &AppHandle) -> Result<WindowsSpeechSession> {
    bail!("Windows SpeechRecognizer is only available on Windows builds")
}

#[cfg(target_os = "windows")]
pub async fn stop_recognition(
    session: WindowsSpeechSession,
    app: &AppHandle,
) -> Result<String> {
    session
        .recognizer
        .ContinuousRecognitionSession()?
        .StopAsync()?
        .await?;
    let result = session.recognizer.RecognizeAsync()?.await?;
    let text = result.Text()?.to_string();
    if !text.is_empty() {
        let _ = app.emit("transcript-chunk", &text);
    }
    Ok(text)
}

#[cfg(not(target_os = "windows"))]
pub async fn stop_recognition(
    _session: WindowsSpeechSession,
    _app: &AppHandle,
) -> Result<String> {
    bail!("Windows SpeechRecognizer is only available on Windows builds")
}

#[cfg(target_os = "windows")]
pub async fn test_connection() -> Result<String> {
    let _recognizer = create_recognizer().await?;
    Ok("Windows SpeechRecognizer is available".to_string())
}

#[cfg(not(target_os = "windows"))]
pub async fn test_connection() -> Result<String> {
    bail!("Windows SpeechRecognizer is only available on Windows builds")
}
