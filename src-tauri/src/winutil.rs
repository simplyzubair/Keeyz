//! Windows helpers: focus, keyboard injection, clipboard, caret position, UI Automation.

use crate::state::INJECTING;
use std::sync::atomic::Ordering;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const VK_C: u16 = 0x43;
pub const VK_V: u16 = 0x56;
pub const VK_A: u16 = 0x41;
pub const VK_Z: u16 = 0x5A;

// ------------------------------------------------------------ windows / focus

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn focus(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let h = HWND(hwnd as *mut _);
        let _ = SetForegroundWindow(h);
        if GetForegroundWindow() != h {
            // Alt tap gives us the right to move focus to another process.
            key(VK_MENU.0, true);
            key(VK_MENU.0, false);
            let _ = SetForegroundWindow(h);
        }
    }
    sleep(Duration::from_millis(80));
}

pub fn app_name(hwnd: isize) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            if let Some(d) = file_description(&path) {
                return d;
            }
            let file = path.rsplit('\\').next().unwrap_or("").trim_end_matches(".exe").trim_end_matches(".EXE");
            let mut c = file.chars();
            return match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            };
        }
        String::new()
    }
}

/// "Google Chrome" instead of "chrome": the File Description stored inside the exe.
fn file_description(path: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), 0, size, data.as_mut_ptr() as *mut _).ok()?;
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let tr: Vec<u16> = "\\VarFileInfo\\Translation".encode_utf16().chain(Some(0)).collect();
        let (lang, cp) = if VerQueryValueW(data.as_ptr() as *const _, PCWSTR(tr.as_ptr()), &mut p, &mut len).as_bool() && len >= 4 {
            let t = p as *const u16;
            (*t, *t.add(1))
        } else {
            (0x0409, 0x04b0)
        };
        let q: Vec<u16> = format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription").encode_utf16().chain(Some(0)).collect();
        if !VerQueryValueW(data.as_ptr() as *const _, PCWSTR(q.as_ptr()), &mut p, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p as *const u16, len as usize));
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty() && s.len() < 60).then_some(s)
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Screen position of the text caret in the focused app, if the app exposes it.
pub fn caret_pos() -> Option<(i32, i32)> {
    unsafe {
        let fg = GetForegroundWindow();
        let tid = GetWindowThreadProcessId(fg, None);
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if GetGUIThreadInfo(tid, &mut info).is_ok() && !info.hwndCaret.0.is_null() {
            let mut p = POINT { x: info.rcCaret.left, y: info.rcCaret.bottom };
            let _ = ClientToScreen(info.hwndCaret, &mut p);
            if p.x != 0 || p.y != 0 {
                return Some((p.x, p.y));
            }
        }
    }
    None
}

// ------------------------------------------------------------ keyboard

fn key(vk: u16, down: bool) {
    let flags = if down { KEYBD_EVENT_FLAGS(0) } else { KEYEVENTF_KEYUP };
    let ext = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E);
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if ext { flags | KEYEVENTF_EXTENDEDKEY } else { flags },
                time: 0,
                dwExtraInfo: 0x4B53, // "KS" marker
            },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn tap(vk: u16) {
    key(vk, true);
    key(vk, false);
}

/// Run `f` while flagged as "injecting", so our keyboard hook ignores these keys.
pub fn injecting<T>(f: impl FnOnce() -> T) -> T {
    INJECTING.store(true, Ordering::SeqCst);
    let r = f();
    sleep(Duration::from_millis(30));
    INJECTING.store(false, Ordering::SeqCst);
    r
}

pub fn combo(mods: &[u16], vk: u16) {
    for m in mods {
        key(*m, true);
    }
    tap(vk);
    for m in mods.iter().rev() {
        key(*m, false);
    }
}

pub fn ctrl(vk: u16) {
    combo(&[VK_CONTROL.0], vk);
}

pub fn is_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// The user is often still holding Ctrl+Shift from the hotkey. Wait, then force release.
pub fn release_modifiers() {
    let mods = [VK_CONTROL.0, VK_SHIFT.0, VK_MENU.0, VK_LWIN.0, VK_RWIN.0];
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(600) && mods.iter().any(|m| is_down(*m)) {
        sleep(Duration::from_millis(15));
    }
    for m in mods {
        if is_down(m) {
            key(m, false);
        }
    }
}

pub fn backspaces(n: usize) {
    for _ in 0..n {
        tap(VK_BACK.0);
    }
}

pub fn select_left(n: usize) {
    key(VK_SHIFT.0, true);
    for _ in 0..n {
        tap(VK_LEFT.0);
    }
    key(VK_SHIFT.0, false);
}

// ------------------------------------------------------------ clipboard

pub fn clip_seq() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub fn clip_get() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

pub fn clip_set(text: &str) -> bool {
    for _ in 0..5 {
        if let Ok(mut c) = arboard::Clipboard::new() {
            if c.set_text(text.to_string()).is_ok() {
                return true;
            }
        }
        sleep(Duration::from_millis(30));
    }
    false
}

/// Copy the current selection of `hwnd` via Ctrl+C, restoring the old clipboard afterwards.
pub fn copy_selection(hwnd: isize) -> String {
    crate::clipboard::begin_own();
    let old = clip_get();
    let before = clip_seq();
    injecting(|| {
        release_modifiers();
        if hwnd != 0 && foreground() != hwnd {
            focus(hwnd);
        }
        ctrl(VK_C);
    });
    let start = Instant::now();
    while clip_seq() == before && start.elapsed() < Duration::from_millis(450) {
        sleep(Duration::from_millis(15));
    }
    let text = if clip_seq() != before { clip_get().unwrap_or_default() } else { String::new() };
    if clip_seq() != before {
        if let Some(o) = old {
            clip_set(&o);
        }
    }
    crate::clipboard::end_own();
    text
}

/// Paste `text` into `hwnd` at the caret (replacing any selection), then restore the clipboard.
pub fn paste_text(hwnd: isize, text: &str) {
    crate::clipboard::begin_own();
    let old = clip_get();
    clip_set(text);
    injecting(|| {
        release_modifiers();
        if hwnd != 0 && foreground() != hwnd {
            focus(hwnd);
        }
        ctrl(VK_V);
    });
    sleep(Duration::from_millis(250));
    if let Some(o) = old {
        clip_set(&o);
    }
    crate::clipboard::end_own();
}

// ------------------------------------------------------------ UI Automation

/// Selected text of the focused control without touching the clipboard.
pub fn uia_selection() -> Option<String> {
    use uiautomation::patterns::UITextPattern;
    let auto = uiautomation::UIAutomation::new().ok()?;
    let el = auto.get_focused_element().ok()?;
    let tp: UITextPattern = el.get_pattern().ok()?;
    let ranges = tp.get_selection().ok()?;
    let mut out = String::new();
    for r in ranges {
        if let Ok(t) = r.get_text(-1) {
            out.push_str(&t);
        }
    }
    let t = out.trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// Whole text of the focused document (used by "Summarize page").
pub fn uia_document_text() -> Option<String> {
    use uiautomation::patterns::UITextPattern;
    let auto = uiautomation::UIAutomation::new().ok()?;
    let mut el = auto.get_focused_element().ok()?;
    for _ in 0..12 {
        if let Ok(tp) = el.get_pattern::<UITextPattern>() {
            if let Ok(r) = tp.get_document_range() {
                if let Ok(t) = r.get_text(200_000) {
                    if t.trim().len() > 40 {
                        return Some(t);
                    }
                }
            }
        }
        let walker = auto.get_control_view_walker().ok()?;
        el = walker.get_parent(&el).ok()?;
    }
    None
}
