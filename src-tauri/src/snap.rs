//! Snap: move and size windows with the keyboard (this used to be the separate Snapperr app).
//!
//! - Double-tap Ctrl: snap mode on for a few seconds (soft sound + small countdown badge).
//! - Triple-tap Ctrl: column picker (2-9), then snap mode.
//! - In snap mode only (so other apps keep these keys):
//!     Ctrl+Left / Down / Right   left / center / right third
//!     Ctrl+L / Ctrl+;            left / right half
//!     Ctrl+F                     full screen (work area)
//!     Ctrl+1 … Ctrl+9            column N from the left (after picking columns)
//!     Ctrl+Shift+Arrow           move 20 px (hold to keep moving)
//!     Ctrl+Shift+Alt+Arrow       resize 20 px (Right/Down bigger, Left/Up smaller)
//!   Every key press restarts the countdown. Esc or a double-tap ends it.
//!
//! The Ctrl taps are counted from the global keyboard hook (cheap bookkeeping only). While snap
//! mode is on, a second low-level keyboard hook catches the snap keys and keeps them away from
//! the app in front (like the old Snapperr did). Normal hotkeys can't be used: other apps often
//! own Ctrl+1…4 and similar, and Windows then refuses them.

use crate::state::AppState;
use crate::winutil;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const STEP: i32 = 20;
pub const MIN_SIZE: i32 = 120;
const TAP_GAP: Duration = Duration::from_millis(400);
const MAX_HOLD: Duration = Duration::from_millis(400);
const TAP_WAIT: Duration = Duration::from_millis(350);

static ON: AtomicBool = AtomicBool::new(false);
/// Switched on by hand (Snap page / tray): stays on until switched off, no countdown, and Esc is
/// left to the apps.
static STICKY: AtomicBool = AtomicBool::new(false);
static LEFT: AtomicI32 = AtomicI32::new(0);
static COLS: AtomicI32 = AtomicI32::new(0);
/// changes whenever snap mode starts/ends, so an old countdown thread stops
static RUN: AtomicU64 = AtomicU64::new(0);
/// window that was active when the column picker opened
static PICKER_TARGET: AtomicIsize = AtomicIsize::new(0);
/// (modifiers, virtual key, command) caught while snap mode is on
static ACTIVE_KEYS: Lazy<Mutex<Vec<(u8, u16, Cmd)>>> = Lazy::new(|| Mutex::new(vec![]));
/// thread that owns the snap keyboard hook (0 = no hook)
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
/// caught keys go to this worker, so the hook itself returns at once
static TX: Lazy<Mutex<Option<std::sync::mpsc::Sender<Cmd>>>> = Lazy::new(|| Mutex::new(None));

const CTRL: u8 = 1;
const SHIFT: u8 = 2;
const ALT: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    Snap(f64, f64),
    Column(i32),
    Move(i32, i32),
    Resize(i32, i32),
    Off,
}

// ------------------------------------------------------------------ Ctrl taps

#[derive(Default)]
pub struct Taps {
    count: u32,
    held: bool,
    down_at: Option<Instant>,
    other_key: bool,
    last_up: Option<Instant>,
    gen: u64,
}

static TAPS: Lazy<Mutex<Taps>> = Lazy::new(|| Mutex::new(Taps::default()));

impl Taps {
    pub fn down(&mut self, now: Instant) {
        if self.held {
            return; // key repeat while Ctrl is held
        }
        self.held = true;
        self.down_at = Some(now);
        self.other_key = false;
        self.gen += 1;
        if self.last_up.map(|u| now - u > TAP_GAP).unwrap_or(false) {
            self.count = 0;
        }
    }
    /// Returns the generation to finish after TAP_WAIT, when this release counted as a tap.
    pub fn up(&mut self, now: Instant) -> Option<u64> {
        let was_held = self.held;
        self.held = false;
        let too_long = self.down_at.map(|d| now - d > MAX_HOLD).unwrap_or(true);
        if !was_held || self.other_key || too_long {
            self.count = 0;
            return None;
        }
        self.count += 1;
        self.last_up = Some(now);
        self.gen += 1;
        Some(self.gen)
    }
    pub fn other(&mut self) {
        self.other_key = true;
        self.count = 0;
        self.gen += 1;
    }
    /// Taps counted, if `gen` is still the newest event.
    pub fn finish(&mut self, gen: u64) -> u32 {
        if gen != self.gen {
            return 0;
        }
        std::mem::take(&mut self.count)
    }
}

