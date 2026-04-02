use anyhow::{bail, Result};
use tauri::AppHandle;
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

#[cfg(target_os = "windows")]
use anyhow::{anyhow, Context};

#[cfg(target_os = "windows")]
use std::sync::{Arc, Mutex};

#[cfg(target_os = "windows")]
use tauri::Emitter;

#[cfg(target_os = "windows")]
use windows::{
    Foundation::{EventRegistrationToken, TypedEventHandler},
    Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs,
        SpeechContinuousRecognitionResultGeneratedEventArgs, SpeechContinuousRecognitionSession,
        SpeechRecognitionResultStatus, SpeechRecognizer,
    },
};

#[cfg(target_os = "windows")]
type CompletionSignal = Arc<Mutex<Option<oneshot::Sender<std::result::Result<(), String>>>>>;

const STOP_COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);
const SPEECH_PRIVACY_POLICY_ERROR_HEX: &str = "0x80045509";

#[cfg(target_os = "windows")]
pub struct WindowsSpeechSession {
    recognizer: SpeechRecognizer,
    recognition_session: SpeechContinuousRecognitionSession,
    result_generated_token: EventRegistrationToken,
    completed_token: EventRegistrationToken,
    transcript: Arc<Mutex<String>>,
    completion_rx: oneshot::Receiver<std::result::Result<(), String>>,
}

#[cfg(not(target_os = "windows"))]
pub struct WindowsSpeechSession;

#[cfg(target_os = "windows")]
fn speech_status_label(status: SpeechRecognitionResultStatus) -> &'static str {
    match status {
        SpeechRecognitionResultStatus::Success => "success",
        SpeechRecognitionResultStatus::TopicLanguageNotSupported => "topic_language_not_supported",
        SpeechRecognitionResultStatus::GrammarLanguageMismatch => "grammar_language_mismatch",
        SpeechRecognitionResultStatus::GrammarCompilationFailure => "grammar_compilation_failure",
        SpeechRecognitionResultStatus::AudioQualityFailure => "audio_quality_failure",
        SpeechRecognitionResultStatus::UserCanceled => "user_canceled",
        SpeechRecognitionResultStatus::Unknown => "unknown",
        SpeechRecognitionResultStatus::TimeoutExceeded => "timeout_exceeded",
        SpeechRecognitionResultStatus::PauseLimitExceeded => "pause_limit_exceeded",
        SpeechRecognitionResultStatus::NetworkFailure => "network_failure",
        SpeechRecognitionResultStatus::MicrophoneUnavailable => "microphone_unavailable",
        _ => "unrecognized_status",
    }
}

#[cfg(target_os = "windows")]
fn signal_completion(completion_tx: &CompletionSignal, outcome: std::result::Result<(), String>) {
    if let Some(tx) = completion_tx.lock().unwrap().take() {
        let _ = tx.send(outcome);
    }
}

async fn await_completion_signal(
    completion_rx: oneshot::Receiver<std::result::Result<(), String>>,
) -> Result<std::result::Result<(), String>> {
    timeout(STOP_COMPLETION_TIMEOUT, completion_rx)
        .await
        .map_err(|_| anyhow::anyhow!("Windows speech recognition stop timed out"))?
        .map_err(|_| anyhow::anyhow!("Windows speech completion signal was dropped"))
}

fn friendly_windows_startup_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("speech privacy policy was not accepted")
        || lower.contains(&SPEECH_PRIVACY_POLICY_ERROR_HEX.to_ascii_lowercase())
    {
        return "Windows SpeechRecognizer cannot start because the Windows speech privacy policy has not been accepted yet. Open Windows Settings > Privacy & security > Speech and turn on Online speech recognition, then retry.".to_string();
    }

    message.to_string()
}

#[cfg(target_os = "windows")]
async fn create_recognizer() -> Result<SpeechRecognizer> {
    let recognizer = SpeechRecognizer::new().context("create Windows SpeechRecognizer")?;
    let compilation = recognizer
        .CompileConstraintsAsync()
        .context("start compiling speech constraints")?
        .get()
        .context("wait for speech constraints compilation")?;
    let status = compilation
        .Status()
        .context("read speech constraints compilation status")?;

    if status != SpeechRecognitionResultStatus::Success {
        bail!(
            "Windows SpeechRecognizer constraint compilation failed: {}",
            speech_status_label(status)
        );
    }

    Ok(recognizer)
}

