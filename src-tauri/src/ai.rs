//! Talks to the AI providers with the user's own key.
//! Supported: OpenAI, Anthropic (Claude), Google Gemini, OpenRouter, DeepSeek, any OpenAI-compatible endpoint.

use crate::settings::Settings;
use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;

const KEYRING_SERVICE: &str = "Keeyz";
/// Keys saved under the app's earlier name are copied over once.
const LEGACY_SERVICES: [&str; 1] = ["Zkey"];

// ---------------------------------------------------------------- secrets

pub fn get_key(provider: &str) -> String {
    let k = keyring::Entry::new(KEYRING_SERVICE, provider)
        .and_then(|e| e.get_password())
        .unwrap_or_default();
    if !k.is_empty() {
        return k;
    }
    for svc in LEGACY_SERVICES {
        let old = keyring::Entry::new(svc, provider)
            .and_then(|e| e.get_password())
            .unwrap_or_default();
        if !old.is_empty() {
            let _ = set_key(provider, &old);
            return old;
        }
    }
    String::new()
}

pub fn set_key(provider: &str, key: &str) -> Result<()> {
    let e = keyring::Entry::new(KEYRING_SERVICE, provider)?;
    if key.trim().is_empty() {
        let _ = e.delete_credential();
    } else {
        e.set_password(key.trim())?;
    }
    Ok(())
}

pub fn has_key(provider: &str) -> bool {
    !get_key(provider).is_empty()
}

// ---------------------------------------------------------------- types

#[derive(Clone, Debug)]
pub struct Msg {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

impl Msg {
    pub fn user(c: impl Into<String>) -> Self {
        Self { role: "user".into(), content: c.into() }
    }
}

pub struct Request<'a> {
    pub system: String,
    pub messages: Vec<Msg>,
    pub settings: &'a Settings,
    pub max_tokens: u32,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .build()
        .expect("http client")
}

fn friendly_status(provider: &str, status: u16, body: &str) -> anyhow::Error {
    let name = match provider {
        "anthropic" => "Anthropic",
        "gemini" => "Gemini",
        "openrouter" => "OpenRouter",
        "deepseek" => "DeepSeek",
        "custom" => "Your endpoint",
        _ => "OpenAI",
    };
    // Prefer the provider's own error message over the raw JSON body.
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.pointer("/message"))
                .and_then(|m| m.as_str().map(String::from))
        })
        .unwrap_or_else(|| body.to_string());
    let short: String = msg.split(". ").next().unwrap_or(&msg).chars().take(160).collect();
    let low = body.to_lowercase();
    if status == 429 && provider != "gemini" && (low.contains("insufficient_quota") || low.contains("exceeded your current quota")) {
        let fix = match provider {
            "openai" => " Add credit at platform.openai.com → Settings → Billing (API use is billed separately from chat subscriptions).",
            "anthropic" => " Add credit at console.anthropic.com → Billing.",
            "openrouter" => " Add credit at openrouter.ai/settings/credits.",
            "deepseek" => " Top up at platform.deepseek.com → Top up.",
            _ => "",
        };
        return anyhow!("Your key works, but your {name} account has no API credit left.{fix}");
    }
    if provider == "deepseek" && status == 402 {
        return anyhow!("Your DeepSeek key works, but the account has no balance. Top up at platform.deepseek.com → Top up, then try again.");
    }
    // Key works but has no budget / credit left (OpenRouter key limits, prepaid credit).
    if matches!(status, 402 | 403)
        && (low.contains("key limit exceeded") || low.contains("insufficient credits") || low.contains("credit") && low.contains("limit"))
    {
        let fix = if provider == "openrouter" {
            " Raise the key's limit at openrouter.ai/settings/keys, or use a free model (its name ends in :free)."
        } else {
            ""
        };
        return anyhow!("Your key works, but its spending limit is used up ($0 left).{fix}");
    }
    if status == 429 && (low.contains("per-day") || low.contains("per day") || low.contains("perday")) {
        return anyhow!("{name}'s free daily limit is used up. It resets tomorrow – or add credit to your {name} account for more.");
    }
    if status == 429 && provider == "openrouter" && low.contains("rate-limited upstream") {
        return anyhow!("This free model is busy right now. Try again in a minute, or pick another model. ({short})");
    }
    if status == 429 && provider == "gemini" {
        return anyhow!("Gemini free-tier limit reached for this model. Wait a minute, or pick a lighter model like gemini-flash-lite-latest. ({short})");
    }
    match status {
        500 | 502 | 503 | 504 | 529 => anyhow!("{name} is busy right now (tried 4 times). Try again in a minute, or pick another model. ({short})"),
        401 => anyhow!("{name} did not accept this key. Copy it again from your {name} account and paste it in Settings → AI Provider. ({short})"),
        403 => anyhow!("{name} refused the request: {short}"),
        404 => anyhow!("Model not found. Your key works, but {name} doesn't know this model. Pick another one in the Model box. ({short})"),
        429 => anyhow!("{name} says: too many requests right now. Wait a minute and try again. ({short})"),
        400 if short.to_lowercase().contains("api key") => anyhow!("{name} did not accept this key. Copy it again from your {name} account. ({short})"),
        _ => anyhow!("{name} error (HTTP {status}): {short}"),
    }
}