/// From the keyboard hook: Ctrl went down.
pub fn ctrl_down() {
    TAPS.lock().unwrap().down(Instant::now());
    crate::hook::dbg_log!("[snap] ctrl down");
}

/// From the keyboard hook: Ctrl came up. Starts a short wait to tell a double from a triple tap.
pub fn ctrl_up(app: &AppHandle) {
    let gen = TAPS.lock().unwrap().up(Instant::now());
    crate::hook::dbg_log!("[snap] ctrl up, counts as tap: {}", gen.is_some());
    let Some(gen) = gen else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(TAP_WAIT);
        let taps = TAPS.lock().unwrap().finish(gen);
        let enabled = app.state::<AppState>().settings().snap_enabled;
        crate::hook::dbg_log!("[snap] taps finished: {taps} (snap enabled: {enabled}, on: {})", is_on());
        if taps < 2 || !enabled {
            return;
        }
        if taps == 2 {
            if ON.load(Ordering::SeqCst) { stop(&app) } else { start(&app, true) }
        } else {
            show_picker(&app);
        }
    });
}

/// From the hook: any other key or a mouse button, so Ctrl+C or Ctrl+click never count as taps.
pub fn other_input() {
    TAPS.lock().unwrap().other();
}

pub fn is_on() -> bool {
    ON.load(Ordering::SeqCst)
}

// ------------------------------------------------------------------ snap mode

pub fn start(app: &AppHandle, sound: bool) {
    // a double-tap or a timed start ends "keep on" mode
    STICKY.store(false, Ordering::SeqCst);
    start_mode(app, sound);
}

/// The Snap page / tray switch: snap mode on until it is switched off again.
pub fn set_sticky(app: &AppHandle, on: bool) {
    if on {
        STICKY.store(true, Ordering::SeqCst);
        start_mode(app, true);
    } else {
        stop(app);
    }
}

fn start_mode(app: &AppHandle, sound: bool) {
    crate::hook::dbg_log!("[snap] snap mode on (keep on: {})", STICKY.load(Ordering::SeqCst));
    let secs = seconds(app);
    LEFT.store(secs, Ordering::SeqCst);
    ON.store(true, Ordering::SeqCst);
    // a new number stops any countdown that is already running (e.g. columns picked while on)
    let run = RUN.fetch_add(1, Ordering::SeqCst) + 1;
    if sound {
        chime(app, "Speech On");
    }
    register_keys(app);
    show_badge(app);
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if RUN.load(Ordering::SeqCst) != run || !ON.load(Ordering::SeqCst) {
            return;
        }
        if STICKY.load(Ordering::SeqCst) {
            continue; // switched on by hand: no countdown
        }
        let left = LEFT.fetch_sub(1, Ordering::SeqCst) - 1;
        if left <= 0 {
            stop(&app);
            return;
        }
        update_badge(&app);
    });
}

pub fn stop(app: &AppHandle) {
    if !ON.swap(false, Ordering::SeqCst) {
        return;
    }
    crate::hook::dbg_log!("[snap] snap mode off");
    STICKY.store(false, Ordering::SeqCst);
    RUN.fetch_add(1, Ordering::SeqCst);
    unregister_keys(app);
    crate::windows_mgr::hide(app, "snapbadge");
    chime(app, "Speech Off");
    let _ = app.emit("snap-state", state());
    crate::tray_snap_checked(false);
}

fn seconds(app: &AppHandle) -> i32 {
    (app.state::<AppState>().settings().snap_seconds as i32).clamp(3, 60)
}

/// Every snap key press keeps snap mode on for the full time again.
fn keep_on(app: &AppHandle) {
    if STICKY.load(Ordering::SeqCst) {
        return;
    }
    LEFT.store(seconds(app), Ordering::SeqCst);
    update_badge(app);
}

fn chime(app: &AppHandle, name: &str) {
    if !app.state::<AppState>().settings().snap_sounds {
        return;
    }
    let path = format!("{}\\Media\\{name}.wav", std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()));
    std::thread::spawn(move || winutil::play_wav(&path));
}

// ------------------------------------------------------------------ keys (only while on)

