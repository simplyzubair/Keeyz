//! Global keyboard + mouse hook.
//! - Typed trigger: "your text Keeyz make it formal" + space space → rewritten in place.
//! - Typed commands: /calc, /search, /yt, /maps, user commands.
//! - Live translate / double-space action.
//! - Selection toolbar: shows a small toolbar after you select text with the mouse.

use crate::state::{AppState, CANCEL, INJECTING, TRIGGER_PAUSED};
use crate::winutil;
use once_cell::sync::{Lazy, OnceCell};
use rdev::{Button, EventType, Key};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

static APP: OnceCell<AppHandle> = OnceCell::new();

struct Typed {
    buf: String,
    /// window the buffer belongs to; switching windows clears it
    hwnd: isize,
    spaces: u8,
    last_key: Instant,
    ctrl: bool,
    alt: bool,
    win: bool,
    busy: bool,
    // mouse
    down_at: Option<((i32, i32), Instant)>,
    last_click: Option<((i32, i32), Instant)>,
}

static T: Lazy<Mutex<Typed>> = Lazy::new(|| {
    Mutex::new(Typed {
        buf: String::new(),
        hwnd: 0,
        spaces: 0,
        last_key: Instant::now(),
        ctrl: false,
        alt: false,
        win: false,
        busy: false,
        down_at: None,
        last_click: None,
    })
});

pub fn start(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| {
        if let Err(e) = rdev::listen(on_event) {
            eprintln!("[hook] failed: {e:?}");
        }
    });
    // "Run on pause" poller
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(300));
        let Some(app) = APP.get() else { continue };
        let s = app.state::<AppState>().settings();
        if !s.instant_run || !s.typed_commands || TRIGGER_PAUSED.load(Ordering::SeqCst) {
            continue;
        }
        let snapshot = {
            let t = T.lock().unwrap();
            if t.busy || t.buf.is_empty() || t.last_key.elapsed() < Duration::from_millis(1800) {
                continue;
            }
            t.buf.clone()
        };
        if find_trigger(&snapshot, &s.trigger_word).is_some() {
            T.lock().unwrap().busy = true;
            std::thread::spawn(move || evaluate(snapshot));
        }
    });
}

fn reset(t: &mut Typed) {
    t.buf.clear();
    t.spaces = 0;
}

fn on_event(ev: rdev::Event) {
    if INJECTING.load(Ordering::SeqCst) {
        return;
    }
    match ev.event_type {
        EventType::KeyPress(k) => on_key(k, ev.name),
        EventType::KeyRelease(k) => {
            let mut t = T.lock().unwrap();
            match k {
                Key::ControlLeft | Key::ControlRight => t.ctrl = false,
                Key::Alt | Key::AltGr => t.alt = false,
                Key::MetaLeft | Key::MetaRight => t.win = false,
                _ => {}
            }
        }
        EventType::ButtonPress(Button::Left) => {
            let p = winutil::cursor_pos();
            let mut t = T.lock().unwrap();
            reset(&mut t);
            t.down_at = Some((p, Instant::now()));
            drop(t);
            if let Some(app) = APP.get() {
                let _ = app.emit("global-click", serde_json::json!({"x": p.0, "y": p.1}));
            }
        }
        EventType::ButtonPress(_) => reset(&mut T.lock().unwrap()),
        EventType::ButtonRelease(Button::Left) => on_mouse_up(),
        _ => {}
    }
}

