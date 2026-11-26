mod ai;
mod clipboard;
mod commands;
mod fast;
mod hook;
mod ocr;
mod prompts;
mod settings;
mod snap;
mod state;
mod windows_mgr;
mod winutil;

use once_cell::sync::Lazy;
use state::AppState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// shortcut id -> our action name
static SHORTCUT_MAP: Lazy<Mutex<HashMap<u32, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// (Re)register every global shortcut from settings. Returns failures to show in the UI.
pub fn register_shortcuts(app: &AppHandle) -> Vec<String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut map = SHORTCUT_MAP.lock().unwrap();
    map.clear();
    let s = app.state::<AppState>().settings();
    let mut list: Vec<(String, String)> = s.shortcuts.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    list.extend(s.custom_shortcuts.iter().map(|c| (format!("custom:{}", c.id), c.accelerator.clone())));
    let mut failed = vec![];
    for (id, accel) in list {
        if accel.trim().is_empty() {
            continue;
        }
        match accel.parse::<Shortcut>() {
            Ok(sc) => {
                if gs.register(sc).is_ok() {
                    map.insert(sc.id(), id);
                } else {
                    failed.push(format!("{accel} is already used by another app. Pick a different combination in Settings → Shortcuts."));
                }
            }
            Err(_) => failed.push(format!("{accel} is not a valid shortcut.")),
        }
    }
    if !failed.is_empty() {
        let _ = app.emit("shortcut-errors", &failed);
    }
    failed
}

#[tauri::command]
fn reregister_shortcuts(app: AppHandle) -> Vec<String> {
    register_shortcuts(&app)
}

#[tauri::command]
fn suspend_shortcuts(app: AppHandle, suspend: bool) {
    // While the user records a new shortcut in Settings.
    if suspend {
        let _ = app.global_shortcut().unregister_all();
    } else {
        register_shortcuts(&app);
    }
}

