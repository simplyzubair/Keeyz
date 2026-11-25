//! Creates / shows the small popup windows (Quick Actions, Launcher, Clipboard, Voice, OCR, toolbar…).

use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub struct Spec {
    pub label: &'static str,
    pub page: &'static str,
    pub w: f64,
    pub h: f64,
    pub focus: bool,
    pub transparent: bool,
}

pub const ACTIONS: Spec = Spec { label: "actions", page: "actions.html", w: 600.0, h: 560.0, focus: true, transparent: false };
pub const LAUNCHER: Spec = Spec { label: "launcher", page: "launcher.html", w: 660.0, h: 460.0, focus: true, transparent: false };
pub const CLIPBOARD: Spec = Spec { label: "clipboard", page: "clipboard.html", w: 440.0, h: 560.0, focus: true, transparent: false };
pub const VOICE: Spec = Spec { label: "voice", page: "voice.html", w: 460.0, h: 260.0, focus: true, transparent: false };
pub const TOOLBAR: Spec = Spec { label: "toolbar", page: "toolbar.html", w: 380.0, h: 46.0, focus: false, transparent: true };
pub const TYPING: Spec = Spec { label: "typing", page: "typing.html", w: 190.0, h: 40.0, focus: false, transparent: true };
pub const SNAPBADGE: Spec = Spec { label: "snapbadge", page: "snap-badge.html", w: 132.0, h: 34.0, focus: false, transparent: true };
pub const SNAPPICKER: Spec = Spec { label: "snappicker", page: "snap-picker.html", w: 280.0, h: 452.0, focus: true, transparent: true };
pub const WORKFLOW: Spec = Spec { label: "workflow", page: "workflow-results.html", w: 640.0, h: 640.0, focus: true, transparent: false };

pub fn get_or_create(app: &AppHandle, spec: &Spec) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(spec.label) {
        return Ok(w);
    }
    let w = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.page.into()))
        .title("Keeyz")
        .inner_size(spec.w, spec.h)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .transparent(spec.transparent)
        .shadow(!spec.transparent)
        .focused(spec.focus)
        .visible(false)
        .build()?;
    Ok(w)
}

/// Monitor work-area that contains point `p` (physical px).
fn monitor_at(app: &AppHandle, p: (i32, i32)) -> Option<(i32, i32, i32, i32)> {
    let mons = app.available_monitors().ok()?;
    for m in mons {
        let pos = m.position();
        let size = m.size();
        if p.0 >= pos.x && p.0 < pos.x + size.width as i32 && p.1 >= pos.y && p.1 < pos.y + size.height as i32 {
            return Some((pos.x, pos.y, size.width as i32, size.height as i32));
        }
    }
    None
}

pub enum Place {
    /// centered horizontally, at 22% from the top of the monitor under the mouse
    TopCenter,
    /// centered on the monitor under the mouse
    Center,
    /// bottom-center of the monitor under the mouse
    BottomCenter,
    /// next to a point (physical px)
    Near(i32, i32),
}

pub fn place(app: &AppHandle, w: &WebviewWindow, how: Place) {
    let scale = w.scale_factor().unwrap_or(1.0);
    let size = w.outer_size().unwrap_or(PhysicalSize::new(600, 400));
    let cursor = crate::winutil::cursor_pos();
    let anchor = match how {
        Place::Near(x, y) => (x, y),
        _ => cursor,
    };
    let (mx, my, mw, mh) = monitor_at(app, anchor).unwrap_or((0, 0, 1920, 1080));
    let (sw, sh) = (size.width as i32, size.height as i32);
    let (mut x, mut y) = match how {
        Place::TopCenter => (mx + (mw - sw) / 2, my + (mh as f64 * 0.2) as i32),
        Place::Center => (mx + (mw - sw) / 2, my + (mh - sh) / 2),
        Place::BottomCenter => (mx + (mw - sw) / 2, my + mh - sh - (90.0 * scale) as i32),
        Place::Near(px, py) => (px - sw / 2, py + (14.0 * scale) as i32),
    };
    x = x.clamp(mx + 4, mx + mw - sw - 4);
    if y + sh > my + mh - 4 {
        y = match how {
            Place::Near(_, py) => py - sh - (20.0 * scale) as i32,
            _ => my + mh - sh - 4,
        };
    }
    y = y.max(my + 4);
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

pub fn show(app: &AppHandle, spec: &Spec, how: Place) -> Option<WebviewWindow> {
    let w = get_or_create(app, spec).ok()?;
    place(app, &w, how);
    let _ = w.show();
    if spec.focus {
        let _ = w.set_focus();
    }
    Some(w)
}

pub fn hide(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
}

pub fn is_own_window(_app: &AppHandle, hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid));
    }
    pid == std::process::id()
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

// ---------------------------------------------------------------- specific windows

pub fn show_typing(app: &AppHandle, pos: (i32, i32), label: &str) {
    let app2 = app.clone();
    let label = label.to_string();
    let _ = app.run_on_main_thread(move || {
        if let Ok(w) = get_or_create(&app2, &TYPING) {
            place(&app2, &w, Place::Near(pos.0 + 80, pos.1));
            let _ = w.set_ignore_cursor_events(true);
            let _ = w.emit("typing-label", label);
            // must not take focus: Keeyz pastes the answer into the app you're typing in
            if let Ok(h) = w.hwnd() {
                crate::winutil::show_no_activate(h.0 as isize);
            }
        }
    });
}

pub fn hide_typing(app: &AppHandle) {
    hide(app, "typing");
}

pub fn show_toolbar(app: &AppHandle, text: String, editable: bool, hwnd: isize, pos: (i32, i32)) {
    crate::hook::TOOLBAR_UP.store(true, std::sync::atomic::Ordering::SeqCst);
    {
        let st = app.state::<AppState>();
        let mut c = st.capture.lock().unwrap();
        c.text = text.clone();
        c.hwnd = hwnd;
        c.app = crate::winutil::app_name(hwnd);
        c.source = "toolbar".into();
        c.auto_action.clear();
    }
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = get_or_create(&app2, &TOOLBAR).ok() {
            let _ = w.set_size(tauri::LogicalSize::new(TOOLBAR.w, TOOLBAR.h));
            place(&app2, &w, Place::Near(pos.0, pos.1));
            let _ = w.emit("toolbar-show", serde_json::json!({ "text": text, "editable": editable }));
            // show next to the selection without stealing focus (the selection must stay active)
            match w.hwnd() {
                Ok(h) => crate::winutil::show_no_activate(h.0 as isize),
                Err(_) => {
                    let _ = w.show();
                }
            }
        }
    });
}

/// Open Quick Actions for the text currently captured in state.
pub fn open_actions(app: &AppHandle) {
    if let Some(w) = show(app, &ACTIONS, Place::TopCenter) {
        let _ = w.emit("actions-open", ());
    }
}