fn on_key(k: Key, name: Option<String>) {
    let mut t = T.lock().unwrap();
    t.last_key = Instant::now();
    let fg = winutil::foreground();
    if fg != t.hwnd {
        t.hwnd = fg;
        reset(&mut t);
    }
    match k {
        Key::ControlLeft | Key::ControlRight => { t.ctrl = true; return; }
        Key::Alt | Key::AltGr => { t.alt = true; return; }
        Key::MetaLeft | Key::MetaRight => { t.win = true; return; }
        Key::ShiftLeft | Key::ShiftRight | Key::CapsLock => return,
        Key::Escape => {
            CANCEL.store(true, Ordering::SeqCst);
            reset(&mut t);
            if let Some(app) = APP.get() {
                let _ = app.emit("escape-pressed", ());
            }
            return;
        }
        Key::Backspace => {
            if t.ctrl { reset(&mut t); } else { t.buf.pop(); t.spaces = 0; }
            return;
        }
        Key::Return | Key::KpReturn | Key::Tab | Key::UpArrow | Key::DownArrow | Key::LeftArrow
        | Key::RightArrow | Key::Home | Key::End | Key::PageUp | Key::PageDown | Key::Delete => {
            reset(&mut t);
            return;
        }
        _ => {}
    }
    if t.ctrl || t.alt || t.win || t.busy {
        if t.ctrl || t.win { reset(&mut t); }
        return;
    }
    let Some(s) = name else { return };
    if s.is_empty() || s.chars().any(|c| c.is_control()) {
        return;
    }
    t.buf.push_str(&s);
    if t.buf.chars().count() > 4000 {
        let skip = t.buf.chars().count() - 4000;
        t.buf = t.buf.chars().skip(skip).collect();
    }
    if s == " " {
        t.spaces += 1;
    } else {
        t.spaces = 0;
        return;
    }
    let Some(app) = APP.get() else { return };
    let settings = app.state::<AppState>().settings();
    if t.spaces >= settings.run_spaces && !TRIGGER_PAUSED.load(Ordering::SeqCst) {
        let snapshot = t.buf.clone();
        let interesting = (settings.typed_commands
            && (find_trigger(&snapshot, &settings.trigger_word).is_some() || find_command(&snapshot, &settings).is_some()))
            || settings.live_translate
            || settings.double_space_action != "none";
        if interesting {
            t.busy = true;
            drop(t);
            std::thread::spawn(move || evaluate(snapshot));
        }
    }
}

fn on_mouse_up() {
    let p = winutil::cursor_pos();
    let (dragged, dbl) = {
        let mut t = T.lock().unwrap();
        let dragged = t
            .down_at
            .map(|(d, _)| (d.0 - p.0).abs() > 6 || (d.1 - p.1).abs() > 6)
            .unwrap_or(false);
        let dbl = t
            .last_click
            .map(|(c, at)| at.elapsed() < Duration::from_millis(450) && (c.0 - p.0).abs() < 6 && (c.1 - p.1).abs() < 6)
            .unwrap_or(false);
        t.last_click = Some((p, Instant::now()));
        (dragged, dbl)
    };
    if !dragged && !dbl {
        return;
    }
    let Some(app) = APP.get().cloned() else { return };
    if !app.state::<AppState>().settings().selection_toolbar {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(140));
        let fg = winutil::foreground();
        if crate::windows_mgr::is_own_window(&app, fg) {
            return;
        }
        if let Some(text) = winutil::uia_selection() {
            if text.chars().count() >= 2 {
                crate::windows_mgr::show_toolbar(&app, text, fg, p);
            }
        }
    });
}

// ------------------------------------------------------------------ parsing

/// Returns (text before trigger, instruction, byte index where trigger starts).
pub fn find_trigger(buf: &str, word: &str) -> Option<(String, String)> {
    let body = buf.trim_end();
    // Words match in any case (keeyz, Keeyz). A one-letter trigger stays case-sensitive so normal typing doesn't fire it.
    let flags = if word.trim().chars().count() > 1 { "(?i)" } else { "" };
    let re = regex::Regex::new(&format!(r"{flags}(?:^|\s){}[\s,:]+(\S.*)$", regex::escape(word.trim()))).ok()?;
    let caps = re.captures_iter(body).last()?;
    let m = caps.get(0)?;
    let before = body[..m.start()].trim_end().to_string();
    let instr = caps.get(1)?.as_str().trim().to_string();
    (!instr.is_empty()).then_some((before, instr))
}

enum Cmd {
    Calc(String, usize),
    Url(String, usize),
    Custom { prompt: String, input: String },
}

