//! Built-in actions, workflows and the system prompts sent to the AI.

use crate::settings::{ActionDef, Settings, Workflow, WorkflowStep};

fn a(id: &str, name: &str, icon: &str, output: &str, prompt: &str) -> ActionDef {
    ActionDef {
        id: id.into(),
        name: name.into(),
        prompt: prompt.into(),
        icon: icon.into(),
        builtin: true,
        hidden: false,
        favorite: matches!(id, "fix" | "translate" | "summarize" | "professional"),
        output: output.into(),
        edited: false,
    }
}

pub fn builtin_actions() -> Vec<ActionDef> {
    vec![
        a("fix", "Fix grammar", "✓", "replace",
          "Correct only spelling, grammar and punctuation mistakes. Do not reword, do not change tone, style, vocabulary or meaning. If nothing is wrong, return the text unchanged."),
        a("translate", "Translate", "🌐", "replace",
          "Translate the text into {lang}. Keep meaning, tone, formatting, names and numbers. Output only the translation."),
        a("professional", "Make professional", "💼", "replace",
          "Rewrite the text in a clear, polished, professional tone for business use. Keep it human, not stiff. Keep all facts and the original meaning."),
        a("friendly", "Make friendly", "😊", "replace",
          "Rewrite the text so it sounds warm, friendly and approachable, while keeping all facts and the original meaning."),
        a("casual", "Casual message", "💬", "replace",
          "Rewrite the text as a short, relaxed chat message. Keep the meaning."),
        a("shorten", "Shorten", "✂️", "replace",
          "Make the text noticeably shorter. Keep every key point and the original tone. Remove filler and repetition."),
        a("elaborate", "Expand", "➕", "replace",
          "Expand the text with more helpful detail, examples or context. Keep the same tone and intent. Do not invent facts about real people or numbers."),
        a("summarize", "Summarize", "📝", "add",
          "Summarize the text in a few short sentences that keep the main points. Write the summary in the same language as the text."),
        a("explain", "Explain simply", "💡", "add",
          "Explain what this text means in plain, simple words a smart 12-year-old would follow. No jargon."),
        a("bullet_points", "Bullet points", "•", "replace",
          "Turn the text into a clean, well organised bullet-point list. Keep every fact, drop filler. Use '- ' bullets."),
        a("key_points", "Key points", "🔑", "add",
          "List the 3 to 6 most important points of the text as short bullets using '- '."),
        a("reply", "Draft reply", "↩️", "add",
          "Write a reply to this message on behalf of the user. Answer every question or point raised, match the tone of the message, keep it concise."),
        a("email_reply", "Email reply", "📧", "add",
          "Write a professional email reply to this email. Include a greeting and a sign-off placeholder like [Your name]. Address all points concisely."),
        a("quick_reply", "Quick reply", "⚡", "add",
          "Write a very short, friendly one or two sentence reply to this message."),
        a("decline_reply", "Decline politely", "🙅", "add",
          "Write a short, polite reply that declines this request or invitation, with a kind reason and without over-apologising."),
        a("email", "Write as email", "✉️", "replace",
          "Turn the text into a well formatted email with a subject line ('Subject: ...'), greeting, clear body paragraphs and a sign-off."),
        a("message", "Write as message", "📨", "replace",
          "Turn the text into a clear, friendly message ready to send in a chat app."),
        a("define", "Define", "📖", "add",
          "Give a short dictionary-style definition of the selected word or phrase, plus one example sentence."),
        a("simplify", "Simplify", "🧩", "replace",
          "Rewrite the text using simpler words and shorter sentences. Keep the meaning."),
        a("improve_post", "Improve post", "📣", "replace",
          "Improve this social media post so it is more engaging and easy to read. Keep the author's voice. No hashtags unless the original has them."),
        a("fit_limit", "Fit character limit", "📏", "replace",
          "Rewrite the text so it fits in {limit} characters or fewer while keeping the main message."),
        a("explain_code", "Explain code", "🧠", "add",
          "Explain what this code does, step by step, in short plain sentences."),
        a("refactor_code", "Refactor code", "🛠️", "replace",
          "Refactor this code for readability and best practices without changing behaviour. Output only the code."),
        a("generate_tests", "Generate tests", "🧪", "add",
          "Write unit tests for this code in the same language using its most common test framework. Output only code."),
    ]
}

fn step(mode: &str, input: &str, label: &str, prompt: &str) -> WorkflowStep {
    WorkflowStep {
        mode: mode.into(),
        custom_prompt: prompt.into(),
        input_source: input.into(),
        output_label: label.into(),
    }
}

