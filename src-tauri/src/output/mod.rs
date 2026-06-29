pub mod clipboard;
pub mod keyboard;

use crate::error::{AppError, UserError};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Keyboard,
    Clipboard,
}

#[async_trait]
pub trait TextOutput: Send + Sync {
    async fn type_text(&self, text: &str) -> Result<(), AppError>;
    fn mode(&self) -> OutputMode;
}

pub fn create_output(mode: OutputMode, app_handle: &tauri::AppHandle) -> Box<dyn TextOutput> {
    match mode {
        OutputMode::Keyboard => Box::new(keyboard::KeyboardOutput::new(app_handle)),
        OutputMode::Clipboard => Box::new(clipboard::ClipboardOutput::new()),
    }
}

/// Simulate a single plain Return/Enter keypress in the focused app, used to
/// optionally submit the output right after the text was typed or pasted (e.g.
/// run a terminal command or send a chat message). This is a *plain* Return —
/// distinct from the Shift+Return soft newlines keyboard typing uses for line
/// breaks within the text.
///
/// The mechanism mirrors the paste path per platform so it needs no extra
/// permissions: on macOS via System Events (covered by the apple-events
/// entitlement, no Accessibility prompt), elsewhere via enigo.
pub fn press_enter() -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        // key code 36 == Return.
        let status = std::process::Command::new("osascript")
            .args(["-e", r#"tell application "System Events" to key code 36"#])
            .status()
            .map_err(|e| AppError::Output(format!("osascript enter error: {}", e)))?;
        if !status.success() {
            return Err(AppError::Output(format!(
                "osascript enter failed with exit code: {:?}",
                status.code()
            )));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to create Enigo: {:?}", e)))?;
        enigo
            .key(Key::Return, Direction::Click)
            .map_err(|e| AppError::Output(format!("Enter key error: {:?}", e)))?;
        Ok(())
    }
}

fn clipboard_warning_for_platform() -> Option<UserError> {
    if crate::platform::is_wayland_session() {
        Some(UserError {
            code: "output_wayland_clipboard_copy_only".to_string(),
            details: None,
            retry_count: 0,
        })
    } else {
        None
    }
}

/// Try keyboard output first. On failure, fall back to clipboard.
/// Returns Ok(Some(UserError)) if fell back to clipboard (warning for frontend).
/// Returns Ok(None) if primary output succeeded.
/// Returns Err if both keyboard and clipboard failed.
pub async fn output_with_fallback(
    app_handle: &tauri::AppHandle,
    text: &str,
    mode: OutputMode,
    restore_clipboard: bool,
) -> Result<Option<UserError>, String> {
    if mode == OutputMode::Clipboard {
        let output = clipboard::ClipboardOutput::with_restore(restore_clipboard);
        return output
            .type_text(text)
            .await
            .map_err(|e| e.to_string())
            .map(|_| clipboard_warning_for_platform());
    }

    // Try keyboard first
    let keyboard = create_output(OutputMode::Keyboard, app_handle);
    match keyboard.type_text(text).await {
        Ok(()) => Ok(None),
        Err(kb_err) => {
            tracing::warn!(
                "Keyboard output failed: {}, falling back to clipboard",
                kb_err
            );
            let clipboard = create_output(OutputMode::Clipboard, app_handle);
            match clipboard.type_text(text).await {
                Ok(()) => Ok(clipboard_warning_for_platform().or(Some(UserError {
                    code: "output_fallback_clipboard".to_string(),
                    details: Some(kb_err.to_string()),
                    retry_count: 0,
                }))),
                Err(cb_err) => Err(format!(
                    "Both keyboard ({}) and clipboard ({}) output failed",
                    kb_err, cb_err
                )),
            }
        }
    }
}
