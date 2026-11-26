//! Keeyz's own global keyboard + mouse hook (replaces `rdev::listen`).
//!
//! Why: Windows silently removes a low-level hook whose callback is ever too slow (for example
//! when Windows 11 slows an idle background app down). Keeyz then sees no keys at all - no Ctrl
//! double-tap, no typed trigger, no toolbar - until it is restarted. rdev could not be restarted
//! and did slow work (character lookup) inside the callback.
//!
//! Here:
//! - the callbacks only copy the raw values and hand them to a worker thread;
//! - the hook thread runs at high priority and Keeyz opts out of Windows' power throttling;
//! - a watchdog compares Windows' "last input" time with the last event our hook saw. If Windows
//!   saw input that our hook missed, the hook is installed again on the spot.
//!
//! The worker turns raw events into `rdev::Event`s, so the rest of hook.rs is unchanged.

use once_cell::sync::{Lazy, OnceCell};
use rdev::{Button, Event, EventType, Key};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, ToUnicodeEx, HKL};
use windows::Win32::UI::WindowsAndMessaging::*;

/// What the callbacks copy out of Windows (nothing slow happens in the callback).
enum Raw {
    Key { down: bool, vk: u32, scan: u32, extended: bool, layout: isize, time: SystemTime },
    Mouse { msg: u32, data: u32, x: i32, y: i32, time: SystemTime },
}

static RAW_TX: OnceCell<Sender<Raw>> = OnceCell::new();
/// GetTickCount() of the last event the hook saw (also mouse moves).
static LAST_HOOK_TICK: AtomicU32 = AtomicU32::new(0);
/// Thread that owns the current hooks (0 = none).
static HOOK_TID: AtomicU32 = AtomicU32::new(0);
static RESTARTS: AtomicU32 = AtomicU32::new(0);
static HOOK_THREAD: Lazy<Mutex<Option<std::thread::JoinHandle<()>>>> = Lazy::new(|| Mutex::new(None));

/// Where the click / wheel event being handled right now happened (screen pixels).
pub static EVENT_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub static EVENT_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Mouse position (screen pixels) as last seen by the hook.
pub static MOUSE_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub static MOUSE_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Start the hook, its worker and the watchdog. `handle` gets every event (in order) on a worker thread.
pub fn start(handle: impl Fn(Event) + Send + 'static) {
    let (tx, rx) = channel::<Raw>();
    if RAW_TX.set(tx).is_err() {
        return; // already started
    }
    opt_out_of_power_throttling();
    std::thread::spawn(move || {
        for raw in rx {
            if let Some(ev) = to_event(raw) {
                handle(ev);
            }
        }
    });
    // Install once Keeyz has fully started: a hook set up while the main window is still being
    // created and shown never received any input, and the window system stalled.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(1500));
        install();
    });
    // watchdog: put the hook back if Windows dropped it
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(1000));
        if HOOK_TID.load(Ordering::SeqCst) == 0 {
            continue; // not installed yet
        }
        if hook_missed_input() {
            let n = RESTARTS.fetch_add(1, Ordering::SeqCst) + 1;
            crate::hook::dbg_log!("[hook] Windows saw input the hook missed - installing the hook again (#{n})");
            install();
            std::thread::sleep(Duration::from_secs(5)); // give it time before judging again
        }
    });
}

/// (Re)install both hooks on a fresh high-priority thread. Hooks die with their thread, so the
/// old thread is told to quit first.
fn install() {
    let old = HOOK_TID.swap(0, Ordering::SeqCst);
    if old != 0 {
        unsafe {
            let _ = PostThreadMessageW(old, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(h) = HOOK_THREAD.lock().unwrap().take() {
            let t0 = std::time::Instant::now();
            while !h.is_finished() && t0.elapsed() < Duration::from_millis(500) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    let h = std::thread::spawn(|| unsafe {
        HOOK_TID.store(GetCurrentThreadId(), Ordering::SeqCst);
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        let module = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).map(HINSTANCE::from).unwrap_or_default();
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), module, 0);
        let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        crate::hook::dbg_log!("[hook] installed: keyboard={} mouse={}", kb.is_ok(), ms.is_ok());
        LAST_HOOK_TICK.store(GetTickCount(), Ordering::SeqCst);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        if let Ok(k) = kb {
            let _ = UnhookWindowsHookEx(k);
        }
        if let Ok(m) = ms {
            let _ = UnhookWindowsHookEx(m);
        }
    });
    *HOOK_THREAD.lock().unwrap() = Some(h);
}

/// True when Windows registered keyboard/mouse input more than a second after the last event
/// our hook saw - i.e. the hook is gone. Input aimed at an administrator window never reaches a
/// normal app's hook, so that case is not counted.
fn hook_missed_input() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if unsafe { !GetLastInputInfo(&mut li).as_bool() } {
        return false;
    }
    let last_input = li.dwTime;
    let last_hook = LAST_HOOK_TICK.load(Ordering::SeqCst);
    let now = unsafe { GetTickCount() };
    let missed = last_input.wrapping_sub(last_hook) as i32 > 1500; // input newer than the hook's last event
    let recent = now.wrapping_sub(last_input) < 30_000;
    missed && recent && !crate::winutil::foreground_is_admin()
}

fn opt_out_of_power_throttling() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_STATE,
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: 1, // PROCESS_POWER_THROTTLING_CURRENT_VERSION
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0, // = never throttle
    };
    unsafe {
        let ok = SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const _,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
        crate::hook::dbg_log!("[hook] power throttling off: {}", ok.is_ok());
    }
}

// ------------------------------------------------------------------ callbacks (keep them tiny)

