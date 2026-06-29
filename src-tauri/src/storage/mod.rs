use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri_plugin_store::StoreExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub stt_provider: String,
    pub stt_api_key: String,
    pub stt_custom_api_key: String,
    pub stt_language: String,
    pub stt_custom_preset: String,
    pub stt_custom_base_url: String,
    pub stt_custom_model: String,
    pub stt_volcengine_resource_id: String,
    pub llm_provider: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub llm_base_url: String,
    pub polish_enabled: bool,
    pub polish_custom_prompt: String,
    pub polish_chinese_script: String,
    pub translate_enabled: bool,
    pub target_lang: String,
    pub hotkey: String,
    pub hotkey_mode: String,
    pub output_mode: String,
    pub selected_text_enabled: bool,
    pub theme: String,
    pub auto_start: bool,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub max_recording_seconds: u32,
    pub ui_language: String,
    pub capsule_auto_hide: bool,
    /// When true, the polished result is shown in an editable preview before
    /// being output to the target app, instead of being typed/pasted directly.
    pub preview_before_output: bool,
    /// When true, short audio cues play on pipeline state transitions
    /// (listening, transcribing, polishing, output).
    pub sound_effects_enabled: bool,
    /// When true, a single Enter is pressed after the text is output, so the
    /// result is submitted (e.g. runs in a terminal) without a manual keypress.
    pub output_append_enter: bool,
    /// When true, long silent gaps are collapsed before audio upload to cut
    /// audio-token cost (audio is billed purely by duration).
    pub trim_silence: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            stt_provider: crate::stt::doubao_audio::DOUBAO_AUDIO_PROVIDER.to_string(),
            stt_api_key: String::new(),
            stt_custom_api_key: String::new(),
            stt_language: "multi".to_string(),
            stt_custom_preset: crate::stt::config::CUSTOM_WHISPER_PRESET_SPEACHES.to_string(),
            stt_custom_base_url: crate::stt::config::DEFAULT_CUSTOM_WHISPER_BASE_URL.to_string(),
            stt_custom_model: crate::stt::config::DEFAULT_CUSTOM_WHISPER_MODEL.to_string(),
            stt_volcengine_resource_id: crate::stt::volcengine::VOLCENGINE_SEEDASR_RESOURCE_ID
                .to_string(),
            llm_provider: "doubao".to_string(),
            llm_api_key: String::new(),
            llm_model: crate::stt::doubao_audio::DOUBAO_AUDIO_MODEL.to_string(),
            llm_base_url: crate::stt::doubao_audio::ARK_BASE_URL.to_string(),
            polish_enabled: false,
            polish_custom_prompt: String::new(),
            polish_chinese_script: "preserve".to_string(),
            translate_enabled: false,
            target_lang: "en".to_string(),
            #[cfg(target_os = "macos")]
            hotkey: "Option+/".to_string(),
            #[cfg(not(target_os = "macos"))]
            hotkey: "Ctrl+/".to_string(),
            hotkey_mode: "toggle".to_string(),
            output_mode: "keyboard".to_string(),
            selected_text_enabled: false,
            theme: "system".to_string(),
            auto_start: false,
            close_to_tray: true,
            start_minimized: false,
            max_recording_seconds: 30,
            ui_language: "en".to_string(),
            capsule_auto_hide: false,
            preview_before_output: true,
            sound_effects_enabled: true,
            output_append_enter: true,
            trim_silence: true,
        }
    }
}

impl AppConfig {
    pub fn new_install_default() -> Self {
        Self {
            capsule_auto_hide: true,
            ..Self::default()
        }
    }

    fn normalize_platform_hotkey(&mut self) {
        #[cfg(target_os = "macos")]
        if self.hotkey == "Alt+/" {
            self.hotkey = "Option+/".to_string();
        }
    }

    fn normalize_values(&mut self) {
        self.polish_custom_prompt = sanitize_polish_custom_prompt(&self.polish_custom_prompt);
        self.polish_chinese_script = "preserve".to_string();
        self.normalize_platform_hotkey();
    }

    pub fn from_stored_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        let has_capsule_auto_hide = value
            .as_object()
            .is_some_and(|object| object.contains_key("capsule_auto_hide"));
        let mut config: Self = serde_json::from_value(value)?;
        if !has_capsule_auto_hide {
            config.capsule_auto_hide = false;
        }
        config.normalize_values();
        Ok(config)
    }
}

const POLISH_CUSTOM_PROMPT_MAX_CHARS: usize = 2000;

fn sanitize_polish_custom_prompt(value: &str) -> String {
    value
        .replace('\0', "")
        .trim()
        .chars()
        .take(POLISH_CUSTOM_PROMPT_MAX_CHARS)
        .collect()
}

// ─── ConfigManager (tauri-plugin-store backed) ───

pub struct ConfigManager {
    app_handle: tauri::AppHandle,
    cache: Mutex<Option<AppConfig>>,
}

