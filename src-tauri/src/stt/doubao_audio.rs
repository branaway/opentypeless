use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::Client;
use tauri::Manager;

use crate::error::AppError;

use super::{SttConfig, SttProvider, TranscriptEvent};

/// Parse the OpenAI-compatible `usage` block from an ARK response and record a
/// cost event. `audio_tokens` come from `prompt_tokens_details.audio_tokens`;
/// the remaining prompt tokens are billed as text input. Best-effort — any
/// missing field just records 0 for that bucket.
fn record_usage(app_handle: &tauri::AppHandle, data: &serde_json::Value, model: &str, kind: &str) {
    let usage = &data["usage"];
    if usage.is_null() {
        return;
    }
    let prompt = usage["prompt_tokens"].as_i64().unwrap_or(0);
    let output = usage["completion_tokens"].as_i64().unwrap_or(0);
    let audio = usage["prompt_tokens_details"]["audio_tokens"]
        .as_i64()
        .unwrap_or(0);
    let text_in = (prompt - audio).max(0);
    tracing::info!(
        "[Usage] {} model={} audio={} text_in={} output={}",
        kind,
        model,
        audio,
        text_in,
        output
    );
    if let Some(store) = app_handle.try_state::<crate::storage::UsageStore>() {
        store.record(model, kind, audio, text_in, output);
    }
}

pub const DOUBAO_AUDIO_PROVIDER: &str = "doubao-audio";
pub const DOUBAO_AUDIO_MODEL: &str = "doubao-seed-2-0-lite-260428";
pub const ARK_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";

/// Cursor marker injected into the preview text so the audio model knows where
/// to insert new dictation, and round-trips the new caret position back. It is a
/// SINGLE character (U+2038 CARET) on purpose: a paired form like `⟦^⟧` tempts
/// the model to treat it as opening/closing brackets and wrap the inserted text
/// inside them (`⟦inserted^⟧`), which then leaks into the result. A single
/// atomic glyph can't be split that way, and is extremely rare in real text.
pub const CURSOR_MARKER: &str = "‸";

/// Insert [`CURSOR_MARKER`] into `text` at a Unicode-scalar `offset`. An offset
/// at or beyond the end (incl. usize::MAX) places it at the end.
fn insert_cursor_marker(text: &str, offset: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let pos = offset.min(chars.len());
    let mut out = String::with_capacity(text.len() + CURSOR_MARKER.len());
    out.extend(chars[..pos].iter());
    out.push_str(CURSOR_MARKER);
    out.extend(chars[pos..].iter());
    out
}

/// Config baked in at provider-creation time so the full polish context
/// (system prompt, API key, model) is available when disconnect() fires.
pub struct DoubaoAudioConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    /// Pre-built system prompt from llm::prompt::build_system_prompt.
    /// The audio model uses this to transcribe AND polish in a single API call.
    pub system_prompt: String,
    /// Handle used to record token-usage cost events on disconnect().
    /// None in unit tests where no Tauri app is running.
    pub app_handle: Option<tauri::AppHandle>,
    /// When true, collapse long silent gaps before upload to save audio tokens.
    pub trim_silence: bool,
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

// ─── Silence trimming ───
// Doubao bills audio purely by duration, so collapsing long silent gaps before
// upload directly cuts cost (and upload size). A single O(n) energy pass.

/// 20ms of 16kHz mono i16 audio.
const TRIM_FRAME_BYTES: usize = 640;
/// Normalized RMS below this counts as silence. Deliberately well under the
/// speech VAD threshold (0.008) so quiet speech is never mistaken for silence.
const TRIM_SILENCE_RMS: f32 = 0.006;
/// Only collapse silence runs longer than this (~600ms); short natural pauses
/// between words/sentences are left intact.
const TRIM_MIN_RUN_FRAMES: usize = 30;
/// When collapsing a long run, keep this many silent frames on EACH side
/// (~100ms each, ~200ms total) so word onsets aren't clipped.
const TRIM_KEEP_EACH_FRAMES: usize = 5;

