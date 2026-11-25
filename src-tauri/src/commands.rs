//! Tauri commands called from the web UI, plus hotkey handlers.

use crate::ai::{self, Msg, Request};
use crate::clipboard::{ClipItem, ClipStore};
use crate::prompts;
use crate::settings::{ActionDef, Settings, Workflow};
use crate::state::{AppState, Capture, HistoryItem};
use crate::windows_mgr::{self as wm, Place};
use crate::winutil;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(x: E) -> String {
    x.to_string()
}

// ================================================================ helpers

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if app.state::<AppState>().settings().notifications {
        let _ = app.notification().builder().title(title).body(body).show();
    }
    let _ = app.emit("toast", json!({"title": title, "body": body}));
}

pub fn play(app: &AppHandle, sound: &str) {
    if app.state::<AppState>().settings().sounds {
        let _ = app.emit("play-sound", sound);
    }
}

pub fn schedule_reminder(app: &AppHandle, instr: &str) {
    // "remind me in 10 minutes to call mom" / "remind me in 2h …"
    let re = regex::Regex::new(r"(?i)in\s+(\d+(?:\.\d+)?)\s*(s|sec|secs|seconds?|m|min|mins|minutes?|h|hr|hrs|hours?)\b\s*(?:to|that|about)?\s*(.*)").unwrap();
    let (secs, what) = match re.captures(instr) {
        Some(c) => {
            let n: f64 = c[1].parse().unwrap_or(10.0);
            let unit = c[2].to_lowercase();
            let mult = if unit.starts_with('h') { 3600.0 } else if unit.starts_with('s') { 1.0 } else { 60.0 };
            let what = c.get(3).map(|m| m.as_str().trim().to_string()).filter(|s| !s.is_empty());
            ((n * mult) as u64, what.unwrap_or_else(|| "Reminder".into()))
        }
        None => (600, instr.trim_start_matches(|c: char| c.is_alphabetic()).trim().to_string()),
    };
    notify(app, "Reminder set", &format!("{what} (in {} min)", (secs as f64 / 60.0).ceil()));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(secs)).await;
        use tauri_plugin_notification::NotificationExt;
        let _ = app.notification().builder().title("Keeyz Reminder").body(&what).show();
        play(&app, "done");
    });
}

fn task_for(s: &Settings, action: &ActionDef, lang: &str) -> String {
    prompts::fill(&action.prompt, if lang.is_empty() { &s.default_translate_lang } else { lang }, None)
}

/// Stream an AI answer to `win` as "ai-delta" events.
/// Each request carries the caller's id, so a window can ignore pieces of an older, still-running answer.
async fn stream_to(win: &WebviewWindow, settings: &Settings, system: String, messages: Vec<Msg>, req: u64) -> R<String> {
    let w = win.clone();
    let _ = w.emit("ai-start", req);
    ai::complete(Request { system, messages, settings, max_tokens: 4096 }, move |d| {
        let _ = w.emit("ai-delta", json!({"req": req, "text": d}));
    })
    .await
    .map_err(e)
}

fn remember_target(st: &AppState) -> isize {
    let fg = winutil::foreground();
    *st.last_hwnd.lock().unwrap() = fg;
    fg
}

// ================================================================ settings

#[tauri::command]
pub fn get_settings(st: State<AppState>) -> Settings {
    st.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, st: State<AppState>, patch: Value) -> R<Settings> {
    let new = {
        let mut cur = serde_json::to_value(st.settings()).map_err(e)?;
        if let (Some(c), Some(p)) = (cur.as_object_mut(), patch.as_object()) {
            for (k, v) in p {
                c.insert(k.clone(), v.clone());
            }
        }
        let mut s: Settings = serde_json::from_value(cur).map_err(e)?;
        s.merge_builtins();
        s
    };
    let shortcuts_changed = patch.get("shortcuts").is_some() || patch.get("custom_shortcuts").is_some();
    *st.settings.lock().unwrap() = new.clone();
    st.save_settings();
    if shortcuts_changed {
        crate::register_shortcuts(&app);
    }
    if patch.get("start_on_boot").is_some() {
        use tauri_plugin_autostart::ManagerExt;
        let al = app.autolaunch();
        let _ = if new.start_on_boot { al.enable() } else { al.disable() };
    }
    if let Some(v) = patch.get("always_on_top").and_then(|v| v.as_bool()) {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_always_on_top(v);
        }
    }
    let _ = app.emit("settings-changed", &new);
    Ok(new)
}

#[tauri::command]
pub fn reset_section(app: AppHandle, st: State<AppState>, section: String) -> R<Settings> {
    let d = Settings::default();
    let patch = match section.as_str() {
        "shortcuts" => json!({"shortcuts": d.shortcuts, "custom_shortcuts": []}),
        "trigger" => json!({"trigger_word": d.trigger_word, "typed_commands": true, "instant_run": false,
            "live_translate": false, "rewrite_whole_field": false, "double_space_action": "none", "run_spaces": 2}),
        "actions" => json!({"actions": [], "action_order": []}),
        _ => serde_json::to_value(Settings::default()).map_err(e)?,
    };
    save_settings(app, st, patch)
}