fn friendly_net(e: reqwest::Error) -> anyhow::Error {
    if e.is_timeout() {
        anyhow!("No answer in time. A firewall or proxy may be dropping the connection.")
    } else if e.is_connect() {
        anyhow!("Can't reach the AI server. Check your internet, VPN or firewall.")
    } else {
        anyhow!("Request failed: {e}")
    }
}

fn openai_base(s: &Settings) -> String {
    openai_base_for(s, &s.provider)
}

fn openai_base_for(s: &Settings, provider: &str) -> String {
    match provider {
        "openrouter" => "https://openrouter.ai/api/v1".into(),
        "deepseek" => "https://api.deepseek.com/v1".into(),
        "custom" => s.endpoint_url.trim_end_matches('/').to_string(),
        _ => "https://api.openai.com/v1".into(),
    }
}

/// Main entry: run a chat completion. `on_delta` gets streamed text pieces.
/// If the chosen model fails before any text arrived (busy, no quota/credit, model gone, too
/// slow to start, key refused), Keeyz moves on by itself: Gemini first tries Flash-Lite, then the
/// fastest other working models the user has keys for (DeepSeek, GPT mini, Haiku, OpenRouter…).
pub async fn complete(req: Request<'_>, mut on_delta: impl FnMut(&str)) -> Result<String> {
    if req.settings.provider == "auto" {
        return complete_auto(&req, &mut on_delta).await;
    }
    let model = req.settings.model();
    // A local / custom model never hands the text to a cloud service. Without other keys there is
    // nothing to switch to, so the chosen model gets its usual patient retries instead.
    let can_switch = req.settings.provider != "custom" && !fallback_models(req.settings, &req.settings.provider, &model, false).is_empty();
    let mut got_text = false;
    let first = {
        let mut cb = |d: &str| {
            if !d.is_empty() {
                got_text = true;
            }
            on_delta(d)
        };
        complete_with(&req, &model, &mut cb, if can_switch { Some(Duration::from_secs(12)) } else { None }).await
    };
    let err = match first {
        Ok(out) => {
            *crate::fast::LAST_USED.lock().unwrap() = format!("{} · {model}", req.settings.provider);
            return Ok(out);
        }
        Err(e) => e,
    };
    if got_text || !can_switch || !worth_switching(&err) {
        return Err(err);
    }
    let failed_provider = req.settings.provider.clone();
    let whole_account = is_account_problem(&err);
    let mut last = err;
    let mut tries = 0;
    for (provider, m) in fallback_models(req.settings, &failed_provider, &model, whole_account) {
        if tries >= 4 {
            break;
        }
        tries += 1;
        let mut s = req.settings.clone();
        s.provider = provider.clone();
        let r = Request { system: req.system.clone(), messages: req.messages.clone(), settings: &s, max_tokens: req.max_tokens };
        match complete_with(&r, &m, &mut on_delta, Some(Duration::from_secs(8))).await {
            Ok(out) => {
                crate::hook::dbg_log!("[ai] {failed_provider} · {model} failed, used {provider} · {m}");
                *crate::fast::LAST_USED.lock().unwrap() = format!("{provider} · {m}");
                return Ok(out);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Errors where another model can help. A problem with the text itself (far too long, etc.) or no
/// internet at all would fail everywhere, so those are shown straight away.
fn worth_switching(e: &anyhow::Error) -> bool {
    let m = e.to_string().to_lowercase();
    if m.contains("can't reach the ai server") || m.contains("too long") || m.contains("context length") {
        return false;
    }
    true
}

/// The whole account can't answer right now (no credit / quota, key refused): skip its other models too.
fn is_account_problem(e: &anyhow::Error) -> bool {
    let m = e.to_string().to_lowercase();
    m.contains("limit") || m.contains("quota") || m.contains("credit") || m.contains("balance") || m.contains("did not accept this key")
}

/// Fast, low-cost models to try after `failed` – measured fastest first (Auto ranking), then
/// sensible defaults for every provider that has a key.
pub fn fallback_models(s: &Settings, failed: &str, failed_model: &str, skip_provider: bool) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut add = |p: &str, m: &str| {
        let bad = (p == failed && (skip_provider || m == failed_model)) || m.is_empty();
        if !bad && !out.iter().any(|(op, om)| op == p && om == m) {
            out.push((p.to_string(), m.to_string()));
        }
    };
    // Gemini busy: its lighter model first (same key, usually free and fast)
    if failed == "gemini" && !skip_provider {
        add("gemini", "gemini-flash-lite-latest");
    }
    for (p, m) in crate::fast::ranked() {
        add(&p, &m);
    }
    let pick = |p: &str, d: &str| s.models.get(p).filter(|m| !m.trim().is_empty()).cloned().unwrap_or_else(|| d.to_string());
    for (p, m) in [
        ("gemini", "gemini-flash-lite-latest".to_string()),
        ("gemini", "gemini-flash-latest".to_string()),
        ("deepseek", "deepseek-chat".to_string()),
        ("openai", "gpt-4o-mini".to_string()),
        ("openrouter", pick("openrouter", "openai/gpt-4o-mini")),
        ("anthropic", "claude-haiku-4-5".to_string()),
    ] {
        if has_key(p) {
            add(p, &m);
        }
    }
    out
}

/// Auto mode: use the fastest working model; if it is slow to start or fails, move to the next one.
async fn complete_auto(req: &Request<'_>, on_delta: &mut impl FnMut(&str)) -> Result<String> {
    let mut list = crate::fast::ranked();
    if list.is_empty() {
        crate::fast::benchmark(req.settings).await?;
        list = crate::fast::ranked();
    }
    if list.is_empty() {
        bail!("Auto mode found no working model. Add an AI key in Settings → AI Provider (Gemini has a free tier), then click “Find fastest now”.");
    }
    let mut last = anyhow!("No model answered.");
    let mut skip: Vec<String> = vec![];
    let mut tries = 0;
    for (provider, model) in list {
        if skip.contains(&provider) || tries >= 5 {
            continue;
        }
        tries += 1;
        let mut s = req.settings.clone();
        s.provider = provider.clone();
        let r = Request { system: req.system.clone(), messages: req.messages.clone(), settings: &s, max_tokens: req.max_tokens };
        match complete_with(&r, &model, on_delta, Some(Duration::from_secs(8))).await {
            Ok(out) => {
                *crate::fast::LAST_USED.lock().unwrap() = format!("{provider} · {model}");
                return Ok(out);
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                let account_limit = msg.contains("limit") || msg.contains("quota") || msg.contains("credit") || msg.contains("did not accept this key");
                if account_limit {
                    // the whole account is out for now – other models of this provider would fail too
                    skip.push(provider.clone());
                } else {
                    crate::fast::demote(&provider, &model);
                }
                last = e;
            }
        }
    }
    Err(last)
}

/// One request to one exact provider/model (used by the speed check).
pub async fn complete_raw(req: &Request<'_>, model: &str, first_token: Option<Duration>) -> Result<String> {
    complete_with(req, model, &mut |_: &str| {}, first_token).await
}

async fn complete_with(req: &Request<'_>, model: &str, on_delta: &mut impl FnMut(&str), first_token: Option<Duration>) -> Result<String> {
    let s = req.settings;
    let provider = s.provider.as_str();
    let key = get_key(provider);
    if key.is_empty() && provider != "custom" {
        bail!("No API key yet. Open Settings → AI Provider and paste your key.");
    }
    let model = model.to_string();
    let http = client();

    // Retry temporary server problems (busy / overloaded) a few times before giving up.
    let mut attempt = 0;
    let resp = loop {
        attempt += 1;
        let send = async { match provider {
        "anthropic" => {
            let msgs: Vec<Value> = req
                .messages
                .iter()
                .map(|m| json!({"role": m.role, "content": m.content}))
                .collect();
            http.post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01")
                .json(&json!({
                    "model": model, "max_tokens": req.max_tokens, "system": req.system,
                    "messages": msgs, "stream": true
                }))
                .send()
                .await
        }
        "gemini" => {
            let contents: Vec<Value> = req
                .messages
                .iter()
                .map(|m| json!({
                    "role": if m.role == "assistant" { "model" } else { "user" },
                    "parts": [{"text": m.content}]
                }))
                .collect();
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse"
            );
            http.post(url)
                .header("x-goog-api-key", &key)
                .json(&json!({
                    "systemInstruction": {"parts": [{"text": req.system}]},
                    "contents": contents,
                    "generationConfig": gemini_config(&model, req.max_tokens)
                }))
                .send()
                .await
        }
        _ => {
            let mut msgs = vec![json!({"role": "system", "content": req.system})];
            msgs.extend(req.messages.iter().map(|m| json!({"role": m.role, "content": m.content})));
            let mut body = json!({"model": model, "messages": msgs, "stream": true});
            if provider == "openai" {
                body["max_completion_tokens"] = json!(req.max_tokens);
            } else {
                body["max_tokens"] = json!(req.max_tokens);
            }
            if provider == "custom" && !s.thinking {
                // Qwen3 / DeepSeek style switch; ignored by servers that don't know it.
                body["think"] = json!(false);
            }
            let mut r = http
                .post(format!("{}/chat/completions", openai_base(s)))
                .json(&body);
            if !key.is_empty() {
                r = r.bearer_auth(&key);
            }
            if provider == "openrouter" {
                r = r.header("HTTP-Referer", "https://github.com/simplyzubair/Keeyz").header("X-Title", "Keeyz");
            }
            r.send().await
        }
    } };
        let r = match first_token {
            Some(t) => tokio::time::timeout(t, send).await.map_err(|_| anyhow!("This model is slow right now."))?,
            None => send.await,
        }
        .map_err(friendly_net)?;
        let status = r.status().as_u16();
        // Busy servers (and busy shared free models on OpenRouter) get a few retries.
        let busy_free = status == 429 && provider == "openrouter" && model.ends_with(":free");
        // In auto mode don't wait around: the next model takes over instead.
        let max_attempts = if first_token.is_some() { 1 } else { 4 };
        if (matches!(status, 500 | 502 | 503 | 504 | 529) || busy_free) && attempt < max_attempts {
            tokio::time::sleep(Duration::from_millis(800 * attempt as u64)).await;
            continue;
        }
        if status >= 400 {
            let body = r.text().await.unwrap_or_default();
            return Err(friendly_status(provider, status, &body));
        }
        break r;
    };

    // ---- read the SSE stream
    let mut out = String::new();
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    loop {
        let next = match first_token {
            Some(t) if out.is_empty() => tokio::time::timeout(t, stream.next()).await.map_err(|_| anyhow!("This model is slow right now."))?,
            _ => stream.next().await,
        };
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(friendly_net)?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" || data.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(err) = v.get("error") {
                bail!("AI error: {}", err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown"));
            }
            let piece = match provider {
                "anthropic" => v.pointer("/delta/text").and_then(|x| x.as_str()).unwrap_or(""),
                "gemini" => v.pointer("/candidates/0/content/parts/0/text").and_then(|x| x.as_str()).unwrap_or(""),
                _ => v.pointer("/choices/0/delta/content").and_then(|x| x.as_str()).unwrap_or(""),
            };
            if !piece.is_empty() {
                out.push_str(piece);
                on_delta(piece);
            }
        }
    }
    let cleaned = clean_output(&out);
    if cleaned.is_empty() {
        bail!("The AI returned an empty result. Try again.");
    }
    Ok(cleaned)
}

/// Numbered Flash models (e.g. gemini-3.5-flash) think before answering by default; for
/// short writing jobs that only adds seconds, so switch it off. Lite / alias models reject the flag.
fn gemini_config(model: &str, max_tokens: u32) -> Value {
    let re = regex::Regex::new(r"^gemini-\d+(\.\d+)?-flash$").unwrap();
    if re.is_match(model) {
        json!({"maxOutputTokens": max_tokens, "thinkingConfig": {"thinkingBudget": 0}})
    } else {
        json!({"maxOutputTokens": max_tokens})
    }
}

/// Remove `<think>` blocks and stray code fences that wrap plain text.
pub fn clean_output(s: &str) -> String {
    let re = regex::Regex::new(r"(?s)<think>.*?</think>").unwrap();
    let mut t = re.replace_all(s, "").trim().to_string();
    // models sometimes echo the <<< >>> markers that wrap the user's text
    if let Some(rest) = t.strip_prefix("<<<") {
        t = rest.trim_start().to_string();
    }
    if let Some(rest) = t.strip_suffix(">>>") {
        t = rest.trim_end().to_string();
    }
    if t.starts_with("```") && t.ends_with("```") && t.len() > 6 {
        let inner = &t[3..t.len() - 3];
        let inner = inner.split_once('\n').map(|(first, rest)| if first.trim().chars().all(|c| c.is_alphanumeric()) { rest } else { inner }).unwrap_or(inner);
        t = inner.trim().to_string();
    }
    t
}

/// One-shot helper without streaming callback.
pub async fn ask(settings: &Settings, system: &str, user: &str) -> Result<String> {
    complete(
        Request { system: system.into(), messages: vec![Msg::user(user)], settings, max_tokens: 4096 },
        |_| {},
    )
    .await
}

// ---------------------------------------------------------------- test + models

pub async fn test_connection(settings: &Settings) -> Result<String> {
    let r = ask(settings, "Reply with the single word: OK", "ping").await?;
    let who = if settings.provider == "auto" { crate::fast::LAST_USED.lock().unwrap().clone() } else { settings.model() };
    Ok(format!("Connected ({}): {}", who, r.chars().take(40).collect::<String>()))
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    #[test]
    fn switching_rules() {
        assert!(worth_switching(&anyhow!("Gemini is busy right now (tried 4 times).")));
        assert!(worth_switching(&anyhow!("Your key works, but your DeepSeek account has no API credit left.")));
        assert!(!worth_switching(&anyhow!("Can't reach the AI server. Check your internet, VPN or firewall.")));
        assert!(is_account_problem(&anyhow!("Gemini's free daily limit is used up.")));
        assert!(is_account_problem(&anyhow!("Your DeepSeek key works, but the account has no balance.")));
        assert!(!is_account_problem(&anyhow!("This model is slow right now.")));
    }

    #[test]
    fn cleans_output() {
        assert_eq!(clean_output("<think>x</think>Hello"), "Hello");
        assert_eq!(clean_output("```\nHello\n```"), "Hello");
        assert_eq!(clean_output("plain"), "plain");
    }
}

pub async fn list_models(settings: &Settings) -> Result<Vec<String>> {
    list_models_for(settings, &settings.provider).await
}

pub async fn list_models_for(settings: &Settings, provider: &str) -> Result<Vec<String>> {
    let http = client();
    let p = provider;
    let key = get_key(p);
    let mut ids: Vec<String> = match p {
        "anthropic" => {
            let v: Value = http.get("https://api.anthropic.com/v1/models?limit=100")
                .header("x-api-key", &key).header("anthropic-version", "2023-06-01")
                .send().await.map_err(friendly_net)?.json().await?;
            v["data"].as_array().cloned().unwrap_or_default().iter()
                .filter_map(|m| m["id"].as_str().map(String::from)).collect()
        }
        "gemini" => {
            let v: Value = http.get("https://generativelanguage.googleapis.com/v1beta/models?pageSize=200")
                .header("x-goog-api-key", &key).send().await.map_err(friendly_net)?.json().await?;
            v["models"].as_array().cloned().unwrap_or_default().iter()
                .filter(|m| m["supportedGenerationMethods"].to_string().contains("generateContent"))
                .filter_map(|m| m["name"].as_str().map(|n| n.trim_start_matches("models/").to_string()))
                .collect()
        }
        _ => {
            let mut r = http.get(format!("{}/models", openai_base_for(settings, p)));
            if !key.is_empty() { r = r.bearer_auth(&key); }
            let v: Value = r.send().await.map_err(friendly_net)?.json().await?;
            v["data"].as_array().cloned().unwrap_or_default().iter()
                .filter_map(|m| m["id"].as_str().map(String::from))
                .filter(|id| p != "openai" || id.starts_with("gpt") || id.starts_with('o') || id.starts_with("chatgpt"))
                .collect()
        }
    };
    ids.sort();
    ids.dedup();
    Ok(ids)
}

// ---------------------------------------------------------------- voice

/// Transcribe audio (webm/ogg/wav bytes) with whichever key works.
/// Order: the fastest working providers from Auto mode, then Gemini, OpenAI, a local server.
/// If one fails (no credit, busy, bad key) the next one is tried; only if all fail is an error shown.
pub async fn transcribe(settings: &Settings, audio: Vec<u8>, mime: &str, lang: &str) -> Result<String> {
    let mut order: Vec<String> = vec![];
    let ranked = crate::fast::ranked();
    if settings.provider != "auto" {
        order.push(settings.provider.clone());
    }
    for (p, _) in &ranked {
        order.push(p.clone());
    }
    for p in ["gemini", "openai", "custom"] {
        order.push(p.into());
    }
    order.dedup();
    let mut seen = std::collections::HashSet::new();
    order.retain(|p| seen.insert(p.clone()) && matches!(p.as_str(), "gemini" | "openai" | "custom"));

    let mut last_err: Option<anyhow::Error> = None;
    for p in order {
        let key = get_key(&p);
        let res = match p.as_str() {
            "gemini" if !key.is_empty() => {
                // fastest Gemini models from Auto mode first, then safe defaults
                let mut models: Vec<String> = ranked.iter().filter(|(rp, _)| rp == "gemini").map(|(_, m)| m.clone()).collect();
                if settings.provider == "gemini" {
                    models.insert(0, settings.model());
                }
                models.extend(["gemini-flash-lite-latest".to_string(), "gemini-flash-latest".to_string()]);
                models.dedup();
                let mut r = Err(anyhow!("Gemini could not transcribe."));
                for m in models.into_iter().take(3) {
                    r = transcribe_gemini(&key, &m, &audio, mime, lang).await;
                    if r.is_ok() {
                        break;
                    }
                }
                r
            }
            "openai" if !key.is_empty() => transcribe_openai("https://api.openai.com/v1", &key, "gpt-4o-mini-transcribe", &audio, mime, lang, &p).await,
            "custom" if settings.provider == "custom" => {
                let base = settings.endpoint_url.trim_end_matches('/').to_string();
                transcribe_openai(&base, &key, "whisper-1", &audio, mime, lang, &p).await
            }
            _ => continue,
        };
        match res {
            Ok(t) => return Ok(t),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("Voice needs a Gemini or OpenAI key (Anthropic keys can't transcribe audio). Add one in Settings → AI Provider.")))
}

async fn transcribe_openai(base: &str, key: &str, model: &str, audio: &[u8], mime: &str, lang: &str, provider: &str) -> Result<String> {
    let ext = if mime.contains("ogg") { "ogg" } else if mime.contains("wav") { "wav" } else { "webm" };
    let part = reqwest::multipart::Part::bytes(audio.to_vec())
        .file_name(format!("audio.{ext}"))
        .mime_str(mime.split(';').next().unwrap_or("audio/webm"))?;
    let mut form = reqwest::multipart::Form::new().text("model", model.to_string()).part("file", part);
    if lang != "auto" && !lang.is_empty() {
        form = form.text("language", lang.split('-').next().unwrap_or(lang).to_string());
    }
    let mut r = client().post(format!("{base}/audio/transcriptions")).multipart(form);
    if !key.is_empty() {
        r = r.bearer_auth(key);
    }
    let resp = r.send().await.map_err(friendly_net)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if status >= 400 {
        return Err(friendly_status(provider, status, &body));
    }
    let v: Value = serde_json::from_str(&body).unwrap_or(json!({"text": body}));
    Ok(v["text"].as_str().unwrap_or("").trim().to_string())
}

async fn transcribe_gemini(key: &str, model: &str, audio: &[u8], mime: &str, lang: &str) -> Result<String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(audio);
    let mut instr = "Transcribe this audio exactly as spoken, in the language spoken, with normal punctuation and capitalisation. Return only the transcript.".to_string();
    if lang != "auto" && !lang.is_empty() {
        instr.push_str(&format!(" The language is {lang}."));
    }
    let resp = client()
        .post(format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"))
        .header("x-goog-api-key", key)
        .json(&json!({
            "contents": [{"parts": [
                {"inline_data": {"mime_type": mime.split(';').next().unwrap_or("audio/webm"), "data": b64}},
                {"text": instr}
            ]}],
            "generationConfig": gemini_config(model, 2048)
        }))
        .send()
        .await
        .map_err(friendly_net)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if status >= 400 {
        return Err(friendly_status("gemini", status, &body));
    }
    let v: Value = serde_json::from_str(&body)?;
    Ok(v.pointer("/candidates/0/content/parts/0/text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string())
}