/// "Ctrl+Shift+ArrowLeft" -> (modifiers, virtual key). None for keys we can't read.
pub fn parse_accel(accel: &str) -> Option<(u8, u16)> {
    let mut mods = 0u8;
    let mut key = None;
    for part in accel.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= CTRL,
            "shift" => mods |= SHIFT,
            "alt" => mods |= ALT,
            _ => key = Some(vk_of(part)?),
        }
    }
    // a lone ";" splits oddly ("Ctrl+;" is fine, but "Ctrl++" would be "+"), so handle "+" too
    if accel.ends_with("++") {
        key = Some(0xBB);
    }
    Some((mods, key?))
}

fn vk_of(k: &str) -> Option<u16> {
    let up = k.to_ascii_uppercase();
    let b = up.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
        return Some(b[0] as u16);
    }
    if let Some(n) = up.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x6F + n);
        }
    }
    if let Some(n) = up.strip_prefix("NUMPAD").and_then(|n| n.parse::<u16>().ok()) {
        if n <= 9 {
            return Some(0x60 + n);
        }
    }
    Some(match up.as_str() {
        "ARROWLEFT" | "LEFT" => 0x25,
        "ARROWUP" | "UP" => 0x26,
        "ARROWRIGHT" | "RIGHT" => 0x27,
        "ARROWDOWN" | "DOWN" => 0x28,
        "ESCAPE" | "ESC" => 0x1B,
        "SPACE" => 0x20,
        "ENTER" => 0x0D,
        "TAB" => 0x09,
        "BACKSPACE" => 0x08,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        "INSERT" => 0x2D,
        "DELETE" => 0x2E,
        ";" => 0xBA,
        "=" => 0xBB,
        "," => 0xBC,
        "-" => 0xBD,
        "." => 0xBE,
        "/" => 0xBF,
        "`" => 0xC0,
        "[" => 0xDB,
        "\\" => 0xDC,
        "]" => 0xDD,
        "'" => 0xDE,
        _ => return None,
    })
}

/// What each snap key id does.
fn cmd_of(id: &str) -> Option<Cmd> {
    Some(match id {
        "left_third" => Cmd::Snap(0.0, 1.0 / 3.0),
        "center_third" => Cmd::Snap(1.0 / 3.0, 1.0 / 3.0),
        "right_third" => Cmd::Snap(2.0 / 3.0, 1.0 / 3.0),
        "left_half" => Cmd::Snap(0.0, 0.5),
        "right_half" => Cmd::Snap(0.5, 0.5),
        "full" => Cmd::Snap(0.0, 1.0),
        "move_left" => Cmd::Move(-STEP, 0),
        "move_right" => Cmd::Move(STEP, 0),
        "move_up" => Cmd::Move(0, -STEP),
        "move_down" => Cmd::Move(0, STEP),
        "narrower" => Cmd::Resize(-STEP, 0),
        "wider" => Cmd::Resize(STEP, 0),
        "shorter" => Cmd::Resize(0, -STEP),
        "taller" => Cmd::Resize(0, STEP),
        "off" => Cmd::Off,
        _ => return None,
    })
}

/// The keys caught in snap mode, from the user's settings (missing ones use the defaults).
pub fn key_list(cols: i32, keys: &std::collections::HashMap<String, String>) -> Vec<(u8, u16, Cmd)> {
    let defaults = crate::settings::default_snap_keys();
    let get = |id: &str| keys.get(id).filter(|v| !v.trim().is_empty()).or_else(|| defaults.get(id)).cloned().unwrap_or_default();
    let mut v = vec![];
    let mut ids: Vec<&String> = defaults.keys().collect();
    ids.sort();
    for id in ids {
        if let (Some(cmd), Some((m, vk))) = (cmd_of(id), parse_accel(&get(id))) {
            v.push((m, vk, cmd));
        }
    }
    // columns: the chosen modifiers + 1…9 (top row and number pad)
    let col_mods = parse_accel(&format!("{}+1", get("columns"))).map(|x| x.0).unwrap_or(CTRL);
    for k in 1..=cols.clamp(0, 9) {
        v.push((col_mods, 0x30 + k as u16, Cmd::Column(k)));
        v.push((col_mods, 0x60 + k as u16, Cmd::Column(k)));
    }
    v
}