impl ConfigManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            cache: Mutex::new(None),
        }
    }

    pub async fn load(&self) -> Result<AppConfig> {
        if let Some(config) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(config);
        }

        let config = match self.app_handle.store("settings.json") {
            Ok(store) => match store.get("app_config") {
                Some(val) => AppConfig::from_stored_value(val.clone())
                    .unwrap_or_else(|_| AppConfig::new_install_default()),
                None => AppConfig::new_install_default(),
            },
            Err(_) => AppConfig::new_install_default(),
        };

        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());
        Ok(config)
    }

    pub async fn save(&self, config: &AppConfig) -> Result<()> {
        let mut config = config.clone();
        config.normalize_values();
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());

        let store = self
            .app_handle
            .store("settings.json")
            .map_err(|e| anyhow::anyhow!("Failed to open store: {}", e))?;
        let val = serde_json::to_value(&config)?;
        store.set("app_config", val);
        store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(())
    }
}

// ─── HistoryStore (SQLite backed) ───

/// Maximum number of history entries to retain. Older entries are pruned on insert.
const MAX_HISTORY_ENTRIES: u32 = 5000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub app_name: String,
    pub app_type: String,
    pub raw_text: String,
    pub polished_text: String,
    pub language: Option<String>,
    pub duration_ms: Option<i64>,
}

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                app_name TEXT NOT NULL DEFAULT '',
                app_type TEXT NOT NULL DEFAULT '',
                raw_text TEXT NOT NULL DEFAULT '',
                polished_text TEXT NOT NULL DEFAULT '',
                language TEXT,
                duration_ms INTEGER
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub async fn add(&self, entry: HistoryEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO history (created_at, app_name, app_type, raw_text, polished_text, language, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                entry.created_at,
                entry.app_name,
                entry.app_type,
                entry.raw_text,
                entry.polished_text,
                entry.language,
                entry.duration_ms,
            ],
        )?;

        // Prune old entries beyond the retention limit
        conn.execute(
            "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
            rusqlite::params![MAX_HISTORY_ENTRIES],
        )?;

        Ok(())
    }

    pub async fn list(&self, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, created_at, app_name, app_type, raw_text, polished_text, language, duration_ms
             FROM history ORDER BY id DESC LIMIT ?1 OFFSET ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                created_at: row.get(1)?,
                app_name: row.get(2)?,
                app_type: row.get(3)?,
                raw_text: row.get(4)?,
                polished_text: row.get(5)?,
                language: row.get(6)?,
                duration_ms: row.get(7)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub async fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("DELETE FROM history", [])?;
        Ok(())
    }
}

// ─── UsageStore (SQLite backed) — token-usage cost auditing ───

/// Doubao pricing, RMB (元) per 1,000,000 tokens. Source: model pricing page
/// (推理输入 0.6 · 音频输入 9 · 推理输出 3.6, 输入 ≤32k). Cost is computed and
/// frozen into each row at record time, so historical totals stay correct even
/// if these prices change later.
const PRICE_AUDIO_IN_PER_M: f64 = 9.0;
const PRICE_TEXT_IN_PER_M: f64 = 0.6;
const PRICE_TEXT_OUT_PER_M: f64 = 3.6;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    /// Total spend (元) across all recorded events.
    pub total_cost: f64,
    /// Spend (元) since the start of the current local-calendar month.
    pub month_cost: f64,
    /// Spend (元) since local midnight today.
    pub today_cost: f64,
    pub total_calls: i64,
    pub audio_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// ISO 4217 code for the amounts above (always "CNY" for now).
    pub currency: String,
}

pub struct UsageStore {
    conn: Mutex<Connection>,
}

impl UsageStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                kind TEXT NOT NULL DEFAULT '',
                audio_tokens INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cost REAL NOT NULL DEFAULT 0
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Cost (元) for a token breakdown, using the fixed Doubao prices above.
    pub fn cost_for(audio_tokens: i64, input_tokens: i64, output_tokens: i64) -> f64 {
        audio_tokens as f64 * PRICE_AUDIO_IN_PER_M / 1_000_000.0
            + input_tokens as f64 * PRICE_TEXT_IN_PER_M / 1_000_000.0
            + output_tokens as f64 * PRICE_TEXT_OUT_PER_M / 1_000_000.0
    }

    /// Record one billable API call. Never fails the caller — a usage write must
    /// not disrupt the recording pipeline, so errors are logged and swallowed.
    pub fn record(&self, model: &str, kind: &str, audio: i64, input: i64, output: i64) {
        let cost = Self::cost_for(audio, input, output);
        let created_at = chrono::Local::now().timestamp_millis();
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = conn.execute(
            "INSERT INTO usage_events (created_at, model, kind, audio_tokens, input_tokens, output_tokens, cost)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![created_at, model, kind, audio, input, output, cost],
        ) {
            tracing::warn!("Failed to record usage event: {}", e);
        }
    }

    pub fn summary(&self) -> Result<UsageSummary> {
        use chrono::Datelike;
        let now = chrono::Local::now();
        let today = now.date_naive();
        let to_millis = |d: chrono::NaiveDate| -> i64 {
            d.and_hms_opt(0, 0, 0)
                .and_then(|ndt| ndt.and_local_timezone(chrono::Local).single())
                .map(|dt| dt.timestamp_millis())
                .unwrap_or(0)
        };
        let today_start = to_millis(today);
        let month_start = to_millis(today.with_day(1).unwrap_or(today));

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let sum_since = |since: i64| -> Result<f64> {
            Ok(conn.query_row(
                "SELECT COALESCE(SUM(cost), 0) FROM usage_events WHERE created_at >= ?1",
                rusqlite::params![since],
                |r| r.get(0),
            )?)
        };
        let total_cost: f64 =
            conn.query_row("SELECT COALESCE(SUM(cost), 0) FROM usage_events", [], |r| {
                r.get(0)
            })?;
        let (total_calls, audio_tokens, input_tokens, output_tokens): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(audio_tokens), 0), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0) FROM usage_events",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;

        Ok(UsageSummary {
            total_cost,
            month_cost: sum_since(month_start)?,
            today_cost: sum_since(today_start)?,
            total_calls,
            audio_tokens,
            input_tokens,
            output_tokens,
            currency: "CNY".to_string(),
        })
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("DELETE FROM usage_events", [])?;
        Ok(())
    }
}