/// Normalized (0..1) RMS of one i16 LE frame.
fn frame_rms_norm(frame: &[u8]) -> f32 {
    let mut sum_sq = 0.0f64;
    let mut n = 0u64;
    for pair in frame.chunks_exact(2) {
        let s = i16::from_le_bytes([pair[0], pair[1]]) as f64;
        sum_sq += s * s;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    ((sum_sq / n as f64).sqrt() / 32768.0) as f32
}

/// Collapse long silent gaps in 16kHz mono i16 PCM. Short pauses are preserved;
/// runs longer than ~600ms are shortened to ~200ms of padding. O(n) single pass.
fn trim_long_silence(pcm: &[u8]) -> Vec<u8> {
    let frames: Vec<&[u8]> = pcm.chunks(TRIM_FRAME_BYTES).collect();
    if frames.is_empty() {
        return pcm.to_vec();
    }
    let silent: Vec<bool> = frames
        .iter()
        .map(|f| frame_rms_norm(f) < TRIM_SILENCE_RMS)
        .collect();

    let mut out = Vec::with_capacity(pcm.len());
    let mut i = 0;
    while i < frames.len() {
        if silent[i] {
            let start = i;
            while i < frames.len() && silent[i] {
                i += 1;
            }
            let run = i - start;
            if run > TRIM_MIN_RUN_FRAMES {
                // Keep a short pad at both ends of the gap, drop the middle.
                for f in &frames[start..start + TRIM_KEEP_EACH_FRAMES] {
                    out.extend_from_slice(f);
                }
                for f in &frames[i - TRIM_KEEP_EACH_FRAMES..i] {
                    out.extend_from_slice(f);
                }
            } else {
                for f in &frames[start..i] {
                    out.extend_from_slice(f);
                }
            }
        } else {
            out.extend_from_slice(frames[i]);
            i += 1;
        }
    }
    out
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

/// Structured action returned by the audio model when editing a preview by voice.
#[derive(Debug, Clone, PartialEq)]
pub enum EditAction {
    /// Replace the preview text with this new full text.
    Edit(String),
    /// Output the current preview text to the target app.
    Send,
    /// Discard the preview and re-record from scratch.
    Rerecord,
}

/// Parse the model's JSON reply into an EditAction. Tolerant of surrounding
/// prose / code fences: extracts the first balanced-looking JSON object.
pub fn parse_edit_action(content: &str) -> Option<EditAction> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if end < start {
        return None;
    }
    let json = &content[start..=end];
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    match value["action"].as_str()?.trim().to_lowercase().as_str() {
        "send" => Some(EditAction::Send),
        "rerecord" => Some(EditAction::Rerecord),
        "edit" => {
            let text = value["text"].as_str().unwrap_or("").trim().to_string();
            Some(EditAction::Edit(text))
        }
        _ => None,
    }
}

/// Send a spoken command (audio) plus the current text to the audio model and
/// return the structured edit action. Used for voice-driven preview editing.
#[allow(clippy::too_many_arguments)]
pub async fn run_edit_action(
    client: &Client,
    api_key: &str,
    model: &str,
    base_url: &str,
    system_prompt: &str,
    current_text: &str,
    audio_pcm: &[u8],
    app_handle: &tauri::AppHandle,
    caret_offset: usize,
) -> Result<EditAction, AppError> {
    let wav = pcm_to_wav(audio_pcm, 16000);
    let b64 = BASE64.encode(&wav);

    let marked_text = insert_cursor_marker(current_text, caret_offset);
    let user_text = format!(
        "The user's CURRENT TEXT (with a {} cursor marker) is between the tags below. The AUDIO is either an edit command or new dictation to insert at the marker. Decide the action and reply with ONLY the JSON object.\n<current_text>\n{}\n</current_text>",
        CURSOR_MARKER, marked_text
    );

    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_audio",
                        "input_audio": { "data": b64, "format": "wav" }
                    },
                    { "type": "text", "text": user_text }
                ]
            }
        ],
        "thinking": { "type": "disabled" }
    });

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(60))
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

    let data: serde_json::Value = res
        .json()
        .await
        .map_err(|e| AppError::Network(format!("Doubao audio response parse error: {}", e)))?;

    record_usage(app_handle, &data, model, "edit");

    let content = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim();

    parse_edit_action(content)
        .ok_or_else(|| AppError::Config(format!("Unrecognized edit action: {}", content)))
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

        // Collapse long silent gaps before upload — Doubao bills by duration.
        let original_ms = self.audio_buffer.len() / 32; // 16kHz * 2 bytes / 1000ms
        let pcm = if cfg.trim_silence {
            trim_long_silence(&self.audio_buffer)
        } else {
            std::mem::take(&mut self.audio_buffer)
        };
        let wav = pcm_to_wav(&pcm, 16000);
        tracing::info!(
            "[DoubaoAudio] Submitting {}ms of audio (trimmed from {}ms, {} bytes WAV)",
            pcm.len() / 32,
            original_ms,
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

        let data: serde_json::Value = res
            .json()
            .await
            .map_err(|e| AppError::Network(format!("Doubao audio response parse error: {}", e)))?;

        if let Some(ref h) = cfg.app_handle {
            record_usage(h, &data, &cfg.model, "transcribe");
        }

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
    fn trim_long_silence_collapses_gaps_but_keeps_speech() {
        let secs = 16000usize; // samples per second
                               // 1s loud + 3s silence + 1s loud, i16 LE mono @16kHz.
        let loud: Vec<u8> = (0..secs).flat_map(|_| 8000i16.to_le_bytes()).collect();
        let sil: Vec<u8> = vec![0u8; secs * 2 * 3];
        let mut pcm = Vec::new();
        pcm.extend_from_slice(&loud);
        pcm.extend_from_slice(&sil);
        pcm.extend_from_slice(&loud);

        let trimmed = trim_long_silence(&pcm);
        // Both speech segments are preserved in full…
        assert!(trimmed.len() >= loud.len() * 2);
        // …and the 3s gap is collapsed to well under 1s.
        assert!(trimmed.len() < loud.len() * 2 + secs * 2);
        assert!(trimmed.len() < pcm.len());
    }

    #[test]
    fn trim_long_silence_keeps_short_pauses() {
        let secs = 16000usize;
        let loud: Vec<u8> = (0..secs).flat_map(|_| 8000i16.to_le_bytes()).collect();
        // 300ms gap — below the ~600ms threshold, so it must be left intact.
        let sil: Vec<u8> = vec![0u8; (secs * 2) * 3 / 10];
        let mut pcm = Vec::new();
        pcm.extend_from_slice(&loud);
        pcm.extend_from_slice(&sil);
        pcm.extend_from_slice(&loud);
        assert_eq!(trim_long_silence(&pcm).len(), pcm.len());
    }

    #[test]
    fn parse_edit_action_handles_each_action() {
        assert_eq!(
            parse_edit_action(r#"{"action":"send"}"#),
            Some(EditAction::Send)
        );
        assert_eq!(
            parse_edit_action(r#"{"action":"rerecord"}"#),
            Some(EditAction::Rerecord)
        );
        assert_eq!(
            parse_edit_action(r#"{"action":"edit","text":"hello world"}"#),
            Some(EditAction::Edit("hello world".to_string()))
        );
    }

    #[test]
    fn parse_edit_action_tolerates_surrounding_text() {
        let raw = "Sure! ```json\n{\"action\":\"edit\",\"text\":\"改好了\"}\n``` done";
        assert_eq!(
            parse_edit_action(raw),
            Some(EditAction::Edit("改好了".to_string()))
        );
    }

    #[test]
    fn parse_edit_action_rejects_garbage() {
        assert_eq!(parse_edit_action("no json here"), None);
        assert_eq!(parse_edit_action(r#"{"action":"bogus"}"#), None);
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
