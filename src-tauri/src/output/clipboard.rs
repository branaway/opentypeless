use async_trait::async_trait;

use crate::error::AppError;

use super::{OutputMode, TextOutput};

/// Delay after writing to clipboard before simulating paste.
const CLIPBOARD_SETTLE_MS: u64 = 20;
/// Delay after paste before restoring the original clipboard, so the paste
/// has finished reading the clipboard before we overwrite it.
const CLIPBOARD_RESTORE_DELAY_MS: u64 = 120;

pub struct ClipboardOutput {
    /// When true, back up the clipboard before pasting and restore it after.
    /// Used for terminal paste so the user's clipboard isn't clobbered.
    restore_clipboard: bool,
}

impl Default for ClipboardOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardOutput {
    pub fn new() -> Self {
        Self {
            restore_clipboard: false,
        }
    }

    /// Create a clipboard output that restores the previous clipboard contents
    /// after pasting (used for terminal output where we paste transiently).
    pub fn with_restore(restore_clipboard: bool) -> Self {
        Self { restore_clipboard }
    }
}

#[cfg(any(target_os = "linux", test))]
fn should_auto_paste_after_clipboard(session_type: &str) -> bool {
    !session_type.eq_ignore_ascii_case("wayland")
}

#[async_trait]
impl TextOutput for ClipboardOutput {
    async fn type_text(&self, text: &str) -> Result<(), AppError> {
        let text = text.to_string();
        let restore_clipboard = self.restore_clipboard;
        tokio::task::spawn_blocking(move || -> Result<(), AppError> {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| AppError::Output(format!("Failed to access clipboard: {}", e)))?;

            // Back up existing clipboard text so we can restore it after pasting.
            let backup = if restore_clipboard {
                clipboard.get_text().ok()
            } else {
                None
            };

            clipboard
                .set_text(&text)
                .map_err(|e| AppError::Output(format!("Failed to set clipboard: {}", e)))?;

            std::thread::sleep(std::time::Duration::from_millis(CLIPBOARD_SETTLE_MS));

            #[cfg(target_os = "linux")]
            if !should_auto_paste_after_clipboard(&crate::platform::current_session_type()) {
                return Ok(());
            }

            // On macOS: trigger Cmd+V via osascript (AppleScript).
            // This avoids the Accessibility permission requirement that enigo's
            // CGEventPost needs. The apple-events entitlement is already declared.
            // On Windows/Linux: use enigo's SendInput which needs no special permissions.
            #[cfg(target_os = "macos")]
            {
                let status = std::process::Command::new("osascript")
                    .args([
                        "-e",
                        r#"tell application "System Events" to keystroke "v" using command down"#,
                    ])
                    .status()
                    .map_err(|e| AppError::Output(format!("osascript error: {}", e)))?;
                if !status.success() {
                    return Err(AppError::Output(format!(
                        "osascript paste failed with exit code: {:?}",
                        status.code()
                    )));
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                use enigo::{Direction, Enigo, Key, Keyboard, Settings};
                let mut enigo = Enigo::new(&Settings::default())
                    .map_err(|e| AppError::Output(format!("Failed to create Enigo: {:?}", e)))?;

                enigo
                    .key(Key::Control, Direction::Press)
                    .map_err(|e| AppError::Output(format!("Key press error: {:?}", e)))?;
                enigo
                    .key(Key::Unicode('v'), Direction::Click)
                    .map_err(|e| AppError::Output(format!("Key click error: {:?}", e)))?;
                enigo
                    .key(Key::Control, Direction::Release)
                    .map_err(|e| AppError::Output(format!("Key release error: {:?}", e)))?;
            }

            // Restore the original clipboard contents after the paste completes.
            // The delay ensures the destination app has finished reading the
            // pasted text before we overwrite the clipboard.
            if restore_clipboard {
                std::thread::sleep(std::time::Duration::from_millis(CLIPBOARD_RESTORE_DELAY_MS));
                match backup {
                    Some(prev) => {
                        let _ = clipboard.set_text(&prev);
                    }
                    None => {
                        // Original clipboard was empty/unreadable — clear our text
                        // so we don't leave the dictation lingering on the clipboard.
                        let _ = clipboard.set_text("");
                    }
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| AppError::Output(format!("Spawn blocking error: {}", e)))?
    }

    fn mode(&self) -> OutputMode {
        OutputMode::Clipboard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_clipboard_output_is_copy_only() {
        assert!(!should_auto_paste_after_clipboard("wayland"));
        assert!(!should_auto_paste_after_clipboard("WAYLAND"));
    }

    #[test]
    fn x11_clipboard_output_keeps_auto_paste() {
        assert!(should_auto_paste_after_clipboard("x11"));
        assert!(should_auto_paste_after_clipboard("unknown"));
    }
}