unsafe extern "system" fn kb_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        LAST_HOOK_TICK.store(GetTickCount(), Ordering::Relaxed);
        if !crate::state::INJECTING.load(Ordering::Relaxed) {
            let k = &*(l.0 as *const KBDLLHOOKSTRUCT);
            let m = w.0 as u32;
            let down = m == WM_KEYDOWN || m == WM_SYSKEYDOWN;
            if down || m == WM_KEYUP || m == WM_SYSKEYUP {
                // keyboard layout of the app in front, for turning the key into a character later
                let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
                let layout = GetKeyboardLayout(fg_thread).0 as isize;
                if let Some(tx) = RAW_TX.get() {
                    let _ = tx.send(Raw::Key {
                        down,
                        vk: k.vkCode,
                        scan: k.scanCode,
                        extended: k.flags.0 & LLKHF_EXTENDED.0 != 0,
                        layout,
                        time: SystemTime::now(),
                    });
                }
            }
        }
    }
    CallNextHookEx(None, code, w, l)
}

unsafe extern "system" fn mouse_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        LAST_HOOK_TICK.store(GetTickCount(), Ordering::Relaxed);
        let m = &*(l.0 as *const MSLLHOOKSTRUCT);
        MOUSE_X.store(m.pt.x, Ordering::Relaxed);
        MOUSE_Y.store(m.pt.y, Ordering::Relaxed);
        let msg = w.0 as u32;
        if msg != WM_MOUSEMOVE && !crate::state::INJECTING.load(Ordering::Relaxed) {
            if let Some(tx) = RAW_TX.get() {
                let _ = tx.send(Raw::Mouse { msg, data: m.mouseData, x: m.pt.x, y: m.pt.y, time: SystemTime::now() });
            }
        }
    }
    CallNextHookEx(None, code, w, l)
}

// ------------------------------------------------------------------ raw -> rdev::Event (worker)

fn to_event(raw: Raw) -> Option<Event> {
    match raw {
        Raw::Key { down, vk, scan, extended, layout, time } => {
            let key = key_of(vk, extended);
            let event_type = if down { EventType::KeyPress(key) } else { EventType::KeyRelease(key) };
            let name = if down { char_of(vk, scan, layout) } else { None };
            Some(Event { event_type, time, name })
        }
        Raw::Mouse { msg, data, x, y, time } => {
            // the handler asks "where was this click?" - answer with the click's own position
            EVENT_X.store(x, Ordering::SeqCst);
            EVENT_Y.store(y, Ordering::SeqCst);
            let hi = (data >> 16) as u16;
            let event_type = match msg {
                WM_LBUTTONDOWN => EventType::ButtonPress(Button::Left),
                WM_LBUTTONUP => EventType::ButtonRelease(Button::Left),
                WM_RBUTTONDOWN => EventType::ButtonPress(Button::Right),
                WM_RBUTTONUP => EventType::ButtonRelease(Button::Right),
                WM_MBUTTONDOWN => EventType::ButtonPress(Button::Middle),
                WM_MBUTTONUP => EventType::ButtonRelease(Button::Middle),
                WM_XBUTTONDOWN => EventType::ButtonPress(Button::Unknown(hi as u8)),
                WM_XBUTTONUP => EventType::ButtonRelease(Button::Unknown(hi as u8)),
                WM_MOUSEWHEEL => EventType::Wheel { delta_x: 0, delta_y: (hi as i16 / 120) as i64 },
                WM_MOUSEHWHEEL => EventType::Wheel { delta_x: (hi as i16 / 120) as i64, delta_y: 0 },
                _ => return None,
            };
            Some(Event { event_type, time, name: None })
        }
    }
}

/// The character a key types without modifiers ("k", "8", " "), like rdev reported it.
/// Flag 4 = don't change the keyboard state (so dead keys in the user's app keep working).
fn char_of(vk: u32, scan: u32, layout: isize) -> Option<String> {
    let state = [0u8; 256];
    let mut buf = [0u16; 8];
    let n = unsafe { ToUnicodeEx(vk, scan, &state, &mut buf, 4, HKL(layout as *mut _)) };
    if n <= 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    if s.is_empty() { None } else { Some(s) }
}

/// Virtual key -> rdev::Key (the keys Keeyz looks at by name; everything else is Unknown(vk)).
pub fn key_of(vk: u32, extended: bool) -> Key {
    match vk {
        0x10 | 0xA0 => Key::ShiftLeft,
        0xA1 => Key::ShiftRight,
        0x11 | 0xA2 => if vk == 0x11 && extended { Key::ControlRight } else { Key::ControlLeft },
        0xA3 => Key::ControlRight,
        0x12 | 0xA4 => Key::Alt,
        0xA5 => Key::AltGr,
        0x5B => Key::MetaLeft,
        0x5C => Key::MetaRight,
        0x1B => Key::Escape,
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => if extended { Key::KpReturn } else { Key::Return },
        0x14 => Key::CapsLock,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::LeftArrow,
        0x26 => Key::UpArrow,
        0x27 => Key::RightArrow,
        0x28 => Key::DownArrow,
        0x2D => Key::Insert,
        0x2E => Key::Delete,
        other => Key::Unknown(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_keeyz_needs() {
        assert_eq!(key_of(0xA2, false), Key::ControlLeft);
        assert_eq!(key_of(0xA3, true), Key::ControlRight);
        assert_eq!(key_of(0x11, false), Key::ControlLeft);
        assert_eq!(key_of(0x0D, false), Key::Return);
        assert_eq!(key_of(0x0D, true), Key::KpReturn);
        assert_eq!(key_of(0x1B, false), Key::Escape);
        assert_eq!(key_of(0xE8, false), Key::Unknown(0xE8));
    }
}
