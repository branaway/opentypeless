use serde::{Deserialize, Serialize};

use crate::llm::AppType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppContext {
    pub app_name: String,
    pub window_title: String,
    pub app_type: AppType,
}

impl Default for AppContext {
    fn default() -> Self {
        Self {
            app_name: String::new(),
            window_title: String::new(),
            app_type: AppType::General,
        }
    }
}

/// The frontmost window's on-screen frame, used to figure out which monitor
/// the user is actually looking at (not just where our own capsule window
/// happens to sit). `physical` indicates whether x/y/width/height are in
/// physical pixels (Windows) or logical points (macOS, via the Accessibility
/// API) so callers can compare against the right coordinate space.
#[derive(Debug, Clone, Copy)]
pub struct WindowFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub physical: bool,
}

pub fn focused_window_frame() -> Option<WindowFrame> {
    #[cfg(target_os = "macos")]
    {
        macos_focused_window_frame()
    }
    #[cfg(target_os = "windows")]
    {
        windows_focused_window_frame()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_focused_window_frame() -> Option<WindowFrame> {
    use std::process::Command;

    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events"
            set frontApp to first application process whose frontmost is true
            try
                set winPos to position of front window of frontApp
                set winSize to size of front window of frontApp
                return (item 1 of winPos as string) & "," & (item 2 of winPos as string) & "," & (item 1 of winSize as string) & "," & (item 2 of winSize as string)
            on error
                return ""
            end try
        end tell"#,
        ])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let parts: Vec<f64> = text
        .trim()
        .split(',')
        .filter_map(|p| p.trim().parse::<f64>().ok())
        .collect();
    if parts.len() != 4 {
        return None;
    }
    Some(WindowFrame {
        x: parts[0],
        y: parts[1],
        width: parts[2],
        height: parts[3],
        physical: false,
    })
}

#[cfg(target_os = "windows")]
fn windows_focused_window_frame() -> Option<WindowFrame> {
    unsafe {
        let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut rect: windows_sys::Win32::Foundation::RECT = std::mem::zeroed();
        if windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        Some(WindowFrame {
            x: rect.left as f64,
            y: rect.top as f64,
            width: (rect.right - rect.left) as f64,
            height: (rect.bottom - rect.top) as f64,
            physical: true,
        })
    }
}

pub fn detect_current_app() -> AppContext {
    #[cfg(target_os = "windows")]
    {
        windows_detect()
    }
    #[cfg(target_os = "macos")]
    {
        macos_detect()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        AppContext::default()
    }
}

#[cfg(target_os = "macos")]
fn macos_detect() -> AppContext {
    use std::process::Command;

    let app_name = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to get name of first application process whose frontmost is true"#,
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let window_title = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events"
            set frontApp to first application process whose frontmost is true
            try
                get name of front window of frontApp
            on error
                return ""
            end try
        end tell"#,
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let app_type = classify_app(&app_name);
    AppContext {
        app_name,
        window_title,
        app_type,
    }
}

#[cfg(target_os = "windows")]
fn windows_detect() -> AppContext {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    unsafe {
        let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if hwnd.is_null() {
            return AppContext::default();
        }

        // Get window title
        let mut title_buf = [0u16; 512];
        let len = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(
            hwnd,
            title_buf.as_mut_ptr(),
            title_buf.len() as i32,
        );
        let window_title = if len > 0 {
            OsString::from_wide(&title_buf[..len as usize])
                .to_string_lossy()
                .to_string()
        } else {
            String::new()
        };

        // Get process name
        let mut pid = 0u32;
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, &mut pid);

        let app_name = get_process_name(pid).unwrap_or_default();
        let app_type = classify_app(&app_name);

        AppContext {
            app_name,
            window_title,
            app_type,
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn get_process_name(pid: u32) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    if pid == 0 {
        return None;
    }

    let handle = windows_sys::Win32::System::Threading::OpenProcess(
        windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
        0,
        pid,
    );
    if handle.is_null() {
        return None;
    }

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let ok = windows_sys::Win32::System::Threading::QueryFullProcessImageNameW(
        handle,
        0,
        buf.as_mut_ptr(),
        &mut size,
    );
    let _ = windows_sys::Win32::Foundation::CloseHandle(handle);

    if ok != 0 && size > 0 {
        let path = OsString::from_wide(&buf[..size as usize])
            .to_string_lossy()
            .to_string();
        path.rsplit('\\').next().map(|s| s.to_string())
    } else {
        None
    }
}

#[allow(dead_code)]
fn classify_app(app_name: &str) -> AppType {
    let name = app_name.to_lowercase();
    if ["outlook", "gmail", "thunderbird", "mail"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Email
    } else if ["slack", "discord", "wechat", "telegram", "teams"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Chat
    } else if [
        "terminal",
        "iterm",
        "warp",
        "kitty",
        "alacritty",
        "wezterm",
        "tabby",
        "hyper",
        "console",
        "ghostty",
    ]
    .iter()
    .any(|k| name.contains(k))
    {
        AppType::Terminal
    } else if ["code", "intellij", "vim", "nvim", "cursor"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Code
    } else if ["word", "docs", "notion", "obsidian", "typora"]
        .iter()
        .any(|k| name.contains(k))
    {
        AppType::Document
    } else {
        AppType::General
    }
}
