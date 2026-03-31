/// llm.rs — LLM-based conservative transcript correction
/// Calls /v1/chat/completions with stream:true; emits `llm-chunk` events.
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::config::LlmConfig;

const SYSTEM_PROMPT: &str = r#"You are a speech recognition post-processor. Your ONLY job is to fix obvious automatic speech recognition (ASR) errors in Chinese/English mixed transcripts.

Rules (strictly follow ALL of them):
1. ONLY fix clear ASR errors, especially:
   - Chinese homophones used instead of intended words (e.g., "配森" → "Python", "杰森" → "JSON", "歌图恩" → "GetToken", "锐克特" → "React", "瑞迪斯" → "Redis", "毕修普" → "Bishop")
   - English technical terms phonetically transcribed as Chinese characters
   - Obvious punctuation errors that change meaning
2. NEVER paraphrase, rephrase, rewrite, expand, or add content.
3. NEVER delete any content that appears intentional.
4. NEVER change grammar, style, or word choice if the original is already correct.
5. If the input is already correct, return it EXACTLY as-is — character for character.
6. Return ONLY the corrected text, no explanations, no quotes, no markdown.
"#;

/// Refine transcript using LLM. Returns corrected text via streaming.
/// Emits `llm-chunk` events with each token delta.
pub async fn refine_transcript(raw_text: &str, cfg: &LlmConfig, app: &AppHandle) -> Result<String> {
    if !cfg.enabled || cfg.api_key.is_empty() {
        return Ok(raw_text.to_string());
    }

    let url = format!("{}/v1/chat/completions", cfg.base_url.trim_end_matches('/'));

    let body = json!({
        "model": cfg.model,
        "stream": true,
        "temperature": 0.0,
        "max_tokens": 2048,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": raw_text }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .context("LLM request failed")?;

    let status = response.status();
    if !status.is_success() {
        let err = response.text().await.unwrap_or_default();
        bail!("LLM API error {}: {}", status, err);
    }

    let mut full_text = String::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read LLM stream")?;
        let raw = String::from_utf8_lossy(&chunk);

        for line in raw.lines() {
            let line = line.trim();
            if !line.starts_with("data: ") {
                continue;
            }
            let data = &line["data: ".len()..];
            if data == "[DONE]" {
                break;
            }
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                // Standard OpenAI stream delta
                if let Some(delta) = json
                    .pointer("/choices/0/delta/content")
                    .and_then(|v| v.as_str())
                {
                    if !delta.is_empty() {
                        full_text.push_str(delta);
                        let _ = app.emit("llm-chunk", delta);
                    }
                }
            }
        }
    }

    if full_text.is_empty() {
        Ok(raw_text.to_string())
    } else {
        Ok(full_text)
    }
}

/// Connectivity test for LLM API.
pub async fn test_connection(cfg: &LlmConfig) -> Result<String> {
    let url = format!("{}/v1/chat/completions", cfg.base_url.trim_end_matches('/'));

    let body = json!({
        "model": cfg.model,
        "max_tokens": 5,
        "messages": [
            { "role": "user", "content": "ping" }
        ]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .context("LLM connection test failed")?;

    let status = resp.status();
    if status.is_success() {
        Ok(format!("Connected (HTTP {})", status))
    } else {
        let body = resp.text().await.unwrap_or_default();
        bail!("HTTP {}: {}", status, body)
    }
}
