use crate::config::AppConfig;
use crate::error::{AppError, Result};
use crate::models::ai::style_prompt;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<String>,
}

/// Whether the AI assistant is configured.
pub fn ai_configured(config: &AppConfig) -> bool {
    config.ai_api_key.is_some()
}

/// Validate a requested model against the configured allowlist.
pub fn validate_model(config: &AppConfig, model: &str) -> Result<()> {
    if config.ai_models.is_empty() {
        return Err(AppError::BadRequest(
            "AI_MODELS is not configured on the server".into(),
        ));
    }
    if !config.ai_models.iter().any(|m| m == model) {
        return Err(AppError::BadRequest(format!(
            "Model '{model}' is not in the configured model list"
        )));
    }
    Ok(())
}

/// Rewrite `text` using the configured OpenAI-compatible API.
pub async fn rewrite_text(
    config: &AppConfig,
    text: &str,
    model: &str,
    style: &str,
) -> Result<String> {
    let api_key = config
        .ai_api_key
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("AI is not configured (AI_API_KEY missing)".into()))?;

    validate_model(config, model)?;

    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::BadRequest("Nothing to rewrite".into()));
    }
    if text.len() > 20_000 {
        return Err(AppError::BadRequest(
            "Text too long for AI rewrite (max 20000 characters)".into(),
        ));
    }

    let url = format!("{}/chat/completions", config.ai_api_base);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to build HTTP client: {e}")))?;

    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "model": model,
            "messages": [
                { "role": "system", "content": style_prompt(style) },
                { "role": "user", "content": text }
            ],
            "temperature": 0.4,
            "max_tokens": 4096,
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to reach AI API: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(
            "AI API returned {status}: {}",
            body.chars().take(300).collect::<String>()
        );
        return Err(AppError::Internal(format!(
            "AI API error ({status}). Check AI_API_KEY / AI_API_BASE."
        )));
    }

    let parsed: ChatCompletionResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse AI response: {e}")))?;

    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::Internal("AI returned an empty response".into()))?;

    Ok(content.trim().to_string())
}
