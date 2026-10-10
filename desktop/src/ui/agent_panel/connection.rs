//! The agent's service as the panel names it: the provider, whether it can chat right now
//! (`agent.connection`), the maker's mark beside a model, reasoning effort names, and errors
//! in words a musician can act on.

use ryolune_engine::settings::Provider;
use serde_json::Value;

/// The short name the header and the model menu show (Settings > Agent uses the long one).
pub fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Lsuite => "lsuite AI",
        Provider::Codex => "Codex",
        Provider::Claude => "Claude Code",
        Provider::Anthropic => "Anthropic API",
        Provider::OpenAi => "OpenAI API",
        Provider::Gemini => "Google Gemini",
        Provider::OpenRouter => "OpenRouter",
        Provider::Mistral => "Mistral",
        Provider::Groq => "Groq",
        Provider::DeepSeek => "DeepSeek",
        Provider::Xai => "xAI Grok",
        Provider::Ollama => "Ollama",
        Provider::LmStudio => "LM Studio",
        Provider::Compatible => "Other compatible server",
    }
}

/// The provider named by its settings key, its short name, or the key itself.
pub fn provider_name_of(key: &str) -> String {
    Provider::parse(key).map_or_else(|| key.to_string(), |p| provider_name(p).to_string())
}

/// What `agent.connection` says about the configured service. Checking never sends a prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub provider: String,
    pub state: String,
    pub message: String,
}

impl Connection {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            provider: value["provider"].as_str()?.to_string(),
            state: value["state"].as_str()?.to_string(),
            message: value["message"].as_str().unwrap_or("").to_string(),
        })
    }
    /// Signed in or configured: a message can go.
    pub fn can_chat(&self) -> bool {
        matches!(self.state.as_str(), "signedIn" | "configured")
    }
}

/// A reasoning effort as people read it.
pub fn effort_name(value: &str) -> String {
    match value {
        "xhigh" => "Extra high",
        "ultra" => "Ultra",
        "none" => "Off",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "max" => "Max",
        other => other,
    }
    .to_string()
}

/// The maker of a model, for its mark: (name, file in `assets/providers`). A model served by
/// a compatible API still shows its maker; an unknown one gets no invented logo.
pub fn model_brand(provider: &str, model: &str) -> Option<(&'static str, &'static str)> {
    let lower = model.to_lowercase();
    let id = lower.rsplit('/').next().unwrap_or("");
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| id.starts_with(p));
    if starts(&["qwen", "qwq"]) {
        return Some(("Qwen", "qwen-color"));
    }
    if starts(&["kimi", "moonshot"]) {
        return Some(("Kimi", "kimi-color"));
    }
    if starts(&["deepseek"]) {
        return Some(("DeepSeek", "deepseek-color"));
    }
    if starts(&["gemini"]) {
        return Some(("Google Gemini", "gemini-color"));
    }
    if starts(&["mistral", "mixtral", "codestral", "devstral", "magistral"]) {
        return Some(("Mistral", "mistral-color"));
    }
    if starts(&["llama", "meta-llama"]) {
        return Some(("Meta", "meta-color"));
    }
    if starts(&["minimax"]) {
        return Some(("MiniMax", "minimax-color"));
    }
    if starts(&["grok"]) {
        return Some(("xAI", "grok"));
    }
    if matches!(provider, "claude" | "anthropic") || id.starts_with("claude") {
        return Some(("Anthropic", "claude-color"));
    }
    let o_series = id.len() >= 2
        && id.starts_with('o')
        && id.as_bytes()[1].is_ascii_digit()
        && id.as_bytes()[1] != b'0'
        && id[2..].chars().next().is_none_or(|c| c == '-');
    if matches!(provider, "codex" | "openai") || id.starts_with("gpt-") || o_series {
        return Some(("OpenAI", "openai"));
    }
    None
}

/// The lsuite AI allowance ran out: the panel says it in one line, with Manage plan.
pub fn allowance_used(error: &str) -> bool {
    error == ryolune_engine::account::ALLOWANCE_MESSAGE
}

/// An agent error in words a musician can act on; `unsent` when the message never left.
pub fn error_message(error: &str, unsent: bool) -> &'static str {
    if allowance_used(error) {
        return ryolune_engine::account::ALLOWANCE_MESSAGE;
    }
    let text = error.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if any(&[
        "429",
        "rate limit",
        "ratelimit",
        "rate-limit",
        "rate_limit",
        "usage limit",
        "usage credits",
        "quota",
        "insufficient_quota",
    ]) {
        return "Your AI service has reached a usage limit. Check your account allowance or choose another service.";
    }
    let invalid_key = text
        .find("invalid")
        .is_some_and(|at| text[at..].contains("key") || text[at..].contains("token"));
    if invalid_key
        || any(&[
            "401",
            "403",
            "unauthorized",
            "unauthorised",
            "not logged in",
            "authentication",
        ])
    {
        return "Your AI service could not verify your account. Sign in again or check your API key in Agent settings.";
    }
    if any(&[
        "connection refused",
        "could not connect",
        "failed to connect",
        "error sending request",
    ]) {
        return "ryolune could not reach your AI service. Check your connection and, for a local model, make sure its server is running.";
    }
    if unsent {
        "Your message was not sent. It is still in the box below; check the details and try again."
    } else {
        "The agent could not finish this request. Completed edits are kept in your project and in Changes."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identifies_compatible_model_makers_without_inventing_logos() {
        assert_eq!(
            model_brand("compatible", "Qwen/Qwen3-32B").unwrap().0,
            "Qwen"
        );
        assert_eq!(
            model_brand("compatible", "moonshotai/kimi-k2").unwrap().0,
            "Kimi"
        );
        assert!(model_brand("compatible", "my-private-model").is_none());
        assert_eq!(model_brand("anthropic", "").unwrap().1, "claude-color");
        assert_eq!(
            model_brand("openrouter", "openai/o3-mini").unwrap().0,
            "OpenAI"
        );
        assert_eq!(model_brand("compatible", "gpt-5").unwrap().1, "openai");
        assert!(model_brand("compatible", "o0-thing").is_none());
    }

    #[test]
    fn every_provider_has_a_short_name_and_connections_read_their_state() {
        for provider in Provider::ALL {
            assert!(!provider_name(provider).is_empty());
            assert_ne!(provider_name_of(provider.key()), provider.key());
        }
        let ready = Connection::from_json(
            &json!({"provider":"codex","state":"signedIn","message":"Signed in."}),
        )
        .unwrap();
        assert!(ready.can_chat());
        let missing =
            Connection::from_json(&json!({"provider":"codex","state":"missingCli","message":""}))
                .unwrap();
        assert!(!missing.can_chat());
        assert!(Connection::from_json(&json!({})).is_none());
    }

    #[test]
    fn errors_are_explained() {
        assert!(error_message("HTTP 429 Too Many Requests", false).contains("usage limit"));
        assert!(error_message("Invalid API key", false).contains("verify your account"));
        assert!(error_message("error sending request for url", false).contains("could not reach"));
        assert!(error_message("boom", true).starts_with("Your message was not sent"));
        assert!(error_message("boom", false).starts_with("The agent could not finish"));
        let used = ryolune_engine::account::ALLOWANCE_MESSAGE;
        assert!(allowance_used(used));
        assert_eq!(error_message(used, false), used);
        assert_eq!(effort_name("xhigh"), "Extra high");
        assert_eq!(effort_name("custom"), "custom");
    }
}