fn register_keys(app: &AppHandle) {
    let mut keys = key_list(COLS.load(Ordering::SeqCst), &app.state::<AppState>().settings().snap_keys);
    if STICKY.load(Ordering::SeqCst) {
        keys.retain(|k| k.2 != Cmd::Off); // kept on by hand: Esc stays with the apps
    }
    *ACTIVE_KEYS.lock().unwrap() = keys;
    if HOOK_THREAD.load(Ordering::SeqCst) != 0 {
        return; // hook already running; it reads the new key list
    }
    let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
    *TX.lock().unwrap() = Some(tx);
    let app = app.clone();
    std::thread::spawn(move || {
        for cmd in rx {
            run_cmd(&app, cmd);
        }
    });
    std::thread::spawn(|| unsafe {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, WH_KEYBOARD_LL};
        HOOK_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
        for h in &HELD_AT {
            h.store(0, Ordering::SeqCst);
        }
        let _ = windows::Win32::System::Threading::SetThreadPriority(
            windows::Win32::System::Threading::GetCurrentThread(),
            windows::Win32::System::Threading::THREAD_PRIORITY_HIGHEST,
        );
        let module: windows::Win32::Foundation::HINSTANCE = GetModuleHandleW(None).map(|m| m.into()).unwrap_or_default();
        let Ok(hook) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), module, 0) else {
            HOOK_THREAD.store(0, Ordering::SeqCst);
            return;
        };
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        let _ = UnhookWindowsHookEx(hook);
    });
}