#[tauri::command]
pub fn key_status() -> Value {
    let mut m = serde_json::Map::new();
    for p in ["openai", "anthropic", "gemini", "openrouter", "custom"] {
        let k = ai::get_key(p);
        let masked = if k.len() > 8 { format!("{}…{}", &k[..4], &k[k.len() - 4..]) } else if k.is_empty() { String::new() } else { "••••".into() };
        m.insert(p.into(), json!({"set": !k.is_empty(), "masked": masked}));
    }
    Value::Object(m)
}

#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> R<()> {
    ai::set_key(&provider, &key).map_err(e)
}

#[tauri::command]
pub async fn test_connection(st: State<'_, AppState>) -> R<String> {
    let s = st.settings();
    ai::test_connection(&s).await.map_err(e)
}

#[tauri::command]
pub async fn list_models(st: State<'_, AppState>) -> R<Vec<String>> {
    let s = st.settings();
    ai::list_models(&s).await.map_err(e)
}

// ================================================================ capture / actions

#[tauri::command]
pub fn get_capture(st: State<AppState>) -> Capture {
    st.capture.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_capture(st: State<AppState>, text: String, source: Option<String>) {
    let mut c = st.capture.lock().unwrap();
    c.text = text;
    if let Some(s) = source {
        c.source = s;
    }
}

/// Open Quick Actions with text from another window (launcher, clipboard, ocr, voice, toolbar).
// Window-creating commands must be async on Windows, or WebView2 deadlocks.
#[tauri::command]
pub async fn open_actions_with(app: AppHandle, st: State<'_, AppState>, text: String, source: String, action: Option<String>) -> R<()> {
    {
        let mut c = st.capture.lock().unwrap();
        c.text = text;
        c.source = source;
        c.auto_action = action.unwrap_or_default();
        if c.hwnd == 0 {
            c.hwnd = *st.last_hwnd.lock().unwrap();
        }
    }
    wm::open_actions(&app);
    Ok(())
}

#[tauri::command]
pub async fn run_action(
    win: WebviewWindow,
    st: State<'_, AppState>,
    action: String,
    text: String,
    lang: Option<String>,
    instruction: Option<String>,
    previous: Option<String>,
    refine: Option<String>,
    req: Option<u64>,
) -> R<String> {
    let s = st.settings();
    let lang = lang.unwrap_or_default();
    let (system, user, name) = if action == "custom" || action.is_empty() {
        let instr = instruction.clone().unwrap_or_default();
        if text.trim().is_empty() {
            (prompts::GENERATE_SYSTEM.to_string(), instr.clone(), "generate".to_string())
        } else {
            (prompts::trigger_system(&s), format!("TEXT:\n<<<\n{text}\n>>>\n\nINSTRUCTION: {instr}"), "custom".to_string())
        }
    } else if action == "summarize_page" {
        (prompts::system_for(&s, prompts::SUMMARIZE_PAGE, false), prompts::wrap_text(&text), action.clone())
    } else {
        let a = s.action(&action).ok_or("Unknown action")?;
        let faithful = a.id == "fix" || a.id == "translate";
        let mut sys = prompts::system_for(&s, &task_for(&s, &a, &lang), faithful);
        if a.id != "translate" {
            sys.push_str(&prompts::language_rule(&text));
        }
        (sys, prompts::wrap_text(&text), a.id)
    };
    // Refine an earlier result (Shorter, Longer, "add a greeting"…): keep the conversation,
    // so the AI changes its last answer instead of starting over.
    let messages = match (previous, refine.as_ref()) {
        (Some(prev), Some(change)) if !prev.trim().is_empty() && !change.trim().is_empty() => vec![
            Msg::user(user.clone()),
            Msg { role: "assistant".into(), content: prev },
            Msg::user(format!("Change your last result: {change}\nReturn only the full new text, nothing else.")),
        ],
        _ => vec![Msg::user(user)],
    };
    let out = stream_to(&win, &s, system, messages, req.unwrap_or(0)).await?;
    st.record(HistoryItem { action: name, original: text, result: out.clone(), ..Default::default() });
    let _ = win.app_handle().emit("history-changed", ());
    Ok(out)
}

#[tauri::command]
pub async fn chat(win: WebviewWindow, st: State<'_, AppState>, messages: Vec<Value>, req: Option<u64>) -> R<String> {
    let s = st.settings();
    let msgs = messages
        .iter()
        .map(|m| Msg { role: m["role"].as_str().unwrap_or("user").into(), content: m["content"].as_str().unwrap_or("").into() })
        .collect();
    let mut sys = prompts::CHAT_SYSTEM.to_string();
    if s.style_enabled && !s.style_text.is_empty() {
        sys.push_str("\nUser style preferences: ");
        sys.push_str(&s.style_text);
    }
    stream_to(&win, &s, sys, msgs, req.unwrap_or(0)).await
}

#[tauri::command]
pub async fn improve_text(st: State<'_, AppState>, text: String, kind: String) -> R<String> {
    let s = st.settings();
    let sys = if kind == "prompt" { prompts::IMPROVE_PROMPT } else { prompts::IMPROVE_INSTRUCTION };
    ai::ask(&s, sys, &text).await.map_err(e)
}

/// Paste a result back into the app the text came from.
#[tauri::command]
pub async fn insert_result(app: AppHandle, win: WebviewWindow, st: State<'_, AppState>, text: String, mode: String, original: Option<String>, action: Option<String>) -> R<()> {
    let hwnd = {
        let c = st.capture.lock().unwrap();
        if c.hwnd != 0 { c.hwnd } else { *st.last_hwnd.lock().unwrap() }
    };
    let _ = win.hide();
    if win.label() != "toolbar" {
        wm::hide(&app, "toolbar");
    }
    let text2 = text.clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(120));
        winutil::focus(hwnd);
        if mode == "add" {
            // put the new text after the selection
            winutil::injecting(|| winutil::tap(windows::Win32::UI::Input::KeyboardAndMouse::VK_RIGHT.0));
            winutil::paste_text(hwnd, &format!("\n{text2}"));
        } else {
            winutil::paste_text(hwnd, &text2);
        }
    })
    .await
    .map_err(e)?;
    st.record(HistoryItem {
        action: action.unwrap_or_else(|| "insert".into()),
        original: original.unwrap_or_default(),
        result: text,
        app: winutil::app_name(hwnd),
        hwnd,
        inserted: true,
        ..Default::default()
    });
    let _ = app.emit("history-changed", ());
    play(&app, "done");
    Ok(())
}

