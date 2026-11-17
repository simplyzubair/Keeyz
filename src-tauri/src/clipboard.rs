//! Clipboard history: watches the clipboard and keeps recent + pinned items (text and images).

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const MAX_ITEMS: usize = 200;

/// While Keeyz itself uses the clipboard (copy selection / paste result), the watcher ignores it.
static BUSY: AtomicU32 = AtomicU32::new(0);
static IGNORE_UNTIL: AtomicU32 = AtomicU32::new(0);

pub fn begin_own() {
    BUSY.fetch_add(1, Ordering::SeqCst);
}

pub fn end_own() {
    IGNORE_UNTIL.store(crate::winutil::clip_seq(), Ordering::SeqCst);
    BUSY.fetch_sub(1, Ordering::SeqCst);
}

/// True for text that looks like a password, API key or token. These are never stored.
pub fn looks_secret(t: &str) -> bool {
    let t = t.trim();
    if t.len() < 16 || t.len() > 400 || t.chars().any(char::is_whitespace) {
        return false;
    }
    const PREFIXES: [&str; 14] = [
        "sk-", "sk_", "AIza", "AQ.", "ghp_", "gho_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "glpat-", "eyJ", "pk_", "rk_",
    ];
    if PREFIXES.iter().any(|p| t.starts_with(p)) {
        return true;
    }
    // Long random-looking token: only letters, digits and a few symbols, with mixed case and digits.
    let ok_chars = t.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+/=".contains(c));
    let up = t.chars().any(|c| c.is_ascii_uppercase());
    let low = t.chars().any(|c| c.is_ascii_lowercase());
    let dig = t.chars().any(|c| c.is_ascii_digit());
    ok_chars && up && low && dig && t.len() >= 24
}

/// Password managers mark copies they don't want recorded (Windows convention).
fn excluded_by_source() -> bool {
    use windows::core::w;
    use windows::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW};
    unsafe {
        let ex = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let hist = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        IsClipboardFormatAvailable(ex).is_ok() || IsClipboardFormatAvailable(hist).is_ok()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ClipItem {
    pub id: String,
    pub time: i64,
    pub kind: String, // "text" | "image"
    pub text: String,
    /// PNG file name inside the clips folder (images only)
    pub file: String,
    /// small PNG preview as data URL (images only)
    pub thumb: String,
    pub pinned: bool,
}

pub struct ClipStore {
    pub dir: PathBuf,
    pub items: Mutex<Vec<ClipItem>>,
}

impl ClipStore {
    pub fn new(app_dir: &PathBuf) -> Self {
        let dir = app_dir.join("clips");
        let _ = std::fs::create_dir_all(&dir);
        let mut items: Vec<ClipItem> = std::fs::read_to_string(app_dir.join("clipboard.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // Drop secrets that older versions recorded.
        let before = items.len();
        items.retain(|i| !(i.kind == "text" && looks_secret(&i.text)));
        let changed = items.len() != before;
        let store = Self { dir, items: Mutex::new(items) };
        if changed {
            let snapshot = store.items.lock().unwrap().clone();
            store.save(&snapshot);
        }
        store
    }

    fn save(&self, items: &Vec<ClipItem>) {
        if let Some(parent) = self.dir.parent() {
            let _ = std::fs::write(parent.join("clipboard.json"), serde_json::to_string(items).unwrap_or_default());
        }
    }

    pub fn list(&self) -> Vec<ClipItem> {
        self.items.lock().unwrap().clone()
    }

    pub fn push(&self, mut item: ClipItem) -> bool {
        let mut items = self.items.lock().unwrap();
        if item.kind == "text" {
            if item.text.trim().is_empty() || looks_secret(&item.text) {
                return false;
            }
            if let Some(pos) = items.iter().position(|i| i.kind == "text" && i.text == item.text) {
                let old = items.remove(pos);
                item.pinned = old.pinned;
            }
        }
        item.id = uuid::Uuid::new_v4().to_string();
        item.time = chrono::Utc::now().timestamp_millis();
        items.insert(0, item);
        // drop the oldest unpinned items
        while items.len() > MAX_ITEMS {
            if let Some(pos) = items.iter().rposition(|i| !i.pinned) {
                let gone = items.remove(pos);
                if !gone.file.is_empty() {
                    let _ = std::fs::remove_file(self.dir.join(&gone.file));
                }
            } else {
                break;
            }
        }
        self.save(&items);
        true
    }

    pub fn remove(&self, id: &str) {
        let mut items = self.items.lock().unwrap();
        if let Some(pos) = items.iter().position(|i| i.id == id) {
            let gone = items.remove(pos);
            if !gone.file.is_empty() {
                let _ = std::fs::remove_file(self.dir.join(&gone.file));
            }
        }
        self.save(&items);
    }

    pub fn toggle_pin(&self, id: &str) {
        let mut items = self.items.lock().unwrap();
        if let Some(i) = items.iter_mut().find(|i| i.id == id) {
            i.pinned = !i.pinned;
        }
        self.save(&items);
    }

    pub fn clear(&self) {
        let mut items = self.items.lock().unwrap();
        items.retain(|i| i.pinned);
        self.save(&items);
    }

    pub fn get(&self, id: &str) -> Option<ClipItem> {
        self.items.lock().unwrap().iter().find(|i| i.id == id).cloned()
    }

    pub fn image_rgba(&self, item: &ClipItem) -> Option<(usize, usize, Vec<u8>)> {
        let img = image::open(self.dir.join(&item.file)).ok()?.to_rgba8();
        Some((img.width() as usize, img.height() as usize, img.into_raw()))
    }
}

fn png_bytes(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn thumb(w: u32, h: u32, rgba: &[u8]) -> String {
    let Some(img) = image::RgbaImage::from_raw(w, h, rgba.to_vec()) else { return String::new() };
    let t = image::imageops::thumbnail(&img, 160.min(w), ((160.min(w) as f32 / w as f32) * h as f32).max(1.0) as u32);
    let mut out = std::io::Cursor::new(Vec::new());
    if t.write_to(&mut out, image::ImageFormat::Png).is_err() {
        return String::new();
    }
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out.into_inner()))
}

/// Background thread polling the clipboard sequence number.
pub fn start_watcher(app: AppHandle, store: std::sync::Arc<ClipStore>) {
    std::thread::spawn(move || {
        let mut last = crate::winutil::clip_seq();
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let seq = crate::winutil::clip_seq();
            if seq == last || BUSY.load(Ordering::SeqCst) > 0 {
                continue;
            }
            last = seq;
            if seq <= IGNORE_UNTIL.load(Ordering::SeqCst) {
                continue;
            }
            std::thread::sleep(Duration::from_millis(60));
            if excluded_by_source() {
                continue;
            }
            let Ok(mut cb) = arboard::Clipboard::new() else { continue };
            let item = if let Ok(t) = cb.get_text() {
                ClipItem { kind: "text".into(), text: t, ..Default::default() }
            } else if let Ok(img) = cb.get_image() {
                let (w, h) = (img.width as u32, img.height as u32);
                let Some(png) = png_bytes(w, h, &img.bytes) else { continue };
                let file = format!("{}.png", uuid::Uuid::new_v4());
                if std::fs::write(store.dir.join(&file), png).is_err() {
                    continue;
                }
                ClipItem { kind: "image".into(), file, thumb: thumb(w, h, &img.bytes), text: format!("Image {w}×{h}"), ..Default::default() }
            } else {
                continue;
            };
            if store.push(item) {
                let _ = app.emit("clipboard-changed", ());
            }
        }
    });
}