#[cfg(target_os = "windows")]
pub async fn start_recognition(app: &AppHandle) -> Result<WindowsSpeechSession> {
    let recognizer = create_recognizer().await?;
    let recognition_session = recognizer
        .ContinuousRecognitionSession()
        .context("create Windows continuous recognition session")?;
    let transcript = Arc::new(Mutex::new(String::new()));
    let (completion_tx, completion_rx) = oneshot::channel();
    let completion_signal = Arc::new(Mutex::new(Some(completion_tx)));

    let app_for_results = app.clone();
    let transcript_for_results = Arc::clone(&transcript);
    let result_generated_token = recognition_session
        .ResultGenerated(&TypedEventHandler::<
            SpeechContinuousRecognitionSession,
            SpeechContinuousRecognitionResultGeneratedEventArgs,
        >::new(move |_sender, args| {
            let Some(args) = args.as_ref() else {
                crate::diag::write("event:windows_stt:result_generated:missing_args");
                return Ok(());
            };

            let result = match args.Result() {
                Ok(result) => result,
                Err(err) => {
                    crate::diag::write(&format!(
                        "event:windows_stt:result_generated:error={}",
                        err
                    ));
                    return Ok(());
                }
            };

            let status = match result.Status() {
                Ok(status) => status,
                Err(err) => {
                    crate::diag::write(&format!("event:windows_stt:result_status:error={}", err));
                    return Ok(());
                }
            };

            if status != SpeechRecognitionResultStatus::Success {
                crate::diag::write(&format!(
                    "event:windows_stt:result_status:{}",
                    speech_status_label(status)
                ));
                return Ok(());
            }

            let text = match result.Text() {
                Ok(text) => text.to_string(),
                Err(err) => {
                    crate::diag::write(&format!("event:windows_stt:result_text:error={}", err));
                    return Ok(());
                }
            };
            let chunk = text.trim();

            if chunk.is_empty() {
                return Ok(());
            }

            {
                let mut full = transcript_for_results.lock().unwrap();
                if !full.is_empty() {
                    full.push(' ');
                }
                full.push_str(chunk);
            }

            let _ = app_for_results.emit("transcript-chunk", chunk.to_string());
            crate::diag::write(&format!("event:windows_stt:chunk:chars={}", chunk.len()));
            crate::diag::write_text("event:windows_stt:text", chunk);
            Ok(())
        }))
        .context("attach Windows speech ResultGenerated handler")?;

    let completion_signal_for_handler = Arc::clone(&completion_signal);
    let completed_token = recognition_session
        .Completed(&TypedEventHandler::<
            SpeechContinuousRecognitionSession,
            SpeechContinuousRecognitionCompletedEventArgs,
        >::new(move |_sender, args| {
            let Some(args) = args.as_ref() else {
                signal_completion(
                    &completion_signal_for_handler,
                    Err("Windows speech completion event did not include args".to_string()),
                );
                return Ok(());
            };

            let outcome = match args.Status() {
                Ok(SpeechRecognitionResultStatus::Success) => Ok(()),
                Ok(status) => Err(format!(
                    "Windows speech recognition completed with status: {}",
                    speech_status_label(status)
                )),
                Err(err) => Err(format!(
                    "Failed to read Windows speech completion status: {}",
                    err
                )),
            };

            signal_completion(&completion_signal_for_handler, outcome);
            Ok(())
        }))
        .context("attach Windows speech Completed handler")?;

    if let Err(err) = recognition_session
        .StartAsync()
        .context("start Windows continuous recognition")?
        .get()
        .context("wait for Windows continuous recognition startup")
    {
        let _ = recognition_session.RemoveResultGenerated(result_generated_token);
        let _ = recognition_session.RemoveCompleted(completed_token);
        let _ = recognizer.Close();
        return Err(anyhow!(friendly_windows_startup_error(&err.to_string())));
    }

    crate::diag::write("event:windows_stt:start");

    Ok(WindowsSpeechSession {
        recognizer,
        recognition_session,
        result_generated_token,
        completed_token,
        transcript,
        completion_rx,
    })
}