#[tauri::command]
pub fn copy_text(text: String) -> bool {
    winutil::clip_set(&text)
}

// ================================================================ history / undo / stats

#[tauri::command]
pub fn get_history(st: State<AppState>) -> Vec<HistoryItem> {
    st.history.lock().unwrap().clone()
}

#[tauri::command]
pub fn clear_history(st: State<AppState>) {
    st.history.lock().unwrap().clear();
    let _ = std::fs::remove_file(st.dir.join("history.json"));
}

pub fn undo_last(app: &AppHandle) {
    let st = app.state::<AppState>();
    let item = {
        let mut h = st.history.lock().unwrap();
        let Some(pos) = h.iter().position(|i| i.inserted) else {
            drop(h);
            notify(app, "Nothing to undo", "No AI change to undo yet.");
            return;
        };
        h.remove(pos)
    };
    let hwnd = if item.hwnd != 0 { item.hwnd } else { winutil::foreground() };
    std::thread::spawn(move || {
        winutil::injecting(|| {
            winutil::release_modifiers();
            winutil::focus(hwnd);
            // select the inserted text (caret is normally right after it) and put the original back
            let n = item.result.replace("\r\n", "\n").chars().count();
            winutil::select_left(n);
        });
        winutil::paste_text(hwnd, &item.original);
    });
    let _ = app.emit("history-changed", ());
}

#[tauri::command]
pub fn undo(app: AppHandle) {
    undo_last(&app);
}

#[tauri::command]
pub fn get_stats(st: State<AppState>) -> Value {
    let s = st.stats.lock().unwrap().clone();
    let today = chrono::Local::now().date_naive();
    let day = |d: chrono::NaiveDate| s.days.get(&d.format("%Y-%m-%d").to_string()).copied().unwrap_or(0);
    let week: u32 = (0..7).map(|i| day(today - chrono::Duration::days(i))).sum();
    let most = s.by_action.iter().max_by_key(|(_, v)| **v).map(|(k, _)| k.clone()).unwrap_or_default();
    json!({"today": day(today), "week": week, "total": s.total, "most_used": most, "words": s.words, "chars": s.chars})
}

// ================================================================ custom actions & workflows

