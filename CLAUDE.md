# Keeyz – rules for Claude

- Keeyz: AI writing assistant for Windows (Quick Actions, typed trigger "Keeyz", launcher, voice, clipboard, text from screen, workflows). Everything unlocked; AI runs on the user's own keys.
- Stack: Tauri 2 (Rust) + plain HTML/JS pages built with Vite (`src/pages`). Windows only for now.
- Design: calm and premium – warm neutrals, ink primary buttons, one quiet teal accent, hairline borders, no gradients/glows.
- Never show other companies' brand names in the UI (no source-app names, no example app lists). AI provider names are the only exception.
- Secrets: API keys only in Windows Credential Manager via `ai::set_key` (service `Keeyz`). The update signing key lives in Infisical (project `keeyz`) and `%USERPROFILE%\.tauri\keeyz.key` – never in the repo.
- Never run work inside the rdev hook callback (`hook.rs::on_event`) – always spawn a thread, or typing freezes.
- When Keeyz itself uses the clipboard, wrap it in `clipboard::begin_own()` / `end_own()` so it doesn't land in clipboard history.
- Check: `npx vite build`, `cargo check` and `cargo test --lib` in `src-tauri`.
- Release (users update in-app): `infisical run --env=prod -- powershell -File scripts\release.ps1 -Version x.y.z -Notes "..."`.
