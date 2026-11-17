//! "Auto · fastest": finds the quickest model that answers correctly with the user's keys.
//! No hard-coded model list: it reads each provider's live model list, tests the fast/free
//! candidates on a small real job, ranks them by speed and re-checks every 12 hours.

use crate::ai::{self, Msg, Request};
use crate::settings::Settings;
use anyhow::Result;
use once_cell::sync::{Lazy, OnceCell};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Cand {
    pub provider: String,
    pub model: String,
    pub ms: u64,
    pub ok: bool,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Ranking {
    pub updated: i64,
    pub list: Vec<Cand>,
}

static RANK: Lazy<Mutex<Ranking>> = Lazy::new(|| Mutex::new(Ranking::default()));
static PATH: OnceCell<PathBuf> = OnceCell::new();
static RUNNING: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
/// Model that answered the last "auto" request (shown in the UI).
pub static LAST_USED: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

const MAX_AGE_MS: i64 = 12 * 3600 * 1000;
const SYSTEM: &str = "You are a precise writing assistant. Correct only spelling, grammar and punctuation. Return ONLY the corrected text.";
const SAMPLE: &str = "hey can u send me the report when you get a chance, i has a meeting tomorow";

pub fn init(dir: &PathBuf) {
    let p = dir.join("fast_models.json");
    if let Some(r) = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Ranking>(&t).ok()) {
        *RANK.lock().unwrap() = r;
    }
    let _ = PATH.set(p);
}

fn save() {
    if let Some(p) = PATH.get() {
        let r = RANK.lock().unwrap().clone();
        let _ = std::fs::write(p, serde_json::to_string_pretty(&r).unwrap_or_default());
    }
}

pub fn snapshot() -> Ranking {
    RANK.lock().unwrap().clone()
}

pub fn is_running() -> bool {
    *RUNNING.lock().unwrap()
}

pub fn stale() -> bool {
    let r = RANK.lock().unwrap();
    r.list.iter().all(|c| !c.ok) || chrono::Utc::now().timestamp_millis() - r.updated > MAX_AGE_MS
}

/// Working candidates, fastest first – then, as backups, models that were only busy or
/// rate-limited during the last check (that is temporary; they often work a minute later).
pub fn ranked() -> Vec<(String, String)> {
    let r = RANK.lock().unwrap();
    let busy = |c: &&Cand| {
        let n = c.note.to_lowercase();
        !c.ok && (n.contains("busy") || n.contains("too many") || n.contains("rate") || n.contains("slow"))
    };
    r.list
        .iter()
        .filter(|c| c.ok)
        .chain(r.list.iter().filter(busy))
        .map(|c| (c.provider.clone(), c.model.clone()))
        .collect()
}

/// A model failed during real use: send it to the back of the line until the next check.
pub fn demote(provider: &str, model: &str) {
    let mut r = RANK.lock().unwrap();
    if let Some(pos) = r.list.iter().position(|c| c.provider == provider && c.model == model) {
        let mut c = r.list.remove(pos);
        c.note = "slow or failed just now".into();
        r.list.push(c);
    }
    drop(r);
    save();
}

/// Pick the fast / free models worth testing from a provider's live list.
fn pick(provider: &str, ids: &[String]) -> Vec<String> {
    let skip = regex::Regex::new(r"(?i)(tts|image|audio|realtime|transcribe|embed|vision|-vl|omni|robotics|computer-use|search|live|guard|code)").unwrap();
    let ok: Vec<&String> = ids.iter().filter(|m| !skip.is_match(m)).collect();
    let mut out: Vec<String> = match provider {
        "gemini" => {
            // numbered flash / flash-lite models, newest first, plus the "latest" aliases
            let re = regex::Regex::new(r"^gemini-(\d+(?:\.\d+)?)-flash(-lite)?$").unwrap();
            let mut v: Vec<(f32, String)> = ok
                .iter()
                .filter_map(|m| re.captures(m).map(|c| (c[1].parse::<f32>().unwrap_or(0.0), m.to_string())))
                .collect();
            v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut v: Vec<String> = v.into_iter().map(|x| x.1).take(4).collect();
            for alias in ["gemini-flash-lite-latest", "gemini-flash-latest"] {
                if ids.iter().any(|m| m == alias) {
                    v.push(alias.into());
                }
            }
            v
        }
        "openrouter" => ok.iter().filter(|m| m.ends_with(":free")).take(8).map(|s| s.to_string()).collect(),
        "openai" => {
            let mut v: Vec<String> = ok.iter().filter(|m| m.contains("mini") && !m.contains("nano-") && m.starts_with("gpt")).map(|s| s.to_string()).collect();
            v.sort();
            v.reverse();
            v.truncate(2);
            v
        }
        "anthropic" => {
            let mut v: Vec<String> = ok.iter().filter(|m| m.contains("haiku")).map(|s| s.to_string()).collect();
            v.sort();
            v.reverse();
            v.truncate(1);
            v
        }
        _ => vec![],
    };
    out.dedup();
    out
}

fn good_answer(out: &str) -> bool {
    let o = out.to_lowercase();
    o.contains("send me the report") && o.contains("chance") && out.len() < SAMPLE.len() * 3
}

async fn time_one(settings: &Settings, provider: &str, model: &str) -> Cand {
    let mut s = settings.clone();
    s.provider = provider.into();
    let req = Request { system: SYSTEM.into(), messages: vec![Msg::user(SAMPLE)], settings: &s, max_tokens: 200 };
    let t0 = Instant::now();
    let res = tokio::time::timeout(Duration::from_secs(15), ai::complete_raw(&req, model, Some(Duration::from_secs(12)))).await;
    let ms = t0.elapsed().as_millis() as u64;
    let (ok, note) = match res {
        Ok(Ok(out)) if good_answer(&out) => (true, String::new()),
        Ok(Ok(_)) => (false, "answered wrongly".into()),
        Ok(Err(e)) => (false, e.to_string().chars().take(90).collect()),
        Err(_) => (false, "too slow".into()),
    };
    Cand { provider: provider.into(), model: model.into(), ms, ok, note }
}

/// Test every candidate on every provider that has a key. Providers run in parallel,
/// models of one provider one after another (keeps free-tier rate limits happy).
pub async fn benchmark(settings: &Settings) -> Result<Ranking> {
    {
        let mut r = RUNNING.lock().unwrap();
        if *r {
            anyhow::bail!("Already checking models, one moment.");
        }
        *r = true;
    }
    let providers: Vec<&str> = ["gemini", "openrouter", "openai", "anthropic"].into_iter().filter(|p| ai::has_key(p)).collect();
    let jobs = providers.iter().map(|p| {
        let p = p.to_string();
        let s = settings.clone();
        async move {
            let ids = ai::list_models_for(&s, &p).await.unwrap_or_default();
            let mut res = vec![];
            for m in pick(&p, &ids) {
                res.push(time_one(&s, &p, &m).await);
            }
            res
        }
    });
    let mut list: Vec<Cand> = futures_util::future::join_all(jobs).await.into_iter().flatten().collect();
    list.sort_by(|a, b| b.ok.cmp(&a.ok).then(a.ms.cmp(&b.ms)));
    let rank = Ranking { updated: chrono::Utc::now().timestamp_millis(), list };
    *RANK.lock().unwrap() = rank.clone();
    save();
    *RUNNING.lock().unwrap() = false;
    Ok(rank)
}