#[tauri::command]
pub fn save_action(app: AppHandle, st: State<AppState>, mut action: ActionDef) -> R<Settings> {
    if action.name.trim().is_empty() {
        return Err("Give the action a name.".into());
    }
    if action.prompt.trim().is_empty() {
        return Err("Add an instruction for the AI.".into());
    }
    let mut actions = st.settings().actions;
    if action.id.is_empty() {
        action.id = format!("custom_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    }
    if action.builtin {
        if let Some(orig) = crate::prompts::builtin_actions().into_iter().find(|b| b.id == action.id) {
            action.edited = action.prompt.trim() != orig.prompt || action.name.trim() != orig.name;
        }
    }
    match actions.iter_mut().find(|a| a.id == action.id) {
        Some(a) => *a = action,
        None => actions.push(action),
    }
    save_settings(app, st, json!({"actions": actions}))
}

#[tauri::command]
pub fn delete_action(app: AppHandle, st: State<AppState>, id: String) -> R<Settings> {
    let mut actions = st.settings().actions;
    actions.retain(|a| a.id != id);
    save_settings(app, st, json!({"actions": actions}))
}

#[tauri::command]
pub fn save_workflow(app: AppHandle, st: State<AppState>, mut workflow: Workflow) -> R<Settings> {
    if workflow.name.trim().is_empty() || workflow.steps.is_empty() {
        return Err("A workflow needs a name and at least one step.".into());
    }
    if workflow.id.is_empty() {
        workflow.id = format!("wf_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    }
    let mut list = st.settings().workflows;
    match list.iter_mut().find(|w| w.id == workflow.id) {
        Some(w) => *w = workflow,
        None => list.push(workflow),
    }
    save_settings(app, st, json!({"workflows": list}))
}

#[tauri::command]
pub fn delete_workflow(app: AppHandle, st: State<AppState>, id: String) -> R<Settings> {
    let mut list = st.settings().workflows;
    list.retain(|w| w.id != id);
    save_settings(app, st, json!({"workflows": list}))
}

async fn run_step(s: &Settings, step: &crate::settings::WorkflowStep, original: &str, prev: &str, all: &[(String, String)]) -> R<String> {
    let input = match step.input_source.as_str() {
        "previous_step" if !prev.is_empty() => prev.to_string(),
        "combined" => {
            let mut t = format!("ORIGINAL TEXT:\n{original}\n");
            for (label, out) in all {
                t.push_str(&format!("\n{}:\n{out}\n", label.to_uppercase()));
            }
            t
        }
        _ => original.to_string(),
    };
    let task = if step.mode == "custom" || step.mode.is_empty() {
        step.custom_prompt.clone()
    } else {
        let a = s.action(&step.mode).ok_or("Unknown step action")?;
        let mut t = task_for(s, &a, "English");
        if !step.custom_prompt.is_empty() {
            t.push_str("\nExtra instruction: ");
            t.push_str(&step.custom_prompt);
        }
        t
    };
    ai::ask(s, &prompts::system_for(s, &task, false), &prompts::wrap_text(&input)).await.map_err(e)
}

/// Runs a workflow and streams progress to the workflow window.
#[tauri::command]
pub async fn run_workflow(app: AppHandle, st: State<'_, AppState>, id: String, text: String) -> R<Value> {
    // only one workflow at a time: a second run would overwrite the first one's results
    if WF_RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("A workflow is still running. Wait for it to finish, then try again.".into());
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            WF_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _done = Done;
    let s = st.settings();
    let wf = s.workflows.iter().find(|w| w.id == id).cloned().ok_or("Workflow not found")?;
    let mut run = json!({"workflow": wf, "original": text, "results": [], "status": "running"});
    *st.workflow_run.lock().unwrap() = run.clone();
    let _ = app.emit("workflow-progress", &run);
    let mut done: Vec<(String, String)> = vec![];
    let mut prev = String::new();
    for (i, step) in wf.steps.iter().enumerate() {
        match run_step(&s, step, &text, &prev, &done).await {
            Ok(out) => {
                prev = out.clone();
                done.push((step.output_label.clone(), out.clone()));
                run["results"].as_array_mut().unwrap().push(json!({"index": i, "label": step.output_label, "text": out}));
            }
            Err(err) => {
                run["status"] = json!("error");
                run["error"] = json!(err);
                break;
            }
        }
        *st.workflow_run.lock().unwrap() = run.clone();
        let _ = app.emit("workflow-progress", &run);
    }
    if run["status"] == "running" {
        run["status"] = json!("done");
    }
    *st.workflow_run.lock().unwrap() = run.clone();
    let _ = app.emit("workflow-progress", &run);
    st.record(HistoryItem { action: format!("workflow:{}", wf.name), original: text, result: prev, ..Default::default() });
    Ok(run)
}

#[tauri::command]
pub async fn rerun_step(app: AppHandle, st: State<'_, AppState>, index: usize) -> R<Value> {
    if WF_RUNNING.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Wait until the workflow has finished, then re-run a step.".into());
    }
    let s = st.settings();
    let mut run = st.workflow_run.lock().unwrap().clone();
    let wf: Workflow = serde_json::from_value(run["workflow"].clone()).map_err(e)?;
    let original = run["original"].as_str().unwrap_or("").to_string();
    let results: Vec<(String, String)> = run["results"].as_array().cloned().unwrap_or_default().iter()
        .map(|r| (r["label"].as_str().unwrap_or("").into(), r["text"].as_str().unwrap_or("").into())).collect();
    let prev = if index > 0 { results.get(index - 1).map(|r| r.1.clone()).unwrap_or_default() } else { String::new() };
    let step = wf.steps.get(index).ok_or("No such step")?;
    let out = run_step(&s, step, &original, &prev, &results[..index.min(results.len())]).await?;
    if let Some(r) = run["results"].get_mut(index) {
        r["text"] = json!(out);
    }
    *st.workflow_run.lock().unwrap() = run.clone();
    let _ = app.emit("workflow-progress", &run);
    Ok(run)
}

#[tauri::command]
pub fn get_workflow_run(st: State<AppState>) -> Value {
    st.workflow_run.lock().unwrap().clone()
}

static WF_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The workflow the results window should run next. The window takes it exactly once,
/// so a late or repeated "workflow-start" ping can never start an old job.
static WF_PENDING: once_cell::sync::Lazy<std::sync::Mutex<Option<(String, String)>>> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

pub fn show_workflow(app: &AppHandle, id: String, text: String) {
    *WF_PENDING.lock().unwrap() = Some((id, text));
    if let Some(w) = wm::show(app, &wm::WORKFLOW, Place::Center) {
        let _ = w.emit("workflow-start", ());
        // a freshly created window may not listen yet; it also asks on load
        let w2 = w.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            let _ = w2.emit("workflow-start", ());
        });
    }
}

#[tauri::command]
pub fn take_workflow_start() -> Option<Value> {
    WF_PENDING.lock().unwrap().take().map(|(id, text)| json!({"id": id, "text": text}))
}

#[tauri::command]
pub async fn open_workflow(app: AppHandle, id: String, text: String) -> R<()> {
    show_workflow(&app, id, text);
    Ok(())
}

#[tauri::command]
pub fn export_markdown(app: AppHandle, st: State<AppState>) -> R<String> {
    let run = st.workflow_run.lock().unwrap().clone();
    let mut md = format!("# {}\n\n## Your text\n\n{}\n", run["workflow"]["name"].as_str().unwrap_or("Workflow"), run["original"].as_str().unwrap_or(""));
    for r in run["results"].as_array().cloned().unwrap_or_default() {
        md.push_str(&format!("\n## {}\n\n{}\n", r["label"].as_str().unwrap_or(""), r["text"].as_str().unwrap_or("")));
    }
    // Downloads is not covered by Windows' "Controlled folder access"; Documents often is.
    let name = format!("workflow-{}.md", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let mut dirs = vec![];
    if let Ok(d) = app.path().download_dir() {
        dirs.push(d.join("Keeyz"));
    }
    dirs.push(st.dir.join("exports"));
    let mut last = String::from("No folder to save to.");
    for dir in dirs {
        let file = dir.join(&name);
        match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, &md)) {
            Ok(_) => return Ok(file.to_string_lossy().to_string()),
            Err(err) => last = err.to_string(),
        }
    }
    Err(format!("Couldn't save the file: {last}"))
}


