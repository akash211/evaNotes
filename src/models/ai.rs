use serde::{Deserialize, Serialize};

/// Rewrite styles offered by the AI assistant.
pub const AI_STYLES: [(&str, &str); 6] = [
    ("minimal", "Minimal changes — fix typos & grammar only"),
    ("formal", "Formal"),
    ("casual", "Casual"),
    ("concise", "Concise — shorten"),
    ("professional", "Professional"),
    ("friendly", "Friendly"),
];

pub fn style_prompt(style: &str) -> &'static str {
    match style {
        "minimal" => "You are a careful editor. Fix spelling, grammar and punctuation. Do NOT change meaning, tone, structure or length. Return only the corrected text, no explanations.",
        "formal" => "You are a writing assistant. Rewrite the text in a formal, polished, business-like tone. Keep the original meaning. Return only the rewritten text, no explanations.",
        "casual" => "You are a writing assistant. Rewrite the text in a relaxed, casual, conversational tone. Keep the original meaning. Return only the rewritten text, no explanations.",
        "concise" => "You are a writing assistant. Rewrite the text more concisely: remove redundancy, tighten sentences, keep the key meaning. Return only the rewritten text, no explanations.",
        "professional" => "You are a writing assistant. Rewrite the text in a clear, professional style suitable for work documents or emails. Keep the original meaning. Return only the rewritten text, no explanations.",
        "friendly" => "You are a writing assistant. Rewrite the text in a warm, friendly, approachable tone. Keep the original meaning. Return only the rewritten text, no explanations.",
        _ => "You are a careful editor. Improve the text while keeping its meaning. Return only the rewritten text, no explanations.",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRewriteRequest {
    pub text: String,
    pub model: String,
    #[serde(default = "default_ai_style")]
    pub style: String,
}

fn default_ai_style() -> String {
    "minimal".to_string()
}