fn unregister_keys(_app: &AppHandle) {
    ACTIVE_KEYS.lock().unwrap().clear();
    *TX.lock().unwrap() = None; // ends the worker
    let tid = HOOK_THREAD.swap(0, Ordering::SeqCst);
    if tid != 0 {
        unsafe {
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Low-level keyboard hook, only installed in snap mode. Must return fast: it only looks the
/// key up and hands the command to the worker thread.
/// When Ctrl / Shift / Alt went down, as seen by the snap hook itself (ms since 1970, 0 = up).
/// Windows' own key state can lag a moment behind; a remembered press only counts for 1 s, so a
/// missed key-up can never leave a modifier "stuck".
static HELD_AT: [std::sync::atomic::AtomicU64; 3] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

fn held_recently(bit: u8) -> bool {
    let i = match bit { CTRL => 0, SHIFT => 1, _ => 2 };
    let at = HELD_AT[i].load(Ordering::SeqCst);
    at != 0 && crate::hook::now_ms().saturating_sub(at) < 1000
}

fn modifier_bit(vk: u32) -> u8 {
    match vk {
        0x11 | 0xA2 | 0xA3 => CTRL,
        0x10 | 0xA0 | 0xA1 => SHIFT,
        0x12 | 0xA4 | 0xA5 => ALT,
        _ => 0,
    }
}

unsafe extern "system" fn key_hook(code: i32, w: windows::Win32::Foundation::WPARAM, l: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN};
    let down = w.0 as u32 == WM_KEYDOWN || w.0 as u32 == WM_SYSKEYDOWN;
    if code == HC_ACTION as i32 {
        let k = &*(l.0 as *const KBDLLHOOKSTRUCT);
        let bit = modifier_bit(k.vkCode);
        if bit != 0 {
            let i = match bit { CTRL => 0, SHIFT => 1, _ => 2 };
            if down {
                if HELD_AT[i].load(Ordering::SeqCst) == 0 {
                    HELD_AT[i].store(crate::hook::now_ms(), Ordering::SeqCst); // first press, not key repeat
                }
            } else {
                HELD_AT[i].store(0, Ordering::SeqCst);
            }
        }
    }
    if code == HC_ACTION as i32 && down && !crate::state::INJECTING.load(Ordering::SeqCst) {
        let k = &*(l.0 as *const KBDLLHOOKSTRUCT);
        // held = seen by this hook OR reported by Windows (covers keys pressed before the hook started)
        let mods = (if winutil::is_down(0x11) || held_recently(CTRL) { CTRL } else { 0 })
            | (if winutil::is_down(0x10) || held_recently(SHIFT) { SHIFT } else { 0 })
            | (if winutil::is_down(0x12) || held_recently(ALT) { ALT } else { 0 });
        let hit = ACTIVE_KEYS.lock().ok().and_then(|keys| keys.iter().find(|(m, v, _)| *m == mods && *v as u32 == k.vkCode).map(|x| x.2));
        if let Some(cmd) = hit {
            crate::hook::dbg_log!("[snap] key caught: {cmd:?}");
            // Ctrl+key is not a Ctrl tap (the other hook never sees this key)
            if let Ok(mut t) = TAPS.lock() {
                t.other();
            }
            if let Some(tx) = TX.lock().ok().and_then(|t| t.clone()) {
                let _ = tx.send(cmd);
            }
            return LRESULT(1); // keep the key away from the app in front
        }
    }
    CallNextHookEx(None, code, w, l)
}

fn run_cmd(app: &AppHandle, cmd: Cmd) {
    if cmd == Cmd::Off {
        return stop(app);
    }
    keep_on(app);
    let Some(hwnd) = target(app) else { return };
    // holding a key repeats it by itself (Windows sends the key again and again)
    match cmd {
        Cmd::Snap(x, w) => place(hwnd, x, w),
        Cmd::Column(k) => {
            let n = COLS.load(Ordering::SeqCst).max(1);
            place(hwnd, (k - 1) as f64 / n as f64, 1.0 / n as f64)
        }
        Cmd::Move(dx, dy) => winutil::nudge_window(hwnd, dx, dy, 0, 0, MIN_SIZE),
        Cmd::Resize(dw, dh) => winutil::nudge_window(hwnd, 0, 0, dw, dh, MIN_SIZE),
        Cmd::Off => {}
    }
}

/// The window to move: the active one, unless it is one of snap's own little windows.
fn target(app: &AppHandle) -> Option<isize> {
    let fg = winutil::foreground();
    if fg == 0 {
        return None;
    }
    let _ = app;
    if ["snapbadge", "snappicker"].iter().any(|l| crate::windows_mgr::hwnd_of(l) == Some(fg)) {
        return None;
    }
    Some(fg)
}

/// Put the window at `x_ratio` of its monitor's work area, `w_ratio` wide, full height.
fn place(hwnd: isize, x_ratio: f64, w_ratio: f64) {
    winutil::restore_window(hwnd);
    let Some(work) = winutil::work_area_of(hwnd) else { return };
    let r = snap_rect(work, x_ratio, w_ratio);
    winutil::set_visible_rect(hwnd, r);
}

/// Target rectangle (x, y, w, h) inside a work area (left, top, right, bottom).
pub fn snap_rect(work: (i32, i32, i32, i32), x_ratio: f64, w_ratio: f64) -> (i32, i32, i32, i32) {
    let (l, t, r, b) = work;
    let ww = (r - l) as f64;
    let x = l + (ww * x_ratio).round() as i32;
    // end exactly where the next column starts, so columns never overlap or leave gaps
    let end = l + (ww * (x_ratio + w_ratio)).round() as i32;
    (x, t, end - x, b - t)
}

// ------------------------------------------------------------------ badge + picker windows

fn monitor_box(app: &AppHandle) -> (i32, i32, i32, i32) {
    let fg = winutil::foreground();
    winutil::work_area_of(fg)
        .or_else(|| {
            app.primary_monitor().ok().flatten().map(|m| {
                let p = m.position();
                let s = m.size();
                (p.x, p.y, p.x + s.width as i32, p.y + s.height as i32)
            })
        })
        .unwrap_or((0, 0, 1920, 1080))
}

fn badge_payload() -> serde_json::Value {
    let left = if STICKY.load(Ordering::SeqCst) { -1 } else { LEFT.load(Ordering::SeqCst) };
    serde_json::json!({ "left": left, "cols": COLS.load(Ordering::SeqCst) })
}

fn show_badge(app: &AppHandle) {
    let (l, t, r, _) = monitor_box(app);
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Ok(w) = crate::windows_mgr::get_or_create(&app2, &crate::windows_mgr::SNAPBADGE) else { return };
        let scale = w.scale_factor().unwrap_or(1.0);
        let bw = (crate::windows_mgr::SNAPBADGE.w * scale) as i32;
        let _ = w.set_position(tauri::PhysicalPosition::new((l + r - bw) / 2, t + (8.0 * scale) as i32));
        let _ = w.set_ignore_cursor_events(true);
        let _ = w.emit("snap-badge", badge_payload());
        if let Ok(h) = w.hwnd() {
            winutil::show_no_activate(h.0 as isize);
        }
    });
    let _ = app.emit("snap-state", state());
    crate::tray_snap_checked(STICKY.load(Ordering::SeqCst));
}

fn update_badge(app: &AppHandle) {
    let _ = app.emit_to("snapbadge", "snap-badge", badge_payload());
}

