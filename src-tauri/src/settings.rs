//! App settings, saved as JSON in the app config folder.
//! API keys are NOT stored here – they live in Windows Credential Manager (see `secrets`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionDef {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub icon: String,
    pub builtin: bool,
    pub hidden: bool,
    pub favorite: bool,
    /// "replace" = rewrites the selection, "add" = writes new text (reply, answer…)
    pub output: String,
    /// true once the user changed a built-in action – then app updates keep their version
    pub edited: bool,
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    #[test]
    fn merges_legacy_defaults_and_models() {
        for trigger in ["K", "Zkey"] {
            let mut s = Settings::default();
            s.trigger_word = trigger.into();
            s.merge_builtins();
            assert_eq!(s.trigger_word, "Keeyz");
        }
        let mut s = Settings::default();
        s.actions.clear();
        s.merge_builtins();
        for action in crate::prompts::builtin_actions() { assert!(s.actions.iter().any(|a| a.id == action.id)); }
        s.run_spaces = 9;
        s.merge_builtins();
        assert_eq!(s.run_spaces, 2);

        let mut s = Settings::default();
        s.provider = "openai".into();
        let models = s.models.clone();
        s.custom_model = "gen-lang-client-123".into();
        s.merge_builtins();
        assert!(s.custom_model.is_empty());
        assert_eq!(s.models, models);
    }
}