// ================================================================ clipboard history

#[tauri::command]
pub fn clip_list(store: State<Arc<ClipStore>>) -> Vec<ClipItem> {
    store.list()
}

#[tauri::command]
pub fn clip_remove(store: State<Arc<ClipStore>>, id: String) {
    store.remove(&id)
}

#[tauri::command]
pub fn clip_pin(store: State<Arc<ClipStore>>, id: String) {
    store.toggle_pin(&id)
}

#[tauri::command]
pub fn clip_clear(store: State<Arc<ClipStore>>) {
    store.clear()
}

#[tauri::command]
pub async fn clip_paste(app: AppHandle, st: State<'_, AppState>, store: State<'_, Arc<ClipStore>>, id: String) -> R<()> {
    let item = store.get(&id).ok_or("Item not found")?;
    let hwnd = *st.last_hwnd.lock().unwrap();
    wm::hide(&app, "clipboard");
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(100));
        if item.kind == "image" {
            if let Some((w, h, rgba)) = store.image_rgba(&item) {
                crate::clipboard::begin_own();
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_image(arboard::ImageData { width: w, height: h, bytes: rgba.into() });
                }
                winutil::injecting(|| {
                    winutil::focus(hwnd);
                    winutil::ctrl(winutil::VK_V);
                });
                std::thread::sleep(Duration::from_millis(200));
                crate::clipboard::end_own();
            }
        } else {
            winutil::focus(hwnd);
            winutil::paste_text(hwnd, &item.text);
        }
    })
    .await
    .map_err(e)
}

#[tauri::command]
pub async fn clip_ocr(store: State<'_, Arc<ClipStore>>, id: Option<String>) -> R<String> {
    let rgba = match id {
        Some(id) => {
            let item = store.get(&id).ok_or("Item not found")?;
            store.image_rgba(&item).ok_or("Image missing")?
        }
        None => {
            let mut cb = arboard::Clipboard::new().map_err(e)?;
            let img = cb.get_image().map_err(|_| "No image on the clipboard. Take a screenshot first, then try again.")?;
            (img.width, img.height, img.bytes.into_owned())
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        let (w, h, px) = rgba;
        crate::ocr::recognize(crate::ocr::rgba_to_bgra(px), w as u32, h as u32).map_err(e)
    })
    .await
    .map_err(e)?
}

// ================================================================ OCR (text from screen)

pub fn open_ocr(app: &AppHandle) {
    let Ok(w) = (if let Some(w) = app.get_webview_window("ocr") {
        Ok(w)
    } else {
        tauri::WebviewWindowBuilder::new(app, "ocr", tauri::WebviewUrl::App("ocr.html".into()))
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false)
            .build()
    }) else { return };
    // cover every monitor
    if let Ok(mons) = app.available_monitors() {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for m in mons {
            let p = m.position();
            let s = m.size();
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x + s.width as i32);
            y1 = y1.max(p.y + s.height as i32);
        }
        let _ = w.set_position(tauri::PhysicalPosition::new(x0, y0));
        let _ = w.set_size(tauri::PhysicalSize::new((x1 - x0) as u32, (y1 - y0) as u32));
    }
    let _ = w.emit("ocr-start", ());
    let _ = w.show();
    let _ = w.set_focus();
}

#[tauri::command]
pub async fn ocr_capture(win: WebviewWindow, x: f64, y: f64, w: f64, h: f64) -> R<String> {
    let scale = win.scale_factor().map_err(e)?;
    let origin = win.outer_position().map_err(e)?;
    let _ = win.hide();
    let (px, py) = (origin.x + (x * scale) as i32, origin.y + (y * scale) as i32);
    let (pw, ph) = ((w * scale) as i32, (h * scale) as i32);
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(180));
        let bgra = crate::ocr::capture(px, py, pw, ph).map_err(e)?;
        crate::ocr::recognize(bgra, pw as u32, ph as u32).map_err(e)
    })
    .await
    .map_err(e)?
}