fn show_picker(app: &AppHandle) {
    PICKER_TARGET.store(winutil::foreground(), Ordering::SeqCst);
    chime(app, "Speech Disambiguation");
    let (l, t, r, b) = monitor_box(app);
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Ok(w) = crate::windows_mgr::get_or_create(&app2, &crate::windows_mgr::SNAPPICKER) else { return };
        let scale = w.scale_factor().unwrap_or(1.0);
        let pw = (crate::windows_mgr::SNAPPICKER.w * scale) as i32;
        let ph = (crate::windows_mgr::SNAPPICKER.h * scale) as i32;
        let _ = w.set_position(tauri::PhysicalPosition::new((l + r - pw) / 2, (t + b - ph) / 2));
        let _ = w.emit("snap-picker", 4);
        let _ = w.show();
        let _ = w.set_focus();
    });
}

/// Column picker closed: `cols` = 2..9 picked, None = cancelled.
pub fn picked(app: &AppHandle, cols: Option<i32>) {
    crate::windows_mgr::hide(app, "snappicker");
    let back = PICKER_TARGET.swap(0, Ordering::SeqCst);
    if back != 0 {
        winutil::focus(back);
    }
    if let Some(n) = cols {
        COLS.store(n.clamp(2, 9), Ordering::SeqCst);
        start(app, false); // the triple-tap sound already played
    }
}

