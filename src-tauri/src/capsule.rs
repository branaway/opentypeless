use tauri::{LogicalPosition, Manager};

use crate::app_detector::WindowFrame;

/// Vertical gap (in logical points) between the capsule's bottom edge and the
/// bottom of the monitor it's centered on.
const BOTTOM_MARGIN: f64 = 80.0;

/// Move the capsule window to be horizontally centered, near the bottom, of
/// whatever monitor the user's currently-focused window lives on. Falls back
/// to leaving the capsule where it is if the focused window's monitor can't
/// be determined (e.g. Linux, or Accessibility permission not granted).
pub fn reposition_to_focused_window(app_handle: &tauri::AppHandle) {
    let Some(window) = app_handle.get_webview_window("capsule") else {
        return;
    };
    let Some(frame) = crate::app_detector::focused_window_frame() else {
        return;
    };
    let Ok(monitors) = window.available_monitors() else {
        return;
    };
    let Some(monitor) = find_containing_monitor(&frame, &monitors) else {
        return;
    };

    let scale = monitor.scale_factor();
    let mon_x = monitor.position().x as f64 / scale;
    let mon_y = monitor.position().y as f64 / scale;
    let mon_w = monitor.size().width as f64 / scale;
    let mon_h = monitor.size().height as f64 / scale;

    let Ok(outer_size) = window.outer_size() else {
        return;
    };
    let win_w = outer_size.width as f64 / scale;
    let win_h = outer_size.height as f64 / scale;

    let x = mon_x + (mon_w / 2.0 - win_w / 2.0);
    let y = mon_y + (mon_h - win_h - BOTTOM_MARGIN);
    let _ = window.set_position(LogicalPosition::new(x, y));
}

fn find_containing_monitor<'a>(
    frame: &WindowFrame,
    monitors: &'a [tauri::Monitor],
) -> Option<&'a tauri::Monitor> {
    let center_x = frame.x + frame.width / 2.0;
    let center_y = frame.y + frame.height / 2.0;

    monitors.iter().find(|m| {
        let scale = if frame.physical { 1.0 } else { m.scale_factor() };
        let mx = m.position().x as f64 / scale;
        let my = m.position().y as f64 / scale;
        let mw = m.size().width as f64 / scale;
        let mh = m.size().height as f64 / scale;
        center_x >= mx && center_x < mx + mw && center_y >= my && center_y < my + mh
    })
}