/// Utility + user commands. Usize = number of chars belonging to the command (to delete).
fn find_command(buf: &str, s: &crate::settings::Settings) -> Option<Cmd> {
    let body = buf.trim_end();
    let util = regex::Regex::new(r"(?i)(?:^|\s)(/calc|/search|/yt|/maps)\s+(\S.*)$").unwrap();
    if let Some(c) = util.captures(body) {
        let whole = c.get(0).unwrap();
        let start = whole.start() + whole.as_str().len() - whole.as_str().trim_start().len();
        let n = body[start..].chars().count();
        let arg = c.get(2).unwrap().as_str().trim().to_string();
        let enc = urlencoding::encode(&arg).to_string();
        return Some(match c.get(1).unwrap().as_str().to_lowercase().as_str() {
            "/calc" => Cmd::Calc(arg, n),
            "/search" => Cmd::Url(format!("https://www.google.com/search?q={enc}"), n),
            "/yt" => Cmd::Url(format!("https://www.youtube.com/results?search_query={enc}"), n),
            _ => Cmd::Url(format!("https://www.google.com/maps/search/{enc}"), n),
        });
    }
    for c in &s.custom_commands {
        let pat = c.pattern.trim();
        if pat.len() < 2 {
            continue;
        }
        if c.scope == "after" {
            if let Some(pos) = body.to_lowercase().rfind(&pat.to_lowercase()) {
                let input = body[pos + pat.len()..].trim().to_string();
                if !input.is_empty() {
                    return Some(Cmd::Custom { prompt: c.prompt.clone(), input });
                }
            }
        } else if body.to_lowercase().ends_with(&pat.to_lowercase()) {
            let input = body[..body.len() - pat.len()].trim().to_string();
            if !input.is_empty() {
                return Some(Cmd::Custom { prompt: c.prompt.clone(), input });
            }
        }
    }
    None
}

// ------------------------------------------------------------------ running

fn done() {
    let mut t = T.lock().unwrap();
    t.busy = false;
    reset(&mut t);
}

fn evaluate(buf: String) {
    let Some(app) = APP.get().cloned() else { return done() };
    let settings = app.state::<AppState>().settings();
    let total = buf.chars().count();
    let hwnd = winutil::foreground();
    if crate::windows_mgr::is_own_window(&app, hwnd) {
        return done();
    }

    if settings.typed_commands {
        if let Some(cmd) = find_command(&buf, &settings) {
            let trailing = buf.chars().count() - buf.trim_end().chars().count();
            match cmd {
                Cmd::Calc(expr, n) => {
                    let expr2 = expr.replace('x', "*").replace('×', "*").replace('÷', "/").replace(',', "");
                    let out = match meval::eval_str(&expr2) {
                        Ok(v) => format_num(v),
                        Err(_) => return done(),
                    };
                    winutil::injecting(|| {
                        winutil::release_modifiers();
                        winutil::backspaces(n + trailing)
                    });
                    winutil::paste_text(hwnd, &out);
                    return done();
                }
                Cmd::Url(url, n) => {
                    winutil::injecting(|| {
                        winutil::release_modifiers();
                        winutil::backspaces(n + trailing)
                    });
                    let _ = tauri_plugin_opener::open_url(url, None::<&str>);
                    return done();
                }
                Cmd::Custom { prompt, input } => {
                    let sys = crate::prompts::system_for(&settings, &prompt, false);
                    return run_ai(app, hwnd, total, sys, input.clone(), input, "custom_command".into());
                }
            }
        }
        if let Some((before, instr)) = find_trigger(&buf, &settings.trigger_word) {
            let lower = instr.to_lowercase();
            if lower.starts_with("remind") {
                crate::commands::schedule_reminder(&app, &instr);
                let keep = before.chars().count();
                winutil::injecting(|| winutil::backspaces(total - keep));
                return done();
            }
            if before.is_empty() {
                return run_ai(app, hwnd, total, crate::prompts::GENERATE_SYSTEM.into(), instr.clone(), instr, "generate".into());
            }
            let mut text = before.clone();
            let mut delete = total;
            if settings.rewrite_whole_field {
                // Grab the whole field: select all + copy
                winutil::injecting(|| {
                    winutil::ctrl(winutil::VK_A);
                    std::thread::sleep(Duration::from_millis(60));
                });
                let copied = winutil::copy_selection(hwnd);
                if let Some((before_trigger, _)) = find_trigger(&copied, &settings.trigger_word) {
                    text = before_trigger;
                    delete = usize::MAX; // signals "replace selection"
                }
            }
            let user = format!("TEXT:\n<<<\n{text}\n>>>\n\nINSTRUCTION: {instr}");
            return run_ai(app, hwnd, delete, crate::prompts::trigger_system(&settings), user, text, instr);
        }
    }
    let text = buf.trim().to_string();
    if text.chars().count() < 2 {
        return done();
    }
    if settings.live_translate {
        let task = crate::prompts::fill("Translate the text into {lang}. Output only the translation.", &settings.live_translate_lang, None);
        let sys = crate::prompts::system_for(&settings, &task, true);
        return run_ai(app, hwnd, total, sys, text.clone(), text, "translate".into());
    }
    if settings.double_space_action != "none" {
        if let Some(a) = settings.action(&settings.double_space_action) {
            let task = crate::prompts::fill(&a.prompt, &settings.default_translate_lang, None);
            let sys = crate::prompts::system_for(&settings, &task, a.id == "fix" || a.id == "translate");
            return run_ai(app, hwnd, total, sys, text.clone(), text, a.id);
        }
    }
    done()
}