/// For the Snap page: is snap mode on and how many columns.
pub fn state() -> serde_json::Value {
    serde_json::json!({ "on": is_on(), "sticky": STICKY.load(Ordering::SeqCst), "left": LEFT.load(Ordering::SeqCst), "cols": COLS.load(Ordering::SeqCst) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirds_halves_and_full() {
        let work = (0, 0, 1920, 1040);
        assert_eq!(snap_rect(work, 0.0, 1.0 / 3.0), (0, 0, 640, 1040));
        assert_eq!(snap_rect(work, 1.0 / 3.0, 1.0 / 3.0), (640, 0, 640, 1040));
        assert_eq!(snap_rect(work, 2.0 / 3.0, 1.0 / 3.0), (1280, 0, 640, 1040));
        assert_eq!(snap_rect(work, 0.5, 0.5), (960, 0, 960, 1040));
        assert_eq!(snap_rect(work, 0.0, 1.0), (0, 0, 1920, 1040));
    }

    #[test]
    fn columns_fill_the_screen_without_gaps() {
        let work = (-1366, 30, 0, 768); // second monitor left of the main one, taskbar on top
        for n in 2..=9 {
            let mut next = -1366;
            for k in 1..=n {
                let (x, y, w, h) = snap_rect(work, (k - 1) as f64 / n as f64, 1.0 / n as f64);
                assert_eq!(x, next, "column {k} of {n} starts where the last ended");
                assert_eq!((y, h), (30, 738));
                next = x + w;
            }
            assert_eq!(next, 0, "last of {n} columns ends at the right edge");
        }
    }

    #[test]
    fn ctrl_taps() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        // double tap
        let mut t = Taps::default();
        t.down(ms(0));
        assert!(t.up(ms(80)).is_some());
        t.down(ms(200));
        let g = t.up(ms(260)).unwrap();
        assert_eq!(t.finish(g), 2);
        // Ctrl+C is not a tap
        let mut t = Taps::default();
        t.down(ms(0));
        t.other();
        assert!(t.up(ms(80)).is_none());
        // holding Ctrl too long is not a tap
        let mut t = Taps::default();
        t.down(ms(0));
        assert!(t.up(ms(600)).is_none());
        // a slow second tap starts counting again
        let mut t = Taps::default();
        t.down(ms(0));
        t.up(ms(50));
        t.down(ms(900));
        let g = t.up(ms(950)).unwrap();
        assert_eq!(t.finish(g), 1);
        // triple tap
        let mut t = Taps::default();
        let mut g = 0;
        for i in 0..3 {
            t.down(ms(i * 200));
            g = t.up(ms(i * 200 + 60)).unwrap();
        }
        assert_eq!(t.finish(g), 3);
        // an old timer does nothing once a newer tap happened
        let mut t = Taps::default();
        t.down(ms(0));
        let old = t.up(ms(50)).unwrap();
        t.down(ms(150));
        assert_eq!(t.finish(old), 0);
    }

    /// Moves a hidden test window of our own (never the user's windows).
    #[test]
    fn moves_and_resizes_a_real_window() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW};
        let h = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("KeeyzSnapTest"), WS_OVERLAPPEDWINDOW, 300, 200, 500, 400, None, None, None, None)
        }
        .expect("test window");
        let hwnd = h.0 as isize;
        let pos = || winutil::window_rect(hwnd).unwrap();

        let (x, y, w, hh) = pos();
        winutil::nudge_window(hwnd, STEP, 0, 0, 0, MIN_SIZE);
        assert_eq!(pos(), (x + 20, y, w, hh), "move right 20 px, size same");
        winutil::nudge_window(hwnd, 0, 0, STEP, -STEP, MIN_SIZE);
        assert_eq!(pos(), (x + 20, y, w + 20, hh - 20), "resize, place same");
        winutil::nudge_window(hwnd, 0, 0, -5000, -5000, MIN_SIZE);
        // Windows may keep it a little bigger (its own minimum width), never smaller
        assert!(pos().2 >= MIN_SIZE && pos().3 >= MIN_SIZE && pos().2 < MIN_SIZE + 40, "never smaller than the minimum: {:?}", pos());

        let work = winutil::work_area_of(hwnd).expect("work area");
        place(hwnd, 0.0, 0.5);
        let (px, py, pw, ph) = pos();
        let (l, t, r, b) = work;
        assert!((px - l).abs() <= 16 && (py - t).abs() <= 16, "left half starts at the corner: {px},{py} vs {l},{t}");
        assert!((pw - (r - l) / 2).abs() <= 32 && (ph - (b - t)).abs() <= 32, "half wide, full height: {pw}x{ph}");
        unsafe { let _ = DestroyWindow(h); }
    }

    #[test]
    fn default_keys_and_custom_keys() {
        let d = crate::settings::default_snap_keys();
        let keys = key_list(0, &d);
        let find = |m: u8, vk: u16| keys.iter().find(|k| k.0 == m && k.1 == vk).map(|k| k.2);
        assert_eq!(find(CTRL, 0x25), Some(Cmd::Snap(0.0, 1.0 / 3.0)), "Ctrl+Left");
        assert_eq!(find(CTRL, 0xBA), Some(Cmd::Snap(0.5, 0.5)), "Ctrl+;");
        assert_eq!(find(CTRL, 0x46), Some(Cmd::Snap(0.0, 1.0)), "Ctrl+F");
        assert_eq!(find(CTRL | SHIFT, 0x27), Some(Cmd::Move(STEP, 0)));
        assert_eq!(find(CTRL | SHIFT | ALT, 0x25), Some(Cmd::Resize(-STEP, 0)));
        assert_eq!(find(0, 0x1B), Some(Cmd::Off), "Esc");
        assert_eq!(keys.len(), 15);
        // the user changes left half to Alt+1 and columns to Alt
        let mut mine = d.clone();
        mine.insert("left_half".into(), "Alt+1".into());
        mine.insert("columns".into(), "Alt".into());
        let keys = key_list(3, &mine);
        assert!(keys.iter().any(|k| k.0 == ALT && k.1 == 0x31 && k.2 == Cmd::Snap(0.0, 0.5)));
        assert!(!keys.iter().any(|k| k.0 == CTRL && k.1 == 0x4C), "old Ctrl+L is gone");
        assert!(keys.iter().any(|k| k.0 == ALT && k.1 == 0x32 && k.2 == Cmd::Column(2)));
        // parsing
        assert_eq!(parse_accel("Ctrl+Shift+ArrowUp"), Some((CTRL | SHIFT, 0x26)));
        assert_eq!(parse_accel("Alt+F5"), Some((ALT, 0x74)));
        assert_eq!(parse_accel("Ctrl+'"), Some((CTRL, 0xDE)));
        assert_eq!(parse_accel("Ctrl+Whatever"), None);
    }

    #[test]
    fn columns_register_only_what_was_picked() {
        let none = std::collections::HashMap::new();
        let cols = |n| key_list(n, &none).iter().filter(|(_, _, c)| matches!(c, Cmd::Column(_))).count();
        assert_eq!(cols(0), 0);
        assert_eq!(cols(4), 8, "top row + number pad");
        assert_eq!(cols(9), 18);
        // every key + modifier combination means one thing only
        let keys = key_list(9, &none);
        for (i, a) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].iter().any(|b| a.0 == b.0 && a.1 == b.1), "duplicate key {a:?}");
        }
    }
}