// ================================================================ voice

#[tauri::command]
pub async fn voice_transcribe(st: State<'_, AppState>, audio: Vec<u8>, mime: String) -> R<String> {
    let s = st.settings();
    let lang = s.voice_lang.clone();
    ai::transcribe(&s, audio, &mime, &lang).await.map_err(e)
}

#[tauri::command]
pub async fn voice_polish(st: State<'_, AppState>, text: String) -> R<String> {
    let s = st.settings();
    ai::ask(&s, prompts::VOICE_POLISH, &text).await.map_err(e)
}

#[tauri::command]
pub async fn paste_into_last(app: AppHandle, win: WebviewWindow, st: State<'_, AppState>, text: String) -> R<()> {
    let hwnd = *st.last_hwnd.lock().unwrap();
    let _ = win.hide();
    let t = text.clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(120));
        winutil::focus(hwnd);
        winutil::paste_text(hwnd, &t);
    })
    .await
    .map_err(e)?;
    st.record(HistoryItem { action: "voice".into(), result: text, app: winutil::app_name(hwnd), hwnd, inserted: true, ..Default::default() });
    let _ = app.emit("history-changed", ());
    Ok(())
}

// ================================================================ windows / misc

#[tauri::command]
pub async fn open_tool_cmd(app: AppHandle, tool: String) -> R<()> {
    open_tool(app, tool);
    Ok(())
}

pub fn open_tool(app: AppHandle, tool: String) {
    // keep the app the user came from as paste target
    match tool.as_str() {
        "actions" => {
            {
                let st = app.state::<AppState>();
                let mut c = st.capture.lock().unwrap();
                c.source = "manual".into();
                c.auto_action.clear();
            }
            wm::open_actions(&app)
        }
        "launcher" => { wm::show(&app, &wm::LAUNCHER, Place::TopCenter); }
        "clipboard" => { wm::show(&app, &wm::CLIPBOARD, Place::Center); }
        "voice" => { wm::show(&app, &wm::VOICE, Place::BottomCenter).map(|w| w.emit("voice-start", ())); }
        "ocr" => open_ocr(&app),
        "settings" | "main" => wm::show_main(&app),
        "workflows" => { wm::show_main(&app); let _ = app.emit("navigate", "flows"); }
        "snap" => {
            // back to the window the user came from, then snap mode for it
            let hwnd = *app.state::<AppState>().last_hwnd.lock().unwrap();
            let a2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(120));
                winutil::focus(hwnd);
                crate::snap::start(&a2, true);
            });
        }
        _ => {}
    }
}

#[tauri::command]
pub async fn find_fastest(app: AppHandle, st: State<'_, AppState>) -> R<Value> {
    let s = st.settings();
    let r = crate::fast::benchmark(&s).await.map_err(e)?;
    let _ = app.emit("fast-updated", &r);
    Ok(json!({"ranking": r, "running": false}))
}

#[tauri::command]
pub fn fast_status() -> Value {
    json!({"ranking": crate::fast::snapshot(), "running": crate::fast::is_running(), "last": crate::fast::LAST_USED.lock().unwrap().clone()})
}

// ================================================================ updates

pub async fn find_update(app: &AppHandle) -> anyhow::Result<Value> {
    use tauri_plugin_updater::UpdaterExt;
    let current = app.package_info().version.to_string();
    match app.updater()?.check().await? {
        Some(u) => Ok(json!({"available": true, "current": current, "version": u.version, "notes": u.body.unwrap_or_default()})),
        None => Ok(json!({"available": false, "current": current})),
    }
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> R<Value> {
    find_update(&app).await.map_err(|err| format!("Couldn't check for updates: {err}"))
}

/// Download the new version, install it quietly and restart Keeyz.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> R<()> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app.updater().map_err(e)?.check().await.map_err(e)?.ok_or("You already have the newest version.")?;
    let a2 = app.clone();
    let mut got = 0usize;
    update
        .download_and_install(
            move |chunk, total| {
                got += chunk;
                let pct = total.map(|t| (got as f64 / t as f64 * 100.0) as u32).unwrap_or(0);
                let _ = a2.emit("update-progress", pct);
            },
            || {},
        )
        .await
        .map_err(|err| format!("Update failed: {err}"))?;
    app.restart();
}

/// Launcher picked an action or workflow: grab the selection from the app the user came from.
#[tauri::command]
pub fn launcher_action(app: AppHandle, st: State<AppState>, kind: String, id: String) {
    let hwnd = *st.last_hwnd.lock().unwrap();
    if kind == "action" && id == "summarize_page" {
        winutil::focus(hwnd);
        return hotkey(&app, "summarize");
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(120));
        winutil::focus(hwnd);
        let text = winutil::uia_selection().unwrap_or_else(|| winutil::copy_selection(hwnd));
        let st = app.state::<AppState>();
        let a2 = app.clone();
        if kind == "workflow" {
            *st.capture.lock().unwrap() = Capture { text: text.clone(), hwnd, app: winutil::app_name(hwnd), source: "selection".into(), ..Default::default() };
            let _ = app.run_on_main_thread(move || show_workflow(&a2, id, text));
        } else {
            *st.capture.lock().unwrap() = Capture { text, hwnd, app: winutil::app_name(hwnd), source: "selection".into(), auto_action: id, ..Default::default() };
            let _ = app.run_on_main_thread(move || wm::open_actions(&a2));
        }
    });
}

