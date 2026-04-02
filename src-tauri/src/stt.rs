/// stt.rs — Speech-to-text via OpenAI-compatible /v1/audio/transcriptions
/// Sends WAV data as multipart/form-data. Supports streaming response chunks.
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::multipart;
use tauri::{AppHandle, Emitter};

use crate::config::SttConfig;

/// Transcribe audio bytes (WAV) and emit `transcript-chunk` events as text arrives.
/// Returns the complete transcript.
pub async fn transcribe(
    wav_bytes: Vec<u8>,
    cfg: &SttConfig,
    language: &str,
    app: &AppHandle,
) -> Result<String> {
    if cfg.api_key.is_empty() {
        bail!("STT API key is not configured");
    }

    let url = format!(
        "{}/v1/audio/transcriptions",
        cfg.base_url.trim_end_matches('/')
    );

    let file_part = multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;

    let form = multipart::Form::new()
        .part("file", file_part)
        .text("model", cfg.model.clone())
        .text("language", language.to_string())
        .text("response_format", "verbose_json");

    let client = reqwest::Client::new();
    let request = client
        .post(&url)
        .bearer_auth(&cfg.api_key);

    let response = request
        .multipart(form)
        .send()
        .await
        .context("STT request failed")?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        bail!("STT API error {}: {}", status, err_body);
    }

    // Parse verbose_json response
    let body = response.text().await.context("read STT response body")?;
    let json: serde_json::Value = serde_json::from_str(&body).context("parse STT JSON")?;

    let text = json["text"].as_str().unwrap_or("").trim().to_string();

    // Emit the full transcription as one chunk
    // (For streaming-capable APIs, this could be emitted word-by-word)
    if !text.is_empty() {
        let _ = app.emit("transcript-chunk", &text);
        crate::diag::write_text("event:stt:text", &text);
    }

    Ok(text)
}

/// Streaming transcription for APIs that return text/event-stream.
/// Falls back to `transcribe` for JSON responses.
pub async fn transcribe_streaming(
    wav_bytes: Vec<u8>,
    cfg: &SttConfig,
    language: &str,
    app: &AppHandle,
) -> Result<String> {
    if cfg.api_key.is_empty() {
        bail!("STT API key is not configured");
    }

    let url = format!(
        "{}/v1/audio/transcriptions",
        cfg.base_url.trim_end_matches('/')
    );

    // Clone wav_bytes before consuming it in the form, so we can fall back to non-streaming
    let wav_backup = wav_bytes.clone();

    let file_part = multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;

    let form = multipart::Form::new()
        .part("file", file_part)
        .text("model", cfg.model.clone())
        .text("language", language.to_string())
        .text("response_format", "text")
        .text("stream", "true");

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .multipart(form)
        .send()
        .await
        .context("STT streaming request failed")?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        // Fall back to non-streaming if streaming not supported
        if status.as_u16() == 400 && err_body.contains("stream") {
            log::warn!("Streaming not supported, falling back to non-streaming STT");
            return transcribe(wav_backup, cfg, language, app).await;
        }
        bail!("STT API error {}: {}", status, err_body);
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // If not streaming, parse as JSON
    if !content_type.contains("event-stream") && !content_type.contains("octet-stream") {
        let body = response.text().await?;
        // Try verbose JSON
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
            let text = json["text"].as_str().unwrap_or(&body).trim().to_string();
            if !text.is_empty() {
                let _ = app.emit("transcript-chunk", &text);
                crate::diag::write_text("event:stt:text", &text);
            }
            return Ok(text);
        }
        let text = body.trim().to_string();
        if !text.is_empty() {
            let _ = app.emit("transcript-chunk", &text);
            crate::diag::write_text("event:stt:text", &text);
        }
        return Ok(text);
    }

    // SSE streaming
    let mut full_text = String::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read STT stream chunk")?;
        let raw = String::from_utf8_lossy(&chunk);

        for line in raw.lines() {
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if data == "[DONE]" {
                    break;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = json["delta"].as_str() {
                        full_text.push_str(delta);
                        let _ = app.emit("transcript-chunk", delta);
                        crate::diag::write_text("event:stt:chunk", delta);
                    }
                }
            }
        }
    }

    Ok(full_text)
}

/// Quick connectivity test — sends a tiny silent WAV.
pub async fn test_connection(cfg: &SttConfig) -> Result<String> {
    // 0.1s of silence at 16kHz
    let silence: Vec<f32> = vec![0.0f32; 1600];
    let wav = crate::audio::pcm_to_wav(&silence);

    let url = format!(
        "{}/v1/audio/transcriptions",
        cfg.base_url.trim_end_matches('/')
    );
    let file_part = multipart::Part::bytes(wav)
        .file_name("test.wav")
        .mime_str("audio/wav")?;
    let form = multipart::Form::new()
        .part("file", file_part)
        .text("model", cfg.model.clone())
        .text("language", "en");

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .multipart(form)
        .send()
        .await
        .context("connection test failed")?;

    let status = resp.status();
    if status.is_success() || status.as_u16() == 400 {
        // 400 may mean empty audio — but the API is reachable
        Ok(format!("Connected (HTTP {})", status))
    } else {
        bail!("HTTP {}", status)
    }
}