#[cfg(not(target_os = "windows"))]
pub async fn start_recognition(_app: &AppHandle) -> Result<WindowsSpeechSession> {
    bail!("Windows SpeechRecognizer is only available on Windows builds")
}

#[cfg(target_os = "windows")]
pub async fn stop_recognition(session: WindowsSpeechSession, _app: &AppHandle) -> Result<String> {
    let WindowsSpeechSession {
        recognizer,
        recognition_session,
        result_generated_token,
        completed_token,
        transcript,
        completion_rx,
    } = session;

    let stop_result = recognition_session
        .StopAsync()
        .context("begin stopping Windows speech recognition")?
        .get()
        .context("wait for Windows speech recognition stop");

    let completion_result = match stop_result {
        Ok(()) => await_completion_signal(completion_rx).await?,
        Err(err) => {
            let _ = recognition_session.RemoveResultGenerated(result_generated_token);
            let _ = recognition_session.RemoveCompleted(completed_token);
            let _ = recognizer.Close();
            return Err(err);
        }
    };

    let remove_result_generated = recognition_session.RemoveResultGenerated(result_generated_token);
    let remove_completed = recognition_session.RemoveCompleted(completed_token);
    let close_result = recognizer.Close();

    remove_result_generated.context("remove Windows speech ResultGenerated handler")?;
    remove_completed.context("remove Windows speech Completed handler")?;
    close_result.context("close Windows SpeechRecognizer")?;

    completion_result.map_err(|err| anyhow!(err))?;

    let transcript = transcript.lock().unwrap().trim().to_string();
    crate::diag::write(&format!(
        "event:windows_stt:stop:chars={}",
        transcript.len()
    ));
    Ok(transcript)
}

#[cfg(not(target_os = "windows"))]
pub async fn stop_recognition(_session: WindowsSpeechSession, _app: &AppHandle) -> Result<String> {
    bail!("Windows SpeechRecognizer is only available on Windows builds")
}

#[allow(dead_code)]
#[cfg(target_os = "windows")]
pub async fn test_connection() -> Result<String> {
    let recognizer = create_recognizer().await?;
    recognizer
        .Close()
        .context("close Windows SpeechRecognizer after test")?;
    Ok("Windows SpeechRecognizer is available".to_string())
}

#[allow(dead_code)]
#[cfg(not(target_os = "windows"))]
pub async fn test_connection() -> Result<String> {
    bail!("Windows SpeechRecognizer is only available on Windows builds")
}

#[cfg(test)]
mod tests {
    use super::{await_completion_signal, friendly_windows_startup_error};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn completion_signal_returns_success() {
        let (tx, rx) = oneshot::channel();
        tx.send(Ok(())).unwrap();

        let outcome = await_completion_signal(rx).await.unwrap();

        assert!(outcome.is_ok());
    }

    #[tokio::test]
    async fn completion_signal_returns_backend_error() {
        let (tx, rx) = oneshot::channel();
        tx.send(Err("backend failed".to_string())).unwrap();

        let outcome = await_completion_signal(rx).await.unwrap();

        assert_eq!(outcome.unwrap_err(), "backend failed");
    }

    #[tokio::test]
    async fn completion_signal_times_out_when_event_never_arrives() {
        let (_tx, rx) = oneshot::channel::<std::result::Result<(), String>>();

        let err = await_completion_signal(rx).await.unwrap_err();

        assert_eq!(err.to_string(), "Windows speech recognition stop timed out");
    }

    #[test]
    fn startup_error_maps_speech_privacy_policy_failure() {
        let message = "start Windows continuous recognition: The speech privacy policy was not accepted prior to attempting a speech recognition. (0x80045509)";

        let friendly = friendly_windows_startup_error(message);

        assert!(friendly.contains("Privacy & security > Speech"));
        assert!(friendly.contains("Online speech recognition"));
    }

    #[test]
    fn startup_error_leaves_other_messages_unchanged() {
        let message = "start Windows continuous recognition: Something else failed";

        assert_eq!(friendly_windows_startup_error(message), message);
    }
}