#[tauri::command]
pub fn hide_me(win: WebviewWindow) {
    let _ = win.hide();
}

#[tauri::command]
pub fn resize_me(win: WebviewWindow, w: f64, h: f64) {
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
}

#[tauri::command]
pub fn set_trigger_paused(app: AppHandle, paused: bool) {
    crate::state::TRIGGER_PAUSED.store(paused, std::sync::atomic::Ordering::SeqCst);
    let _ = app.emit("trigger-paused", paused);
}

#[tauri::command]
pub fn trigger_paused() -> bool {
    crate::state::TRIGGER_PAUSED.load(std::sync::atomic::Ordering::SeqCst)
}

#[tauri::command]
pub fn save_feedback(st: State<AppState>, text: String) -> R<String> {
    let file = st.dir.join("feedback.md");
    let mut old = std::fs::read_to_string(&file).unwrap_or_default();
    old.push_str(&format!("\n## {}\n\n{}\n", chrono::Local::now().format("%Y-%m-%d %H:%M"), text));
    std::fs::write(&file, old).map_err(e)?;
    Ok(file.to_string_lossy().to_string())
}

#[tauri::command]
pub fn app_info(app: AppHandle, st: State<AppState>) -> Value {
    json!({"version": app.package_info().version.to_string(), "data_dir": st.dir.to_string_lossy()})
}

#[tauri::command]
pub fn delete_everything(app: AppHandle, st: State<AppState>, store: State<Arc<ClipStore>>) -> R<()> {
    for p in ["openai", "anthropic", "gemini", "openrouter", "custom"] {
        let _ = ai::set_key(p, "");
    }
    store.clear();
    clear_history(st.clone());
    *st.stats.lock().unwrap() = Default::default();
    let _ = std::fs::remove_file(st.dir.join("stats.json"));
    *st.settings.lock().unwrap() = {
        let mut s = Settings::default();
        s.merge_builtins();
        s
    };
    st.save_settings();
    crate::register_shortcuts(&app);
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ================================================================ snap (window snapping)

#[tauri::command]
pub fn snap_start(app: AppHandle) {
    crate::snap::start(&app, true);
}

#[tauri::command]
pub fn snap_stop(app: AppHandle) {
    crate::snap::stop(&app);
}

/// Column picker result: 2..9, or nothing when cancelled.
#[tauri::command]
pub fn snap_pick(app: AppHandle, cols: Option<i32>) {
    crate::snap::picked(&app, cols);
}

#[tauri::command]
pub fn snap_state() -> Value {
    crate::snap::state()
}

/// Folder of the old separate Snapperr app, if it is installed.
fn snapperr_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    ["Programs\\Snapperr", "Snapperr"]
        .iter()
        .map(|p| std::path::Path::new(&base).join(p))
        .find(|d| d.join("Snapperr.exe").exists())
}

/// Is the old Snapperr app installed / running? (Both would react to Ctrl taps.)
#[tauri::command]
pub fn snapperr_status() -> Value {
    let running = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Snapperr.exe", "/NH"])
        .creation_flags_no_window()
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("snapperr.exe"))
        .unwrap_or(false);
    json!({ "installed": snapperr_dir().is_some(), "running": running })
}

#[tauri::command]
pub fn snapperr_close() {
    let _ = std::process::Command::new("taskkill").args(["/IM", "Snapperr.exe", "/F"]).creation_flags_no_window().output();
}

/// Open the old Snapperr's own uninstaller (the user confirms there).
#[tauri::command]
pub fn snapperr_uninstall() -> R<()> {
    snapperr_close();
    let dir = snapperr_dir().ok_or("Snapperr is not installed.")?;
    let un = std::fs::read_dir(&dir)
        .map_err(e)?
        .filter_map(|f| f.ok().map(|f| f.path()))
        .find(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase().starts_with("unins") && n.to_string_lossy().ends_with(".exe")).unwrap_or(false))
        .ok_or("Couldn't find Snapperr's uninstaller. Remove it in Windows Settings → Apps.")?;
    std::process::Command::new(un).spawn().map_err(e)?;
    Ok(())
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}
impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
    }
}

// ================================================================ hotkey handlers

