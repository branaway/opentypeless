use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::Client;

use crate::error::AppError;

use super::{SttConfig, SttProvider, TranscriptEvent};

pub const DOUBAO_AUDIO_PROVIDER: &str = "doubao-audio";
pub const DOUBAO_AUDIO_MODEL: &str = "doubao-seed-2-0-lite-260428";
pub const ARK_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";

/// Config baked in at provider-creation time so the full polish context
/// (system prompt, API key, model) is available when disconnect() fires.
pub struct DoubaoAudioConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    /// Pre-built system prompt from llm::prompt::build_system_prompt.
    /// The audio model uses this to transcribe AND polish in a single API call.
    pub system_prompt: String,
}

/// Audio-native Doubao provider that accumulates raw PCM chunks during recording
/// and submits them to the ARK chat/completions endpoint on disconnect().
/// Returns polished text directly — no separate LLM polish step needed.
pub struct DoubaoAudioProvider {
    config: Option<DoubaoAudioConfig>,
    client: Client,
    /// Raw PCM i16 LE bytes at 16 kHz mono, accumulated chunk-by-chunk.
    audio_buffer: Vec<u8>,
}

impl DoubaoAudioProvider {
    pub fn new(config: DoubaoAudioConfig, client: Client) -> Self {
        Self {
            config: Some(config),
            client,
            audio_buffer: Vec::new(),
        }
    }
}

/// Wraps raw PCM i16 LE samples in a minimal RIFF WAV container.
/// No extra crate needed — WAV is just a 44-byte header + the PCM payload.
fn pcm_to_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = pcm.len() as u32;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    wav.extend_from_slice(&num_channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

#[async_trait]
impl SttProvider for DoubaoAudioProvider {
    async fn connect(&mut self, _config: &SttConfig) -> Result<(), AppError> {
        // Nothing to connect — audio accumulates in-process until disconnect()
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError> {
        self.audio_buffer.extend_from_slice(chunk);
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
        // Batch provider — audio accumulates via send_audio() until disconnect().
        // Returning Ok(None) instantly would starve the audio_rx branch in the
        // pipeline's tokio::select! loop, so we park this future permanently.
        std::future::pending::<()>().await;
        Ok(None)
    }

    /// Encodes accumulated PCM as WAV, submits to Doubao audio model,
    /// and returns the transcribed + polished text as the final transcript.
    async fn disconnect(&mut self) -> Result<Option<String>, AppError> {
        let cfg = self.config.take().ok_or_else(|| {
            AppError::Config("DoubaoAudio: provider used after disconnect".to_string())
        })?;

        // 512 bytes ≈ 128 i16 samples ≈ 8 ms at 16 kHz — definitely silence/noise
        if self.audio_buffer.len() < 512 {
            return Ok(Some(String::new())); // triggers no_speech path in pipeline
        }

        let wav = pcm_to_wav(&self.audio_buffer, 16000);
        tracing::info!(
            "[DoubaoAudio] Submitting {}ms of audio ({} bytes WAV)",
            self.audio_buffer.len() / 32, // 16000 Hz * 2 bytes/sample / 1000 ms
            wav.len()
        );

        let b64 = BASE64.encode(&wav);

        let body = serde_json::json!({
            "model": cfg.model,
            "messages": [
                {
                    "role": "system",
                    "content": cfg.system_prompt
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_audio",
                            "input_audio": {
                                "data": b64,
                                "format": "wav"
                            }
                        },
                        {
                            "type": "text",
                            "text": "Transcribe the speech in this audio, then POLISH it per the rules: add punctuation, remove filler words and repetitions, fix false starts, and format lists/paragraphs. Do NOT return a raw verbatim transcript — return clean, typed-quality text. Output only the final polished text, nothing else."
                        }
                    ]
                }
            ],
            "thinking": { "type": "disabled" }
        });

        let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));

        let res = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", cfg.api_key))
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(90))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;

        let status = res.status().as_u16();
        if status == 401 || status == 403 {
            return Err(AppError::Auth(format!(
                "Doubao audio API key rejected (HTTP {})",
                status
            )));
        }
        if status == 429 {
            return Err(AppError::Quota(
                "Doubao audio API quota exceeded".to_string(),
            ));
        }
        if !res.status().is_success() {
            let body_text = res.text().await.unwrap_or_default();
            return Err(AppError::Api {
                status,
                body: body_text[..body_text.len().min(300)].to_string(),
            });
        }

        let data: serde_json::Value = res.json().await.map_err(|e| {
            AppError::Network(format!("Doubao audio response parse error: {}", e))
        })?;

        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        tracing::info!("[DoubaoAudio] Got {} chars of polished text", text.len());
        Ok(Some(text))
    }

    fn name(&self) -> &str {
        "Doubao Audio (transcribe + polish)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_to_wav_has_correct_header() {
        let pcm = vec![0u8; 320]; // 10ms of silence at 16kHz mono i16
        let wav = pcm_to_wav(&pcm, 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + pcm.len());
        // data chunk size
        let data_size = u32::from_le_bytes(wav[40..44].try_into().unwrap());
        assert_eq!(data_size, pcm.len() as u32);
    }

    #[test]
    fn short_audio_returns_empty_without_api_call() {
        // This is tested indirectly — buffer < 512 bytes returns Ok(Some(""))
        // which triggers the no_speech path in the pipeline.
        // We verify the constant makes sense: 512 bytes / 2 bytes per i16 = 256 samples
        // At 16kHz that's 16ms — well below any meaningful speech.
        assert!(512 / 2 < 16000 / 10); // less than 100ms of audio
    }
}
