//! macOS Fn / 🌐 key trigger.
//!
//! The global-shortcut plugin is built on Carbon hotkeys, which cannot register
//! the Fn (globe / 🌐) key. To use Fn as the recording trigger — like the system
//! dictation double-tap, but a single tap — we tap the CoreGraphics event stream
//! directly and watch the secondary-Fn modifier flag.
//!
//! Detection is a *clean single tap*: Fn pressed then released with no other key
//! or modifier in between. That distinguishes a deliberate tap from `Fn`+arrow /
//! `Fn`+F-key combos and from holding Fn as a real modifier. On a clean tap we
//! run the exact same toggle action as the configured hotkey.
//!
//! Requires Accessibility (already requested by the app) and, for keyboard event
//! taps, macOS may additionally prompt for Input Monitoring. The system's own
//! globe behavior should be set to "Do Nothing" / dictation disabled so the tap
//! isn't shadowed by the OS.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult,
};
use tauri::Manager;

extern "C" {
    /// Re-enable (or disable) an event tap by its mach port. The `core-graphics`
    /// wrapper exposes this only on the non-`Send` `CGEventTap`, which we can't
    /// capture in the tap callback, so we call it directly on the raw port.
    fn CGEventTapEnable(tap: *const c_void, enable: bool);
}

/// macOS Input Monitoring (IOHID "listen") permission. A keyboard `CGEventTap`
/// only receives *global* events when this is granted; without it the tap is
/// created successfully but only sees events while our own app is frontmost —
/// so the Fn trigger appears dead in every other app. This is a *separate*
/// permission from Accessibility, and nothing else in the app requests it, so
/// the app would never even show up in the Input Monitoring list.
mod input_monitoring {
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
        fn IOHIDRequestAccess(request: u32) -> bool;
    }

    // IOHIDRequestType: listen to events (monitoring), as opposed to posting.
    const REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
    // IOHIDAccessType::granted.
    const ACCESS_TYPE_GRANTED: u32 = 0;

    /// True if Input Monitoring is already granted to this app.
    pub fn is_granted() -> bool {
        unsafe { IOHIDCheckAccess(REQUEST_TYPE_LISTEN_EVENT) == ACCESS_TYPE_GRANTED }
    }

    /// Trigger the system Input Monitoring prompt (and register the app in the
    /// Privacy list). Returns true if access is granted. Safe to call repeatedly;
    /// the OS only shows the prompt once.
    pub fn request() -> bool {
        unsafe { IOHIDRequestAccess(REQUEST_TYPE_LISTEN_EVENT) }
    }
}

/// Modifier flags that, if present alongside Fn, mean the user is using Fn as a
/// real modifier (e.g. Fn+Shift) rather than tapping it on its own.
const OTHER_MODS: CGEventFlags = CGEventFlags::from_bits_truncate(
    CGEventFlags::CGEventFlagShift.bits()
        | CGEventFlags::CGEventFlagControl.bits()
        | CGEventFlags::CGEventFlagAlternate.bits()
        | CGEventFlags::CGEventFlagCommand.bits(),
);

/// Longest Fn hold still treated as a "tap". A leisurely tap is fine; a long
/// hold (resting a finger) is ignored so it can't fire on release.
const MAX_TAP_MS: u128 = 700;

struct TapState {
    fn_down: bool,
    press_at: Option<Instant>,
    /// Another key/modifier intervened during this Fn press → not a clean tap.
    dirty: bool,
}

/// Start the Fn-key listener on a dedicated thread with its own run loop. The
/// thread lives for the app's lifetime. No-op effect if the event tap can't be
/// created (e.g. missing permission) — it just logs and exits.
pub fn spawn(app_handle: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("fn-hotkey".into())
        .spawn(move || run(app_handle))
        .ok();
}