/// "Reply from screen": read the conversation in the window you are in, write an answer and
/// put it in the reply box. Words already typed in the box steer the reply.
pub fn reply_from_screen(app: &AppHandle) {
    let st = app.state::<AppState>();
    let hwnd = remember_target(&st);
    if wm::is_own_window(app, hwnd) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let draft = winutil::focused_text().unwrap_or_default().trim().to_string();
        // 1) the page text through UI Automation, 2) otherwise read the window with OCR
        let mut convo = winutil::uia_document_text().unwrap_or_default();
        if convo.trim().len() < 40 || convo.trim() == draft {
            convo = winutil::window_rect(hwnd)
                .and_then(|(x, y, w, h)| crate::ocr::capture(x, y, w, h).ok().map(|b| (b, w, h)))
                .and_then(|(b, w, h)| crate::ocr::recognize(b, w as u32, h as u32).ok())
                .unwrap_or_default();
        }
        if convo.trim().is_empty() {
            return notify(&app, "Nothing to reply to", "Couldn't read the conversation in this window.");
        }
        // the newest messages are at the end; keep the input small so the answer comes fast
        let convo: String = {
            let c: Vec<char> = convo.chars().collect();
            c[c.len().saturating_sub(6000)..].iter().collect()
        };
        let settings = app.state::<AppState>().settings();
        let system = crate::prompts::system_for(&settings, crate::prompts::REPLY_SCREEN, false);
        let mut user = crate::prompts::wrap_text(&convo);
        if !draft.is_empty() {
            user.push_str(&format!("\n\nWhat I want to say (my notes, turn them into the reply): {draft}"));
        }
        let delete = if draft.is_empty() { 0 } else { usize::MAX };
        crate::hook::run_ai(app, hwnd, delete, system, user, draft, "reply_screen".into());
    });
}

pub fn hotkey(app: &AppHandle, id: &str) {
    let st = app.state::<AppState>();
    match id {
        "actions" => {
            let hwnd = remember_target(&st);
            if wm::is_own_window(app, hwnd) {
                return open_tool(app.clone(), "actions".into());
            }
            let app = app.clone();
            std::thread::spawn(move || {
                let text = winutil::uia_selection().unwrap_or_else(|| winutil::copy_selection(hwnd));
                let st = app.state::<AppState>();
                *st.capture.lock().unwrap() = Capture { text, hwnd, app: winutil::app_name(hwnd), source: "selection".into(), ..Default::default() };
                let a2 = app.clone();
                let _ = app.run_on_main_thread(move || wm::open_actions(&a2));
            });
        }
        "launcher" => {
            remember_target(&st);
            let visible = app.get_webview_window("launcher").and_then(|w| w.is_visible().ok()).unwrap_or(false);
            if visible { wm::hide(app, "launcher") } else { open_tool(app.clone(), "launcher".into()) }
        }
        "clipboard" => {
            remember_target(&st);
            let visible = app.get_webview_window("clipboard").and_then(|w| w.is_visible().ok()).unwrap_or(false);
            if visible { wm::hide(app, "clipboard") } else { open_tool(app.clone(), "clipboard".into()) }
        }
        "voice" => {
            let visible = app.get_webview_window("voice").and_then(|w| w.is_visible().ok()).unwrap_or(false);
            if visible {
                let _ = app.emit_to("voice", "voice-toggle", ());
            } else {
                remember_target(&st);
                open_tool(app.clone(), "voice".into());
            }
        }
        "ocr" => {
            remember_target(&st);
            open_ocr(app);
        }
        "undo" => undo_last(app),
        "reply" => reply_from_screen(app),
        "show_app" => wm::show_main(app),
        "summarize" => {
            let hwnd = remember_target(&st);
            let app = app.clone();
            std::thread::spawn(move || {
                let text = winutil::uia_document_text().unwrap_or_else(|| {
                    winutil::injecting(|| {
                        winutil::release_modifiers();
                        winutil::ctrl(winutil::VK_A);
                    });
                    winutil::copy_selection(hwnd)
                });
                if text.trim().is_empty() {
                    return notify(&app, "Nothing to summarize", "Couldn't read this window's text.");
                }
                let st = app.state::<AppState>();
                *st.capture.lock().unwrap() = Capture {
                    text: text.chars().take(60_000).collect(), hwnd, app: winutil::app_name(hwnd),
                    source: "page".into(), auto_action: "summarize_page".into(), ..Default::default()
                };
                let a2 = app.clone();
                let _ = app.run_on_main_thread(move || wm::open_actions(&a2));
            });
        }
        custom => {
            // user shortcut: "custom:<id>"
            let Some(cs) = st.settings().custom_shortcuts.into_iter().find(|c| format!("custom:{}", c.id) == custom) else { return };
            let hwnd = remember_target(&st);
            let app = app.clone();
            std::thread::spawn(move || {
                let text = winutil::uia_selection().unwrap_or_else(|| winutil::copy_selection(hwnd));
                let st = app.state::<AppState>();
                if cs.kind == "workflow" {
                    *st.capture.lock().unwrap() = Capture { text: text.clone(), hwnd, app: winutil::app_name(hwnd), source: "selection".into(), ..Default::default() };
                    let a2 = app.clone();
                    let _ = app.run_on_main_thread(move || show_workflow(&a2, cs.target.clone(), text));
                    return;
                }
                let (action, lang) = if cs.kind == "translate" { ("translate".to_string(), cs.target.clone()) } else { (cs.target.clone(), String::new()) };
                *st.capture.lock().unwrap() = Capture { text, hwnd, app: winutil::app_name(hwnd), source: "selection".into(), auto_action: action, lang };
                let a2 = app.clone();
                let _ = app.run_on_main_thread(move || wm::open_actions(&a2));
            });
        }
    }
}
