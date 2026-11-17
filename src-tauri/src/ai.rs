//! Talks to the AI providers with the user's own key.
//! Supported: OpenAI, Anthropic (Claude), Google Gemini, OpenRouter, any OpenAI-compatible endpoint.

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
            _ => "",
        };
        return anyhow!("Your key works, but your {name} account has no API credit left.{fix}");
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
    match s.provider.as_str() {
        "openrouter" => "https://openrouter.ai/api/v1".into(),
        "custom" => s.endpoint_url.trim_end_matches('/').to_string(),
        _ => "https://api.openai.com/v1".into(),
    }
}

/// Main entry: run a chat completion. `on_delta` gets streamed text pieces.
/// If Gemini is overloaded, it falls back once to the lighter Flash-Lite model.
pub async fn complete(req: Request<'_>, mut on_delta: impl FnMut(&str)) -> Result<String> {
    if req.settings.provider == "auto" {
        return complete_auto(&req, &mut on_delta).await;
    }
    let model = req.settings.model();
    match complete_with(&req, &model, &mut on_delta, None).await {
        Err(e) if req.settings.provider == "gemini" && e.to_string().contains("busy right now") && model != "gemini-flash-lite-latest" => {
            complete_with(&req, "gemini-flash-lite-latest", &mut on_delta, None).await
        }
        r => r,
    }
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
    for (provider, model) in list.into_iter().take(4) {
        let mut s = req.settings.clone();
        s.provider = provider.clone();
        let r = Request { system: req.system.clone(), messages: req.messages.clone(), settings: &s, max_tokens: req.max_tokens };
        match complete_with(&r, &model, on_delta, Some(Duration::from_secs(8))).await {
            Ok(out) => {
                *crate::fast::LAST_USED.lock().unwrap() = format!("{provider} · {model}");
                return Ok(out);
            }
            Err(e) => {
                crate::fast::demote(&provider, &model);
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
            let mut r = http.get(format!("{}/models", openai_base(settings)));
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

/// Transcribe audio (webm/ogg/wav bytes). Uses OpenAI (or OpenAI-compatible) or Gemini.
pub async fn transcribe(settings: &Settings, audio: Vec<u8>, mime: &str, lang: &str) -> Result<String> {
    let http = client();
    // Prefer the active provider; fall back to any provider that has a key and can transcribe.
    let order = [settings.provider.as_str(), "openai", "gemini", "openrouter", "custom"];
    for p in order {
        let key = get_key(p);
        match p {
            "openai" | "custom" if !key.is_empty() || p == "custom" && settings.provider == "custom" => {
                let base = if p == "openai" { "https://api.openai.com/v1".to_string() } else { settings.endpoint_url.trim_end_matches('/').to_string() };
                let ext = if mime.contains("ogg") { "ogg" } else if mime.contains("wav") { "wav" } else { "webm" };
                let part = reqwest::multipart::Part::bytes(audio.clone())
                    .file_name(format!("audio.{ext}"))
                    .mime_str(mime.split(';').next().unwrap_or("audio/webm"))?;
                let mut form = reqwest::multipart::Form::new()
                    .text("model", if p == "openai" { "gpt-4o-mini-transcribe" } else { "whisper-1" })
                    .part("file", part);
                if lang != "auto" && !lang.is_empty() {
                    form = form.text("language", lang.split('-').next().unwrap_or(lang).to_string());
                }
                let mut r = http.post(format!("{base}/audio/transcriptions")).multipart(form);
                if !key.is_empty() { r = r.bearer_auth(&key); }
                let resp = r.send().await.map_err(friendly_net)?;
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                if status >= 400 {
                    if p == "custom" { continue; }
                    return Err(friendly_status(p, status, &body));
                }
                let v: Value = serde_json::from_str(&body).unwrap_or(json!({"text": body}));
                return Ok(v["text"].as_str().unwrap_or("").trim().to_string());
            }
            "gemini" if !key.is_empty() => {
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&audio);
                let model = if settings.provider == "gemini" { settings.model() } else { "gemini-flash-latest".into() };
                let mut instr = "Transcribe this audio exactly as spoken, in the language spoken, with normal punctuation and capitalisation. Return only the transcript.".to_string();
                if lang != "auto" && !lang.is_empty() { instr.push_str(&format!(" The language is {lang}.")); }
                let resp = http
                    .post(format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"))
                    .header("x-goog-api-key", &key)
                    .json(&json!({"contents": [{"parts": [
                        {"inline_data": {"mime_type": mime.split(';').next().unwrap_or("audio/webm"), "data": b64}},
                        {"text": instr}
                    ]}]}))
                    .send().await.map_err(friendly_net)?;
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                if status >= 400 { return Err(friendly_status("gemini", status, &body)); }
                let v: Value = serde_json::from_str(&body)?;
                return Ok(v.pointer("/candidates/0/content/parts/0/text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string());
            }
            _ => {}
        }
    }
    bail!("Voice needs an OpenAI or Gemini key (Anthropic keys can't transcribe audio). Add one in Settings → AI Provider.")
}