fn format_num(v: f64) -> String {
    if (v.fract()).abs() < 1e-9 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.6}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Call the AI, then delete `delete` typed chars (usize::MAX = replace selection) and paste the result.
fn run_ai(app: AppHandle, hwnd: isize, delete: usize, system: String, user: String, original: String, action: String) {
    CANCEL.store(false, Ordering::SeqCst);
    let pos = winutil::caret_pos().unwrap_or_else(winutil::cursor_pos);
    crate::windows_mgr::show_typing(&app, pos, "Working…");
    tauri::async_runtime::spawn(async move {
        let settings = app.state::<AppState>().settings();
        let res = crate::ai::ask(&settings, &system, &user).await;
        crate::windows_mgr::hide_typing(&app);
        if CANCEL.load(Ordering::SeqCst) {
            return done();
        }
        match res {
            Ok(out) => {
                if winutil::foreground() != hwnd {
                    winutil::clip_set(&out);
                    crate::commands::notify(&app, "Result copied", "You switched windows, so the result is on your clipboard.");
                } else {
                    if delete == usize::MAX {
                        // whole field already selected by copy step: re-select and paste
                        winutil::injecting(|| winutil::ctrl(winutil::VK_A));
                    } else {
                        winutil::injecting(|| {
                            winutil::release_modifiers();
                            winutil::backspaces(delete)
                        });
                    }
                    winutil::paste_text(hwnd, &out);
                    let st = app.state::<AppState>();
                    st.record(crate::state::HistoryItem {
                        action: action.clone(),
                        original: original.clone(),
                        result: out.clone(),
                        app: winutil::app_name(hwnd),
                        hwnd,
                        inserted: true,
                        ..Default::default()
                    });
                    let _ = app.emit("history-changed", ());
                    crate::commands::play(&app, "done");
                }
            }
            Err(e) => crate::commands::notify(&app, "Couldn't do that", &e.to_string()),
        }
        done();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd_len(buf: &str) -> Option<(String, usize)> {
        let s = crate::settings::Settings::default();
        match find_command(buf, &s)? {
            Cmd::Calc(expr, n) => Some((expr, n)),
            _ => None,
        }
    }

    #[test]
    fn calc_deletes_only_the_command() {
        assert_eq!(cmd_len("abc /calc 2+2  "), Some(("2+2".into(), 9)));
        assert_eq!(cmd_len("the total is /calc 12*8+4  "), Some(("12*8+4".into(), 12)));
        assert_eq!(cmd_len("/calc 5*5  "), Some(("5*5".into(), 9)));
    }

    #[test]
    fn trigger_word_matching() {
        assert_eq!(find_trigger("make this nice Keeyz make it formal  ", "Keeyz").map(|t| t.1), Some("make it formal".into()));
        assert_eq!(find_trigger("hello keeyz fix grammar", "Keeyz").map(|t| t.0), Some("hello".into()));
        assert!(find_trigger("the keeyzz thing", "Keeyz").is_none());
        assert!(find_trigger("ok k thanks", "K").is_none());
    }
}