impl Default for ActionDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            prompt: String::new(),
            icon: "✨".into(),
            builtin: false,
            hidden: false,
            favorite: false,
            output: "replace".into(),
            edited: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WorkflowStep {
    /// built-in action id, or "custom"
    pub mode: String,
    pub custom_prompt: String,
    /// "original_text" | "previous_step" | "combined"
    pub input_source: String,
    pub output_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub builtin: bool,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TypedCommand {
    /// e.g. "/cost"
    pub pattern: String,
    pub prompt: String,
    /// "before" = runs on the line typed before it, "after" = on the text typed after it
    pub scope: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomShortcut {
    pub id: String,
    /// "action" | "translate" | "workflow"
    pub kind: String,
    /// action id, language name, or workflow id
    pub target: String,
    pub accelerator: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // ---- AI provider ----
    pub provider: String,
    pub models: HashMap<String, String>,
    pub custom_model: String,
    pub endpoint_url: String,
    pub thinking: bool,
    pub style_enabled: bool,
    pub style_text: String,
    pub prompt_context_enabled: bool,
    pub prompt_context: String,

    // ---- typed trigger ----
    pub trigger_word: String,
    pub typed_commands: bool,
    pub instant_run: bool,
    pub live_translate: bool,
    pub live_translate_lang: String,
    pub rewrite_whole_field: bool,
    pub double_space_action: String,
    pub run_spaces: u8,
    pub custom_commands: Vec<TypedCommand>,

    // ---- behaviour ----
    pub output_window: bool,
    pub review_insert: bool,
    pub selection_toolbar: bool,
    pub close_behavior: String,
    pub start_on_boot: bool,
    pub notifications: bool,
    pub sounds: bool,
    pub hide_on_blur: bool,
    pub notify_on_tray: bool,
    pub always_on_top: bool,
    pub tray_notice_shown: bool,

    // ---- appearance ----
    pub theme: String,
    pub text_size: String,
    pub language: String,
    pub default_translate_lang: String,

    // ---- voice ----
    pub voice_review: bool,
    pub voice_auto_stop: bool,
    pub voice_lang: String,
    pub voice_mic: String,

    // ---- shortcuts ----
    pub shortcuts: HashMap<String, String>,
    pub custom_shortcuts: Vec<CustomShortcut>,

    // ---- content ----
    pub actions: Vec<ActionDef>,
    pub action_order: Vec<String>,
    pub workflows: Vec<Workflow>,
    pub onboarded: bool,
}

pub fn default_shortcuts() -> HashMap<String, String> {
    [
        ("actions", "Ctrl+Shift+A"),
        ("launcher", "Ctrl+Shift+Space"),
        ("clipboard", "Ctrl+Shift+V"),
        ("voice", "Ctrl+Shift+M"),
        ("ocr", "Ctrl+Shift+O"),
        ("undo", "Ctrl+Shift+Z"),
        ("summarize", ""),
        ("show_app", ""),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

pub fn default_models() -> HashMap<String, String> {
    [
        ("openai", "gpt-4o-mini"),
        ("anthropic", "claude-haiku-4-5"),
        ("gemini", "gemini-flash-latest"),
        ("openrouter", "openai/gpt-4o-mini"),
        ("custom", "llama3.2"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "auto".into(),
            models: default_models(),
            custom_model: String::new(),
            endpoint_url: "http://localhost:11434/v1".into(),
            thinking: false,
            style_enabled: false,
            style_text: String::new(),
            prompt_context_enabled: false,
            prompt_context: String::new(),
            trigger_word: "Keeyz".into(),
            typed_commands: true,
            instant_run: false,
            live_translate: false,
            live_translate_lang: "English".into(),
            rewrite_whole_field: false,
            double_space_action: "none".into(),
            run_spaces: 2,
            custom_commands: vec![],
            output_window: true,
            review_insert: false,
            selection_toolbar: true,
            close_behavior: "tray".into(),
            start_on_boot: true,
            notifications: true,
            sounds: false,
            hide_on_blur: false,
            notify_on_tray: false,
            always_on_top: false,
            tray_notice_shown: false,
            theme: "system".into(),
            text_size: "default".into(),
            language: "en".into(),
            default_translate_lang: "English".into(),
            voice_review: false,
            voice_auto_stop: true,
            voice_lang: "auto".into(),
            voice_mic: String::new(),
            shortcuts: default_shortcuts(),
            custom_shortcuts: vec![],
            actions: vec![],
            action_order: vec![],
            workflows: vec![],
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn path(dir: &PathBuf) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &PathBuf) -> Self {
        let mut s: Settings = std::fs::read_to_string(Self::path(dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.merge_builtins();
        s
    }

    pub fn save(&self, dir: &PathBuf) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, Self::path(dir))?;
        Ok(())
    }

    /// Make sure every built-in action / workflow / shortcut exists (keeps user edits).
    pub fn merge_builtins(&mut self) {
        for b in crate::prompts::builtin_actions() {
            match self.actions.iter_mut().find(|a| a.id == b.id) {
                // refresh wording from the app, keep the user's star / hidden choices
                Some(a) if a.builtin && !a.edited => {
                    a.name = b.name;
                    a.prompt = b.prompt;
                    a.icon = b.icon;
                    a.output = b.output;
                }
                Some(_) => {}
                None => self.actions.push(b),
            }
        }
        for w in crate::prompts::builtin_workflows() {
            if !self.workflows.iter().any(|x| x.id == w.id) {
                self.workflows.push(w);
            }
        }
        for (k, v) in default_shortcuts() {
            self.shortcuts.entry(k).or_insert(v);
        }
        for (k, v) in default_models() {
            self.models.entry(k).or_insert(v);
        }
        // Older versions had one global "custom model id". Move it into the current provider's model,
        // and drop values that are clearly not models (Google Cloud project names, keys).
        let cm = self.custom_model.trim().to_string();
        if !cm.is_empty() {
            let bad = ["gen-lang-client-", "projects/", "AIza", "AQ.", "sk-"].iter().any(|p| cm.starts_with(p));
            if !bad {
                self.models.insert(self.provider.clone(), cm);
            }
            self.custom_model.clear();
        }
        // The trigger word is "Keeyz" by default. Older defaults move over.
        let tw = self.trigger_word.trim().to_string();
        if tw.is_empty() || ["K", "Zkey"].contains(&tw.as_str()) {
            self.trigger_word = "Keeyz".into();
        } else {
            self.trigger_word = tw;
        }
        if self.run_spaces < 2 || self.run_spaces > 3 {
            self.run_spaces = 2;
        }
    }

    pub fn model(&self) -> String {
        if !self.custom_model.trim().is_empty() {
            return self.custom_model.trim().to_string();
        }
        self.models
            .get(&self.provider)
            .cloned()
            .unwrap_or_else(|| "gpt-4o-mini".into())
    }

    pub fn action(&self, id: &str) -> Option<ActionDef> {
        self.actions.iter().find(|a| a.id == id).cloned()
    }
}