// ─── DictionaryStore (SQLite backed) ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub id: i64,
    pub word: String,
    pub pronunciation: Option<String>,
}

pub struct DictionaryStore {
    conn: Mutex<Connection>,
}

impl DictionaryStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dictionary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                word TEXT NOT NULL,
                pronunciation TEXT
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub async fn add(&self, word: &str, pronunciation: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO dictionary (word, pronunciation) VALUES (?1, ?2)",
            rusqlite::params![word, pronunciation],
        )?;
        Ok(())
    }

    pub async fn remove(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "DELETE FROM dictionary WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<DictionaryEntry>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT id, word, pronunciation FROM dictionary")?;
        let rows = stmt.query_map([], |row| {
            Ok(DictionaryEntry {
                id: row.get(0)?,
                word: row.get(1)?,
                pronunciation: row.get(2)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub async fn words(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = match conn.prepare("SELECT word FROM dictionary") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_defaults_missing_custom_stt_api_key() {
        let value = serde_json::json!({
            "stt_provider": "deepgram",
            "stt_api_key": "hosted-secret"
        });

        let config: AppConfig = serde_json::from_value(value).unwrap();

        assert_eq!(config.stt_provider, "deepgram");
        assert_eq!(config.stt_api_key, "hosted-secret");
        assert_eq!(config.stt_custom_api_key, "");
        assert_eq!(
            config.stt_volcengine_resource_id,
            crate::stt::volcengine::VOLCENGINE_SEEDASR_RESOURCE_ID
        );
    }

    #[test]
    fn app_config_defaults_missing_polish_preferences() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "stt_api_key": "",
            "stt_language": "multi",
            "llm_provider": "openrouter",
            "llm_api_key": "",
            "llm_model": "google/gemini-2.5-flash",
            "llm_base_url": "https://openrouter.ai/api/v1",
            "polish_enabled": true,
            "translate_enabled": false,
            "target_lang": "en",
            "hotkey": "Ctrl+/",
            "hotkey_mode": "hold",
            "output_mode": "keyboard",
            "selected_text_enabled": false,
            "theme": "system",
            "auto_start": false,
            "close_to_tray": true,
            "start_minimized": false,
            "max_recording_seconds": 30,
            "ui_language": "en"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.polish_custom_prompt, "");
        assert_eq!(config.polish_chinese_script, "preserve");
    }

    #[test]
    fn app_config_sanitizes_custom_polish_prompt_and_clears_chinese_script() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["polish_custom_prompt"] = serde_json::json!("  use formal tone\0  ");
        value["polish_chinese_script"] = serde_json::json!("traditional");

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.polish_custom_prompt, "use formal tone");
        assert_eq!(config.polish_chinese_script, "preserve");
    }

    #[test]
    fn app_config_new_install_defaults_capsule_auto_hide_true() {
        let config = AppConfig::new_install_default();
        assert!(config.capsule_auto_hide);
    }

    #[test]
    fn app_config_existing_missing_capsule_auto_hide_defaults_false() {
        let value = serde_json::json!({
            "stt_provider": "deepgram",
            "stt_api_key": "hosted-secret"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.stt_provider, "deepgram");
        assert_eq!(config.stt_api_key, "hosted-secret");
        assert!(!config.capsule_auto_hide);
    }

    #[test]
    fn app_config_existing_explicit_capsule_auto_hide_is_preserved() {
        let value = serde_json::json!({
            "capsule_auto_hide": true
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert!(config.capsule_auto_hide);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_config_migrates_legacy_mac_alt_slash_label() {
        let value = serde_json::json!({
            "hotkey": "Alt+/"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.hotkey, "Option+/");
    }
}
