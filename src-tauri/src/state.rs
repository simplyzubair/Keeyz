//! Shared app state.

use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

/// True while we inject keys, so the keyboard hook ignores them.
pub static INJECTING: AtomicBool = AtomicBool::new(false);
/// "Pause typed triggers" from the tray.
pub static TRIGGER_PAUSED: AtomicBool = AtomicBool::new(false);
/// Set by Esc while a typed trigger is running.
pub static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HistoryItem {
    pub id: String,
    pub time: i64,
    pub action: String,
    pub original: String,
    pub result: String,
    pub app: String,
    #[serde(skip)]
    pub hwnd: isize,
    /// true when the result was typed into an app (can be undone)
    pub inserted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Stats {
    pub days: HashMap<String, u32>,
    pub by_action: HashMap<String, u32>,
    pub total: u32,
    pub words: u64,
    pub chars: u64,
}

/// Text captured for Quick Actions / result windows.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Capture {
    pub text: String,
    pub hwnd: isize,
    pub app: String,
    /// Where the capture came from: "selection", "ocr", "voice", "clipboard", "toolbar", "launcher"
    pub source: String,
    /// Optional action to run immediately (from toolbar / custom shortcut)
    pub auto_action: String,
    pub lang: String,
}

pub struct AppState {
    pub dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub history: Mutex<Vec<HistoryItem>>,
    pub stats: Mutex<Stats>,
    pub capture: Mutex<Capture>,
    pub workflow_run: Mutex<serde_json::Value>,
    pub last_hwnd: Mutex<isize>,
}

impl AppState {
    pub fn new(dir: PathBuf) -> Self {
        let settings = Settings::load(&dir);
        let history = std::fs::read_to_string(dir.join("history.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let stats = std::fs::read_to_string(dir.join("stats.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            dir,
            settings: Mutex::new(settings),
            history: Mutex::new(history),
            stats: Mutex::new(stats),
            capture: Mutex::new(Capture::default()),
            workflow_run: Mutex::new(serde_json::Value::Null),
            last_hwnd: Mutex::new(0),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn save_settings(&self) {
        let s = self.settings.lock().unwrap().clone();
        let _ = s.save(&self.dir);
    }

    pub fn record(&self, mut item: HistoryItem) -> HistoryItem {
        item.id = uuid::Uuid::new_v4().to_string();
        item.time = chrono::Utc::now().timestamp_millis();
        {
            let mut h = self.history.lock().unwrap();
            h.insert(0, item.clone());
            h.truncate(20);
            let _ = std::fs::write(self.dir.join("history.json"), serde_json::to_string(&*h).unwrap_or_default());
        }
        {
            let mut s = self.stats.lock().unwrap();
            let day = chrono::Local::now().format("%Y-%m-%d").to_string();
            *s.days.entry(day).or_default() += 1;
            *s.by_action.entry(item.action.clone()).or_default() += 1;
            s.total += 1;
            s.words += item.result.split_whitespace().count() as u64;
            s.chars += item.result.chars().count() as u64;
            let _ = std::fs::write(self.dir.join("stats.json"), serde_json::to_string(&*s).unwrap_or_default());
        }
        item
    }
}
