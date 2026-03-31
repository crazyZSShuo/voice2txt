/// audio.rs — Microphone capture via cpal/WASAPI
/// Emits `rms-level` (f32 0.0–1.0) each audio frame.
/// Accumulates PCM samples for STT after recording ends.
use crate::diag;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, SupportedStreamConfig};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub struct AudioCapture {
    pub samples: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    input_sample_rate: u32,
    input_channels: u16,
}

// cpal::Stream contains a *mut () (WASAPI internals) which is not Send by default.
// We always access it through a Mutex and only from the recording control thread,
// so this is safe in practice.
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            samples: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            input_sample_rate: 16_000,
            input_channels: 1,
        }
    }

    /// Start capturing. Emits `rms-level` events in real time.
    pub fn start(&mut self, app: AppHandle) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No input device found")?;
        let supported = device
            .default_input_config()
            .context("No default input config found")?;

        let config = preferred_stream_config(&supported);
        self.input_sample_rate = config.sample_rate.0;
        self.input_channels = config.channels;

        self.samples.lock().unwrap().clear();
        let channels = config.channels as usize;

        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                let samples = Arc::clone(&self.samples);
                let app_clone = app.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| process_samples(data, channels, &samples, &app_clone),
                    |err| log::error!("Audio stream error: {}", err),
                    None,
                )?
            }
            SampleFormat::I16 => {
                let samples = Arc::clone(&self.samples);
                let app_clone = app.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let floats: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        process_samples(&floats, channels, &samples, &app_clone);
                    },
                    |err| log::error!("Audio stream error: {}", err),
                    None,
                )?
            }
            SampleFormat::U16 => {
                let samples = Arc::clone(&self.samples);
                let app_clone = app.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        let floats: Vec<f32> = data
                            .iter()
                            .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                            .collect();
                        process_samples(&floats, channels, &samples, &app_clone);
                    },
                    |err| log::error!("Audio stream error: {}", err),
                    None,
                )?
            }
            other => anyhow::bail!("Unsupported input sample format: {:?}", other),
        };

        stream.play()?;
        self.stream = Some(stream);
        log::info!(
            "Audio capture started: {} Hz, {} channel(s)",
            self.input_sample_rate,
            self.input_channels
        );
        Ok(())
    }

    /// Stop capturing. Returns accumulated PCM (16 kHz mono f32).
    pub fn stop(&mut self) -> Vec<f32> {
        drop(self.stream.take());
        let samples = self.samples.lock().unwrap().clone();
        let resampled = resample_linear(&samples, self.input_sample_rate, 16_000);
        log::info!(
            "Audio capture stopped. {} mono samples at {} Hz -> {} samples at 16000 Hz",
            samples.len(),
            self.input_sample_rate,
            resampled.len()
        );
        resampled
    }
}

fn preferred_stream_config(supported: &SupportedStreamConfig) -> StreamConfig {
    StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    }
}

fn process_samples(
    data: &[f32],
    channels: usize,
    samples_arc: &Arc<Mutex<Vec<f32>>>,
    app: &AppHandle,
) {
    static RMS_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let mono = downmix_interleaved_to_mono(data, channels);
    samples_arc.lock().unwrap().extend_from_slice(&mono);
    let rms = if mono.is_empty() {
        0.0
    } else {
        let sq: f32 = mono.iter().map(|&x| x * x).sum();
        (sq / mono.len() as f32).sqrt()
    };
    let level = (rms * 8.0_f32).min(1.0);
    if level > 0.02 && !RMS_LOGGED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        diag::write(&format!("audio:rms-first:{:.4}", level));
    }
    if let Some(win) = app.get_webview_window("capsule") {
        let _ = win.emit("rms-level", level);
    }
    let _ = app.emit("rms-level", level);
}

fn downmix_interleaved_to_mono(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }

    data.chunks(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn resample_linear(data: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if data.is_empty() || from_rate == 0 || to_rate == 0 || from_rate == to_rate {
        return data.to_vec();
    }

    let out_len = ((data.len() as u64 * to_rate as u64) / from_rate as u64).max(1) as usize;
    let step = from_rate as f32 / to_rate as f32;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src = i as f32 * step;
        let left = src.floor() as usize;
        let right = (left + 1).min(data.len().saturating_sub(1));
        let frac = src - left as f32;
        let sample = data[left] * (1.0 - frac) + data[right] * frac;
        out.push(sample);
    }
    out
}

/// Convert f32 PCM to 16-bit WAV bytes (16 kHz, mono).
pub fn pcm_to_wav(samples: &[f32]) -> Vec<u8> {
    let sample_rate: u32 = 16_000;
    let channels: u16 = 1;
    let bits: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * bits as u32 / 8;
    let block_align = channels * bits / 8;
    let data_len = (samples.len() * 2) as u32;

    let mut buf = Vec::with_capacity(44 + samples.len() * 2);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::{downmix_interleaved_to_mono, resample_linear};

    #[test]
    fn downmixes_stereo_interleaved_audio_to_mono() {
        let mono = downmix_interleaved_to_mono(&[1.0, -1.0, 0.25, 0.75], 2);
        assert_eq!(mono, vec![0.0, 0.5]);
    }

    #[test]
    fn resample_is_identity_when_sample_rate_matches() {
        let data = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&data, 16_000, 16_000), data);
    }

    #[test]
    fn resamples_linearly_to_target_rate() {
        let resampled = resample_linear(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], 6, 3);
        assert_eq!(resampled, vec![0.0, 2.0, 4.0]);
    }
}
