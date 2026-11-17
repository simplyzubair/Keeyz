# Keeyz

AI that types where you type. Select text in any Windows app, press a key, done.
A Tauri 2 (Rust + web UI) desktop app. Everything is unlocked, and the AI runs on **your own API key**.

## Features

| Feature | How |
|---|---|
| Quick Actions | Select text → `Ctrl+Shift+A` → Fix grammar, Translate, Shorten, Professional, Reply, Summarize… or describe a change. Diff view, versions, refine chips, Replace / Add / Copy |
| Typed trigger | `your text Keeyz make it formal` + space space → rewritten in place. Esc cancels. Standalone (`Keeyz write a thank-you note`), reminders (`Keeyz remind me in 10 min to call mom`) |
| Typed commands | `/calc 12*8`, `/search …`, `/yt …`, `/maps …` + space space. Plus your own commands |
| Live translate / double-space action | Settings → Typing trigger |
| Selection toolbar | Select text with the mouse → Explain / Summarize / Translate / Fix / More (uses UI Automation, clipboard untouched) |
| Launcher | `Ctrl+Shift+Space` → tools, actions, workflows, calculator, clipboard search, AI chat (Tab) |
| Voice typing | `Ctrl+Shift+M` → dictate (OpenAI or Gemini transcription), Insert cleaned / as heard / Continue / Quick Actions |
| Clipboard history | `Ctrl+Shift+V` → search, pin, 1–9 quick paste, images, OCR on images, AI on any clip |
| Text from screen | `Ctrl+Shift+O` → drag an area → Windows OCR (offline) → copy or Quick Actions |
| Undo | `Ctrl+Shift+Z` → puts the original text back (last 20 changes kept) |
| Summarize page | Optional shortcut (Settings → Shortcuts) |
| Workflows | Multi-step pipelines (Meeting notes, Email responder, Translate & polish + your own), re-run a step, export .md |
| Custom actions | AI tab → New action, star / hide / reorder, own hotkeys ("Yours") |
| Your style + Prompt context | Tone rules and background facts sent with requests |
| Providers | OpenAI, Anthropic, Gemini, OpenRouter, any OpenAI-compatible endpoint (Ollama, LM Studio). Live model list, test connection |
| App | Tray (pause triggers), start on boot, 7 themes, text size, notifications, sounds, stats, history, feedback, delete everything |

## Run it

- **Install:** download the latest `Keeyz_x.y.z_x64-setup.exe` from GitHub Releases. After that, Keeyz updates itself (Settings → About → Check for updates).
- **Dev mode:** `npm install`, then `npx tauri dev`.
- **Build:** `npx tauri build`.

## API keys

Paste them in the app: **Settings → AI Provider** (🔑 in the left rail). They are stored in **Windows Credential Manager**
(service `Keeyz`), never in files or the repo. Nothing else needs secrets. See `.env.example`.

## Project layout

```
src/pages/        web UI (one HTML page per window) + lib/ (shared css/js)
src-tauri/src/    Rust: ai.rs (providers), hook.rs (typed trigger + toolbar), winutil.rs (keys, clipboard, UIA),
                  ocr.rs, clipboard.rs, commands.rs, windows_mgr.rs, settings.rs, prompts.rs, lib.rs
scripts/          release.ps1 – build, sign and publish an update
```

Settings, history, stats and clipboard history live in `%APPDATA%\com.skillsgeek.keeyz`.