/// Tray "Check for updates…" item; it turns into "Update to vX" once a new version is found.
static UPDATE_ITEM: once_cell::sync::OnceCell<MenuItem<tauri::Wry>> = once_cell::sync::OnceCell::new();
static UPDATE_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Show "⬆ Update to vX" in the tray menu.
pub fn tray_update_ready(version: &str) {
    if let Some(item) = UPDATE_ITEM.get() {
        UPDATE_READY.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = item.set_text(format!("⬆  Update to v{version}"));
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let version = app.package_info().version.to_string();
    let open = MenuItem::with_id(app, "open", "Open Keeyz", true, None::<&str>)?;
    let feedback = MenuItem::with_id(app, "feedback", "Send feedback", true, None::<&str>)?;
    let help = MenuItem::with_id(app, "help", "How to use", true, None::<&str>)?;
    let ver = MenuItem::with_id(app, "version", format!("Version {version}"), false, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for updates…", true, None::<&str>)?;
    let tour = MenuItem::with_id(app, "tour", "Run setup tour…", true, None::<&str>)?;
    let snap_on = MenuItem::with_id(app, "snap", "Snap a window (or double-tap Ctrl)", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause typed triggers", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(
        app,
        &[&open, &sep()?, &snap_on, &sep()?, &feedback, &help, &sep()?, &ver, &update, &sep()?, &tour, &pause, &settings, &sep()?, &quit],
    )?;
    let _ = UPDATE_ITEM.set(update.clone());
    let pause2 = pause.clone();
    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().cloned().expect("icon"))
        .tooltip("Keeyz – click to open")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            "open" => windows_mgr::show_main(app),
            "pause" => {
                let p = pause2.is_checked().unwrap_or(false);
                commands::set_trigger_paused(app.clone(), p);
            }
            "help" => {
                windows_mgr::show_main(app);
                let _ = app.emit("navigate", "help");
            }
            "feedback" => {
                windows_mgr::show_main(app);
                let _ = app.emit("navigate", "feedback");
            }
            "settings" => {
                windows_mgr::show_main(app);
                let _ = app.emit("navigate", "settings");
            }
            "snap" => snap::start(app, true),
            "tour" => {
                windows_mgr::show_main(app);
                let _ = app.emit("run-tour", ());
            }
            "update" => {
                windows_mgr::show_main(app);
                let _ = app.emit("navigate", "settings");
                if UPDATE_READY.load(std::sync::atomic::Ordering::SeqCst) {
                    let _ = app.emit("install-update", ());
                } else {
                    let _ = app.emit("check-update", ());
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                windows_mgr::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// First start after a rename: copy settings/history from the newest old data folder (old folders are left as is).
fn copy_old_data(dir: &std::path::Path) {
    let Some(parent) = dir.parent() else { return };
    if dir.join("settings.json").exists() {
        return;
    }
    let Some(old) = ["com.skillsgeek.zkey"]
        .iter()
        .map(|n| parent.join(n))
        .find(|p| p.join("settings.json").exists())
    else {
        return;
    };
    for f in ["settings.json", "history.json", "stats.json", "clipboard.json", "feedback.md"] {
        let _ = std::fs::copy(old.join(f), dir.join(f));
    }
    if let Ok(entries) = std::fs::read_dir(old.join("clips")) {
        let _ = std::fs::create_dir_all(dir.join("clips"));
        for e in entries.flatten() {
            let _ = std::fs::copy(e.path(), dir.join("clips").join(e.file_name()));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows_mgr::show_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, sc, ev| {
                    if ev.state() != ShortcutState::Pressed {
                        return;
                    }
                    let id = SHORTCUT_MAP.lock().unwrap().get(&sc.id()).cloned();
                    if let Some(id) = id {
                        commands::hotkey(app, &id);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let dir = app.path().app_config_dir().expect("config dir");
            std::fs::create_dir_all(&dir).ok();
            copy_old_data(&dir);
            fast::init(&dir);
            let st = AppState::new(dir.clone());
            let first_run = !st.settings().onboarded;
            let on_top = st.settings().always_on_top;
            let st_boot = st.settings().start_on_boot;
            app.manage(st);
            let store = Arc::new(clipboard::ClipStore::new(&dir));
            app.manage(store.clone());

            let handle = app.handle().clone();
            build_tray(&handle)?;
            register_shortcuts(&handle);
            clipboard::start_watcher(handle.clone(), store);
            hook::start(handle.clone());
            // Look for a new version shortly after start, then every hour. A new version gets a
            // small "Update now / Later" pop-up once (it stays in the tray menu after "Later").
            let h3 = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                let mut asked = String::new();
                loop {
                    if let Ok(v) = commands::find_update(&h3).await {
                        if v["available"] == true {
                            let ver = v["version"].as_str().unwrap_or_default().to_string();
                            tray_update_ready(&ver);
                            *commands::PENDING_UPDATE.lock().unwrap() = Some(v.clone());
                            let _ = h3.emit("update-available", &v);
                            if ver != asked {
                                asked = ver;
                                windows_mgr::show_update_popup(&h3, v);
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                }
            });
            // Auto mode: keep the "fastest model" list fresh (checks at start, then every 12 hours).
            let h2 = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let s = h2.state::<AppState>().settings();
                    if s.provider == "auto" && fast::stale() {
                        let _ = fast::benchmark(&s).await;
                        let _ = h2.emit("fast-updated", fast::snapshot());
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
                }
            });

            // Start with Windows: make the Windows start-up entry match the setting on every start.
            // (Only setting it on the first run missed PCs that first ran Keeyz under an older name,
            // so Keeyz - and Snap with it - was not running after a restart.)
            {
                use tauri_plugin_autostart::ManagerExt;
                let al = handle.autolaunch();
                let want = st_boot || first_run;
                let _ = if want { al.enable() } else { al.disable() };
            }
            let hidden = std::env::args().any(|a| a == "--hidden");
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_always_on_top(on_top);
                if !hidden || first_run {
                    let _ = w.show();
                }
            }
            Ok(())
        })
        .on_window_event(|win, ev| {
            if win.label() != "main" {
                return;
            }
            let app = win.app_handle();
            let st = app.state::<AppState>();
            match ev {
                WindowEvent::CloseRequested { api, .. } => {
                    let s = st.settings();
                    if s.close_behavior == "quit" {
                        app.exit(0);
                        return;
                    }
                    api.prevent_close();
                    let _ = win.hide();
                    if !s.tray_notice_shown || s.notify_on_tray {
                        commands::notify(app, "Keeyz is in the tray", "Your shortcuts keep working. Click the tray icon to open it again.");
                        st.settings.lock().unwrap().tray_notice_shown = true;
                        st.save_settings();
                    }
                }
                WindowEvent::Focused(false) => {
                    if st.settings().hide_on_blur {
                        let _ = win.hide();
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            reregister_shortcuts,
            suspend_shortcuts,
            commands::get_settings,
            commands::save_settings,
            commands::reset_section,
            commands::key_status,
            commands::set_api_key,
            commands::test_connection,
            commands::list_models,
            commands::get_capture,
            commands::set_capture,
            commands::open_actions_with,
            commands::run_action,
            commands::chat,
            commands::improve_text,
            commands::insert_result,
            commands::copy_text,
            commands::get_history,
            commands::clear_history,
            commands::undo,
            commands::get_stats,
            commands::save_action,
            commands::delete_action,
            commands::save_workflow,
            commands::delete_workflow,
            commands::run_workflow,
            commands::rerun_step,
            commands::get_workflow_run,
            commands::open_workflow,
            commands::take_workflow_start,
            commands::export_markdown,
            commands::clip_list,
            commands::clip_remove,
            commands::clip_pin,
            commands::clip_clear,
            commands::clip_paste,
            commands::clip_ocr,
            commands::ocr_capture,
            commands::voice_transcribe,
            commands::snap_start,
            commands::pending_update,
            commands::snap_stop,
            commands::snap_pick,
            commands::snap_state,
            commands::snapperr_status,
            commands::snapperr_close,
            commands::snapperr_uninstall,
            commands::voice_polish,
            commands::paste_into_last,
            commands::open_tool_cmd,
            commands::launcher_action,
            commands::find_fastest,
            commands::check_update,
            commands::install_update,
            commands::fast_status,
            commands::hide_me,
            commands::resize_me,
            commands::set_trigger_paused,
            commands::trigger_paused,
            commands::save_feedback,
            commands::app_info,
            commands::delete_everything,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Keeyz");
}