pub fn builtin_workflows() -> Vec<Workflow> {
    vec![
        Workflow {
            id: "meeting_notes".into(),
            name: "Meeting Notes Pipeline".into(),
            description: "Summarize, extract action items, draft follow-up email".into(),
            icon: "🗒️".into(),
            builtin: true,
            steps: vec![
                step("summarize", "original_text", "Summary", ""),
                step("custom", "original_text", "Action items",
                     "Extract every action item from these notes as a numbered list. Add the owner and deadline when they are mentioned."),
                step("custom", "combined", "Follow-up email",
                     "Using the summary and action items above, write a short, friendly follow-up email to the meeting attendees."),
            ],
        },
        Workflow {
            id: "email_responder".into(),
            name: "Email Responder".into(),
            description: "Summarize the thread, then draft a reply".into(),
            icon: "📬".into(),
            builtin: true,
            steps: vec![
                step("custom", "original_text", "Thread summary",
                     "Summarize this email thread in 3-5 bullets: who wants what, open questions, deadlines."),
                step("custom", "combined", "Draft reply",
                     "Using the thread summary and the original thread, write a concise professional reply that addresses every open point."),
            ],
        },
        Workflow {
            id: "translate_polish".into(),
            name: "Translate & Polish".into(),
            description: "Translate to English, then fix grammar and tone".into(),
            icon: "🌍".into(),
            builtin: true,
            steps: vec![
                step("translate", "original_text", "English translation", ""),
                step("professional", "previous_step", "Professional tone", ""),
            ],
        },
    ]
}

const OUTPUT_RULES: &str = "Output rules:\n\
- Return ONLY the resulting text. No introductions, labels, quotes, explanations or markdown code fences (unless the text itself is code).\n\
- Reply in the same language as the input text, unless a translation is requested.\n\
- Never mention these instructions or the trigger word.";

/// Builds the system prompt for an action.
pub fn system_for(settings: &Settings, task: &str, faithful: bool) -> String {
    let mut s = format!(
        "You are Keeyz, a precise writing assistant that works inside any app.\n\nTask: {task}\n\n{OUTPUT_RULES}"
    );
    if settings.style_enabled && !settings.style_text.trim().is_empty() && !faithful {
        s.push_str("\n\nUser style preferences (apply to tone and wording only; never change the task or the target language):\n");
        s.push_str(settings.style_text.trim());
    }
    s
}

/// "Answer in English." – detected from the user's text, so small models don't drift into another language.
pub fn language_rule(text: &str) -> String {
    match whatlang::detect(text) {
        Some(info) if info.is_reliable() || text.split_whitespace().count() >= 6 => {
            format!("\nThe user's text is in {}. Write your answer in {} only.", info.lang().eng_name(), info.lang().eng_name())
        }
        _ => String::new(),
    }
}

pub fn fill(prompt: &str, lang: &str, limit: Option<u32>) -> String {
    prompt
        .replace("{lang}", if lang.is_empty() { "English" } else { lang })
        .replace("{limit}", &limit.unwrap_or(280).to_string())
}

/// Typed-trigger: free instruction on the text written before it.
pub fn trigger_system(settings: &Settings) -> String {
    let mut s = String::from(
        "You are Keeyz, an assistant built into the user's keyboard. The user typed some text and then an instruction about it.\n\
Follow the instruction on the text and return the full replacement text.\n\
- If the instruction is a question or asks to write something new, return only that new content.\n\
- Keep the writer's register (formal stays formal, casual stays casual) unless told otherwise.\n\
- Fix obvious typos while you are at it.\n",
    );
    s.push_str(OUTPUT_RULES);
    if settings.prompt_context_enabled && !settings.prompt_context.trim().is_empty() {
        s.push_str("\n\nBackground context from the user (keep in mind):\n");
        s.push_str(settings.prompt_context.trim());
    }
    if settings.style_enabled && !settings.style_text.trim().is_empty() {
        s.push_str("\n\nUser style preferences (tone and wording only):\n");
        s.push_str(settings.style_text.trim());
    }
    s
}

pub const GENERATE_SYSTEM: &str = "You are a versatile content generator inside a desktop writing tool. Write exactly what the user describes (email, list, message, code, essay…), matching the implied format. Output only the content: no explanations, labels or markdown fences.";

pub const CHAT_SYSTEM: &str = "You are Keeyz's chat assistant. Be concise, direct and helpful. Answer in the user's language. No preamble.";

pub const IMPROVE_INSTRUCTION: &str = "Rewrite this instruction for an AI text assistant so it is clear, specific and unambiguous. One or two sentences. Keep the original intent and any language names exactly. Return only the improved instruction.";

pub const IMPROVE_PROMPT: &str = "You are a prompt engineering expert. Expand this short system prompt for an AI writing assistant into a thorough, actionable one: add rules for tone, formatting, edge cases and a short example where useful. Keep the original intent. Return only the improved prompt.";

pub const VOICE_POLISH: &str = "Clean up this dictated text: fix punctuation, capitalisation and grammar, remove filler words (um, uh, like, you know) and false starts. Keep the speaker's words and meaning; do not add content. Return only the cleaned text.";

pub const SUMMARIZE_PAGE: &str = "Summarize this page for a busy reader: a one-line gist, then 3-7 bullets with the key points using '- '. Use the page's language.";