fn run(app_handle: tauri::AppHandle) {
    tracing::info!("fn-hotkey: thread started, creating event tap…");

    // A keyboard event tap needs Input Monitoring to see events globally. Without
    // it the Fn trigger only fires while our own window is frontmost, which looks
    // like "the hotkey does nothing in other apps". Request it up front so the
    // system prompt appears and the app is added to the Privacy list; if it's
    // still not granted, point the user straight at the right settings pane.
    if !input_monitoring::is_granted() {
        tracing::warn!("Input Monitoring not granted; requesting access for the Fn trigger…");
        if !input_monitoring::request() {
            tracing::warn!(
                "Input Monitoring still not granted — the Fn trigger will only work while \
                 MyTypeless is frontmost until it is enabled in System Settings → Privacy & \
                 Security → Input Monitoring (a restart may be needed after enabling)."
            );
            let _ = std::process::Command::new("/usr/bin/open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
                .spawn();
        }
    }
    let state = Arc::new(Mutex::new(TapState {
        fn_down: false,
        press_at: None,
        dirty: false,
    }));
    // Raw mach port of the tap, filled in after creation, so the callback can
    // re-enable the tap if macOS disables it under load.
    let port: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

    let cb_state = state.clone();
    let cb_port = port.clone();
    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        // Listen only: we observe Fn, never consume or alter events.
        CGEventTapOptions::ListenOnly,
        // Only real event types go in the interest mask. The crate builds it as
        // `1 << (type as u64)`, and the TapDisabled* sentinels (0xFFFF_FFFE/F)
        // would overflow that shift and panic. Tap-disabled notifications are
        // delivered to the callback regardless of the mask, so we still handle
        // them below without listing them here.
        vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
        move |_proxy, event_type, event| {
            match event_type {
                // macOS can disable a tap under heavy load — re-enable it.
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                    let p = cb_port.load(Ordering::SeqCst);
                    if p != 0 {
                        unsafe { CGEventTapEnable(p as *const c_void, true) };
                    }
                }
                CGEventType::KeyDown => {
                    // Any key pressed while Fn is held means Fn is being used as a
                    // modifier, not tapped.
                    let mut s = cb_state.lock().unwrap_or_else(|e| e.into_inner());
                    if s.fn_down {
                        s.dirty = true;
                    }
                }
                CGEventType::FlagsChanged => {
                    let flags = event.get_flags();
                    let fn_now = flags.contains(CGEventFlags::CGEventFlagSecondaryFn);
                    let other_now = flags.intersects(OTHER_MODS);
                    let mut s = cb_state.lock().unwrap_or_else(|e| e.into_inner());
                    if fn_now && !s.fn_down {
                        // Fn pressed.
                        s.fn_down = true;
                        s.press_at = Some(Instant::now());
                        s.dirty = other_now; // pressed together with another modifier
                    } else if !fn_now && s.fn_down {
                        // Fn released — fire on a clean, quick tap.
                        let quick = s
                            .press_at
                            .map(|t| t.elapsed().as_millis() <= MAX_TAP_MS)
                            .unwrap_or(false);
                        let clean = !s.dirty && quick;
                        s.fn_down = false;
                        s.press_at = None;
                        s.dirty = false;
                        drop(s);
                        if clean {
                            tracing::info!("fn-hotkey: clean Fn tap → trigger");
                            trigger(&app_handle);
                        }
                    } else if fn_now && other_now {
                        // Another modifier joined while Fn held → not a clean tap.
                        s.dirty = true;
                    }
                }
                _ => {}
            }
            // Listen-only: never alter the event stream.
            CallbackResult::Keep
        },
    );

    let tap = match tap {
        Ok(t) => t,
        Err(_) => {
            tracing::warn!(
                "Fn-key tap could not be created (grant Accessibility / Input Monitoring \
                 to MyTypeless in System Settings → Privacy & Security)"
            );
            return;
        }
    };

    let mach_port = tap.mach_port();
    let source = match mach_port.create_runloop_source(0) {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!("Fn-key tap: failed to create run-loop source");
            return;
        }
    };
    port.store(
        mach_port.as_concrete_TypeRef() as *const c_void as usize,
        Ordering::SeqCst,
    );
    tap.enable();

    let run_loop = CFRunLoop::get_current();
    unsafe {
        run_loop.add_source(&source, kCFRunLoopCommonModes);
    }
    tracing::info!("Fn-key trigger active");
    CFRunLoop::run_current();
}

/// Fire the same toggle action as the configured hotkey, honoring the user's
/// toggle/hold mode setting.
fn trigger(app_handle: &tauri::AppHandle) {
    let hotkey_mode = app_handle
        .state::<crate::HotkeyModeCache>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    crate::hotkey::spawn_press(app_handle.clone(), hotkey_mode);
}
