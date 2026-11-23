//! Cloud providers the AI can use instead of the model on this computer, with what Settings
//! needs to guide someone through connecting one: the address, a sensible model to start with
//! and where to get a key. Every one of them speaks the OpenAI chat API with tool calls.

use crate::model::Endpoint;
use anyhow::{Context, Result, bail};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    /// Base URL, before /chat/completions. Empty for "your own server".
    pub url: &'static str,
    /// A model to start with; the list from the provider replaces it when it loads.
    pub model: &'static str,
    /// Where to make a key. Empty when no key is needed.
    pub keys: &'static str,
    /// One line about it.
    pub about: &'static str,
}

pub const PROVIDERS: [Provider; 8] = [
    Provider {
        id: "openai",
        name: "OpenAI",
        url: "https://api.openai.com/v1",
        model: "gpt-6-luna",
        keys: "https://platform.openai.com/api-keys",
        about: "gpt models. pay as you go.",
    },
    Provider {
        id: "anthropic",
        name: "Anthropic",
        url: "https://api.anthropic.com/v1",
        model: "claude-sonnet-5-5",
        keys: "https://platform.claude.com/settings/keys",
        about: "claude models, through anthropic's openai-compatible api.",
    },
    Provider {
        id: "gemini",
        name: "Google Gemini",
        url: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-3.8-flash",
        keys: "https://aistudio.google.com/apikey",
        about: "gemini models. has a free tier.",
    },
    Provider {
        id: "deepseek",
        name: "DeepSeek",
        url: "https://api.deepseek.com",
        model: "deepseek-flash",
        keys: "https://platform.deepseek.com/api_keys",
        about: "deepseek models. low prices.",
    },
    Provider {
        id: "sarvam",
        name: "Sarvam AI",
        url: "https://api.sarvam.ai/v1",
        model: "sarvam-105b",
        keys: "https://indus.sarvam.ai/key-management",
        about: "made in india, strong in indian languages.",
    },
    Provider {
        id: "openrouter",
        name: "OpenRouter",
        url: "https://openrouter.ai/api/v1",
        model: "openai/gpt-6-luna",
        keys: "https://openrouter.ai/settings/keys",
        about: "one key for models from many companies.",
    },
    Provider {
        id: "groq",
        name: "Groq",
        url: "https://api.groq.com/openai/v1",
        model: "",
        keys: "https://console.groq.com/keys",
        about: "open models, very fast.",
    },
    Provider {
        id: "own",
        name: "your own server",
        url: "",
        model: "",
        keys: "",
        about: "llama.cpp, ollama, lm studio or vllm on another machine. no key needed.",
    },
];

/// The provider an endpoint belongs to, by its address; "your own server" otherwise.
pub fn for_url(url: &str) -> &'static Provider {
    let url = url.trim_end_matches('/');
    PROVIDERS.iter().find(|p| !p.url.is_empty() && url == p.url).unwrap_or(&PROVIDERS[PROVIDERS.len() - 1])
}

pub fn by_id(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The model ids a server offers (GET {url}/models), sorted. Chat models only, as far as the
/// name tells: embedding, audio, image and moderation models are left out.
pub fn models(e: &Endpoint) -> Result<Vec<String>> {
    let mut req =
        crate::model::http_agent().get(&format!("{}/models", e.url)).config().http_status_as_error(false).build();
    if !e.key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", e.key));
    }
    let mut resp = req.call().with_context(|| format!("couldn't reach {}", e.url))?;
    match resp.status().as_u16() {
        200 => {}
        401 | 403 => bail!("the server turned the key down"),
        status => bail!("the server didn't list its models ({status})"),
    }
    let text = resp.body_mut().read_to_string()?;
    Ok(parse_models(&text))
}

pub fn parse_models(json: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let list = v.get("data").or_else(|| v.get("models")).and_then(|d| d.as_array()).cloned().unwrap_or_default();
    let skip = ["embed", "whisper", "tts", "dall-e", "image", "moderation", "audio", "transcribe", "realtime", "guard"];
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|m| m.get("id").or_else(|| m.get("name")).and_then(|i| i.as_str()))
        // Gemini lists "models/gemini-…"; requests take the bare id.
        .map(|id| id.trim_start_matches("models/").to_string())
        .filter(|id| !skip.iter().any(|s| id.to_lowercase().contains(s)))
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_providers_by_address() {
        assert_eq!(for_url("https://api.deepseek.com/").id, "deepseek");
        assert_eq!(for_url("https://api.sarvam.ai/v1").id, "sarvam");
        assert_eq!(for_url("http://192.168.1.5:8080/v1").id, "own");
        assert!(PROVIDERS.iter().all(|p| p.id == "own" || p.url.starts_with("https://")));
        assert_eq!(by_id("anthropic").map(|p| p.name), Some("Anthropic"));
    }

    #[test]
    fn lists_chat_models_only() {
        let json = r#"{"data":[{"id":"gpt-6-luna"},{"id":"text-embedding-3-small"},{"id":"whisper-1"},{"id":"gpt-6.1-terra"}]}"#;
        assert_eq!(parse_models(json), ["gpt-6-luna", "gpt-6.1-terra"]);
        let gemini = r#"{"data":[{"id":"models/gemini-3.8-flash"},{"id":"models/gemini-embedding-001"}]}"#;
        assert_eq!(parse_models(gemini), ["gemini-3.8-flash"]);
        assert!(parse_models("nope").is_empty());
    }
}
