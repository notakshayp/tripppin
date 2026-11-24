//! Cloud providers the AI can use instead of the model on this computer, with what Settings
//! needs to guide someone through connecting one: the address, a sensible model to start with,
//! where to get a key and how to check it. Every one of them speaks the OpenAI chat API with
//! tool calls; listing models differs a little between them, which is handled here.
//!
//! Keys never appear in errors or logs: messages from providers are scrubbed of the key before
//! they are shown.

use crate::model::Endpoint;
use anyhow::{Result, anyhow, bail};

/// How a provider wants the key when listing models.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Auth {
    /// `Authorization: Bearer <key>` (the OpenAI way; most providers).
    Bearer,
    /// Anthropic's own API: `x-api-key` and `anthropic-version`.
    Anthropic,
    /// Sarvam: `api-subscription-key`, and the bearer header too.
    Sarvam,
}

/// How to check a key and find the models it can use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Listing {
    /// GET {url}/models with the key; a refused key is a 401 or 403.
    Models,
    /// OpenRouter: the model list is public, so the key is checked at {url}/key first.
    OpenRouter,
    /// No list endpoint: the key is checked with a one-word chat, and these are offered.
    Fixed(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    /// Base URL, before /chat/completions. Empty for "your own server".
    pub url: &'static str,
    /// The model to preselect when the provider lists it.
    pub model: &'static str,
    /// Others to prefer, in order, when `model` isn't listed (parts of model ids).
    pub prefer: &'static [&'static str],
    /// Where to make a key. Empty when no key is needed.
    pub keys: &'static str,
    /// One line about it.
    pub about: &'static str,
    pub auth: Auth,
    pub listing: Listing,
}

pub const PROVIDERS: [Provider; 8] = [
    Provider {
        id: "openai",
        name: "OpenAI",
        url: "https://api.openai.com/v1",
        model: "gpt-6-luna",
        prefer: &["gpt-6", "gpt-5"],
        keys: "https://platform.openai.com/api-keys",
        about: "GPT models. Pay as you go.",
        auth: Auth::Bearer,
        listing: Listing::Models,
    },
    Provider {
        id: "anthropic",
        name: "Anthropic",
        url: "https://api.anthropic.com/v1",
        model: "claude-sonnet-5-5",
        prefer: &["claude-sonnet", "claude-opus"],
        keys: "https://platform.claude.com/settings/keys",
        about: "Claude models, through Anthropic's OpenAI-compatible API.",
        auth: Auth::Anthropic,
        listing: Listing::Models,
    },
    Provider {
        id: "gemini",
        name: "Google Gemini",
        url: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-3.8-flash",
        prefer: &["flash", "pro"],
        keys: "https://aistudio.google.com/apikey",
        about: "Gemini models. Has a free tier.",
        auth: Auth::Bearer,
        listing: Listing::Models,
    },
    Provider {
        id: "deepseek",
        name: "DeepSeek",
        url: "https://api.deepseek.com",
        model: "deepseek-flash",
        prefer: &["deepseek-chat"],
        keys: "https://platform.deepseek.com/api_keys",
        about: "DeepSeek models. Low prices.",
        auth: Auth::Bearer,
        listing: Listing::Models,
    },
    Provider {
        id: "sarvam",
        name: "Sarvam AI",
        url: "https://api.sarvam.ai/v1",
        model: "sarvam-105b",
        prefer: &[],
        keys: "https://indus.sarvam.ai/key-management",
        about: "Made in India, strong in Indian languages.",
        auth: Auth::Sarvam,
        listing: Listing::Fixed(&["sarvam-105b", "sarvam-105b-conversations"]),
    },
    Provider {
        id: "openrouter",
        name: "OpenRouter",
        url: "https://openrouter.ai/api/v1",
        model: "openai/gpt-6-luna",
        prefer: &["anthropic/claude-sonnet", "openai/gpt"],
        keys: "https://openrouter.ai/settings/keys",
        about: "One key for models from many companies.",
        auth: Auth::Bearer,
        listing: Listing::OpenRouter,
    },
    Provider {
        id: "groq",
        name: "Groq",
        url: "https://api.groq.com/openai/v1",
        model: "",
        prefer: &["gpt-oss-120b", "kimi-k2", "llama-3.3-70b", "qwen"],
        keys: "https://console.groq.com/keys",
        about: "Open models, very fast.",
        auth: Auth::Bearer,
        listing: Listing::Models,
    },
    Provider {
        id: "own",
        name: "Your own server",
        url: "",
        model: "",
        prefer: &[],
        keys: "",
        about: "llama.cpp, Ollama, LM Studio or vLLM on another machine. No key needed.",
        auth: Auth::Bearer,
        listing: Listing::Models,
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

/// A model a provider offers.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteModel {
    /// What requests name it by.
    pub id: String,
    /// What the provider calls it, when it says; otherwise the id.
    pub name: String,
}

/// Checks the key and lists the chat models it can use, sorted by id. Errors say plainly what
/// went wrong: a refused key, no network, or the provider's own message.
pub fn list_models(e: &Endpoint) -> Result<Vec<RemoteModel>> {
    let p = for_url(&e.url);
    if p.id != "own" && e.key.trim().is_empty() {
        bail!("Paste the key from {} first.", p.name);
    }
    match p.listing {
        Listing::Models => {
            let suffix = if p.auth == Auth::Anthropic { "/models?limit=1000" } else { "/models" };
            let text = get(e, p.auth, &format!("{}{suffix}", e.url))?;
            Ok(parse_models(&text))
        }
        Listing::OpenRouter => {
            get(e, p.auth, &format!("{}/key", e.url))?;
            let text = get(e, p.auth, &format!("{}/models", e.url))?;
            Ok(parse_models(&text))
        }
        Listing::Fixed(ids) => {
            let check = Endpoint { model: ids.first().copied().unwrap_or(p.model).to_string(), ..e.clone() };
            crate::model::test_endpoint(&check).map_err(|err| friendly_test_error(&err.to_string(), &e.key))?;
            Ok(ids.iter().map(|id| RemoteModel { id: id.to_string(), name: id.to_string() }).collect())
        }
    }
}

/// The ids only (kept for callers that just need names).
pub fn models(e: &Endpoint) -> Result<Vec<String>> {
    Ok(list_models(e)?.into_iter().map(|m| m.id).collect())
}

/// Which model to preselect: the provider's usual one if listed, then its preferred families
/// (the newest-looking id of the first family found), then the first in the list.
pub fn default_model(p: &Provider, models: &[RemoteModel]) -> Option<usize> {
    if models.is_empty() {
        return None;
    }
    if let Some(i) = models.iter().position(|m| !p.model.is_empty() && m.id == p.model) {
        return Some(i);
    }
    for want in p.prefer {
        let best = models
            .iter()
            .enumerate()
            .filter(|(_, m)| m.id.contains(want))
            // Prefer plain ids over dated or preview ones, then the greatest (usually newest) id.
            .max_by_key(|(_, m)| (!m.id.contains("preview") && !m.id.contains("mini"), m.id.clone()));
        if let Some((i, _)) = best {
            return Some(i);
        }
    }
    Some(0)
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url)
}

fn get(e: &Endpoint, auth: Auth, url: &str) -> Result<String> {
    let mut req = crate::model::http_agent().get(url).config().http_status_as_error(false).build();
    let key = e.key.trim();
    if !key.is_empty() {
        match auth {
            Auth::Bearer => req = req.header("Authorization", &format!("Bearer {key}")),
            Auth::Anthropic => {
                req = req.header("x-api-key", key).header("anthropic-version", "2023-06-01");
            }
            Auth::Sarvam => {
                req = req.header("api-subscription-key", key).header("Authorization", &format!("Bearer {key}"));
            }
        }
    }
    let mut resp = req.call().map_err(|_| {
        anyhow!("Couldn't reach {}. Check that this computer is online and the address is right.", host(&e.url))
    })?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    if status == 200 {
        return Ok(text);
    }
    Err(status_error(status, &text, key))
}

/// A plain sentence for a provider's error answer. The key is scrubbed from anything it says.
pub fn status_error(status: u16, body: &str, key: &str) -> anyhow::Error {
    match status {
        401 | 403 => anyhow!("The provider turned the key down. Check that you copied all of it."),
        402 => anyhow!("The provider says the account has no credit left."),
        404 => anyhow!("The server doesn't list its models. Type the model name instead."),
        429 => anyhow!("Too many requests, or the account is out of credit. Try again in a minute."),
        _ => {
            let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            let err = v.get("error").unwrap_or(&v);
            let message =
                err.get("message").or_else(|| err.as_str().map(|_| err)).and_then(|m| m.as_str()).unwrap_or("").trim();
            let message = scrub(message, key);
            let message: String = message.chars().take(200).collect();
            if message.is_empty() {
                anyhow!("The provider answered with an error ({status}).")
            } else {
                anyhow!("The provider answered with an error ({status}): {message}")
            }
        }
    }
}

fn friendly_test_error(message: &str, key: &str) -> anyhow::Error {
    let m = scrub(message, key);
    if m.contains("turned the key down") {
        anyhow!("The provider turned the key down. Check that you copied all of it.")
    } else if m.contains("couldn't reach") {
        anyhow!("Couldn't reach the provider. Check that this computer is online.")
    } else {
        anyhow!("{}", sentence(&m))
    }
}

fn sentence(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Removes the key (and any long run of it) from text that might be shown.
fn scrub(text: &str, key: &str) -> String {
    let key = key.trim();
    if key.len() < 6 { text.to_string() } else { text.replace(key, "…") }
}

/// The chat models in a provider's list, sorted. Embedding, audio, image and moderation models
/// are left out, as are models a provider marks inactive (Groq) or without tool calls
/// (OpenRouter's `supported_parameters`).
pub fn parse_models(json: &str) -> Vec<RemoteModel> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let list = v.get("data").or_else(|| v.get("models")).and_then(|d| d.as_array()).cloned().unwrap_or_default();
    let skip = [
        "embed",
        "whisper",
        "tts",
        "dall-e",
        "image",
        "moderation",
        "audio",
        "transcribe",
        "realtime",
        "guard",
        "babbage",
        "davinci",
        "sora",
        "imagen",
        "veo",
        "aqa",
    ];
    let mut models: Vec<RemoteModel> = list
        .iter()
        .filter(|m| m.get("active").and_then(|a| a.as_bool()) != Some(false))
        .filter(|m| {
            m.get("supported_parameters")
                .and_then(|s| s.as_array())
                .is_none_or(|s| s.iter().any(|p| p.as_str() == Some("tools")))
        })
        .filter_map(|m| {
            let id = m.get("id").or_else(|| m.get("name")).and_then(|i| i.as_str())?;
            // Gemini lists "models/gemini-…"; requests take the bare id.
            let id = id.trim_start_matches("models/").to_string();
            let name = m
                .get("display_name")
                .or_else(|| m.get("displayName"))
                .or_else(|| m.get("name").filter(|_| m.get("id").is_some()))
                .and_then(|n| n.as_str())
                .filter(|n| !n.is_empty() && !n.starts_with("models/"))
                .map_or_else(|| id.clone(), str::to_string);
            Some(RemoteModel { id, name })
        })
        .filter(|m| !skip.iter().any(|s| m.id.to_lowercase().contains(s)))
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

/// Models whose id or name contains every word of `query`, ignoring case.
pub fn search<'a>(models: &'a [RemoteModel], query: &str) -> Vec<&'a RemoteModel> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    models
        .iter()
        .filter(|m| {
            let hay = format!("{} {}", m.id, m.name).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(models: &[RemoteModel]) -> Vec<&str> {
        models.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn knows_providers_by_address() {
        assert_eq!(for_url("https://api.deepseek.com/").id, "deepseek");
        assert_eq!(for_url("https://api.sarvam.ai/v1").id, "sarvam");
        assert_eq!(for_url("http://192.168.1.5:8080/v1").id, "own");
        assert!(PROVIDERS.iter().all(|p| p.id == "own" || p.url.starts_with("https://")));
        assert_eq!(by_id("anthropic").map(|p| p.auth), Some(Auth::Anthropic));
    }

    #[test]
    fn lists_chat_models_only() {
        let json = r#"{"data":[{"id":"gpt-6-luna"},{"id":"text-embedding-3-small"},{"id":"whisper-1"},{"id":"gpt-6.1-terra"}]}"#;
        assert_eq!(ids(&parse_models(json)), ["gpt-6-luna", "gpt-6.1-terra"]);
        let gemini = r#"{"object":"list","data":[{"id":"models/gemini-3.8-flash","display_name":"Gemini 3.8 Flash"},{"id":"models/gemini-embedding-001"}]}"#;
        let g = parse_models(gemini);
        assert_eq!(ids(&g), ["gemini-3.8-flash"]);
        assert_eq!(g[0].name, "Gemini 3.8 Flash");
        assert!(parse_models("nope").is_empty());
    }

    #[test]
    fn reads_each_providers_shape() {
        let anthropic = r#"{"data":[{"type":"model","id":"claude-sonnet-5-5","display_name":"Claude Sonnet 5.5","created_at":"2026-09-01T00:00:00Z"}],"has_more":false}"#;
        assert_eq!(parse_models(anthropic)[0].name, "Claude Sonnet 5.5");
        let groq = r#"{"data":[{"id":"llama-3.3-70b-versatile","active":true},{"id":"old-model","active":false},{"id":"whisper-large-v3"}]}"#;
        assert_eq!(ids(&parse_models(groq)), ["llama-3.3-70b-versatile"]);
        let openrouter = r#"{"data":[{"id":"openai/gpt-6-luna","name":"OpenAI: GPT-6 Luna","supported_parameters":["tools","temperature"]},{"id":"some/model","name":"No tools","supported_parameters":["temperature"]}]}"#;
        let o = parse_models(openrouter);
        assert_eq!(ids(&o), ["openai/gpt-6-luna"]);
        assert_eq!(o[0].name, "OpenAI: GPT-6 Luna");
    }

    #[test]
    fn picks_a_sensible_default() {
        let m = |ids: &[&str]| ids.iter().map(|i| RemoteModel { id: i.to_string(), name: i.to_string() }).collect();
        let openai: Vec<RemoteModel> = m(&["gpt-4o", "gpt-6-luna", "o9"]);
        assert_eq!(default_model(by_id("openai").unwrap(), &openai), Some(1));
        let groq: Vec<RemoteModel> = m(&["gemma2-9b-it", "llama-3.3-70b-versatile", "qwen-3-32b"]);
        assert_eq!(default_model(by_id("groq").unwrap(), &groq), Some(1));
        let other: Vec<RemoteModel> = m(&["a", "b"]);
        assert_eq!(default_model(by_id("own").unwrap(), &other), Some(0));
        assert_eq!(default_model(by_id("own").unwrap(), &[]), None);
    }

    #[test]
    fn errors_never_show_the_key() {
        let key = "sk-secret-123456";
        let body = r#"{"error":{"message":"bad things happened with sk-secret-123456"}}"#;
        let e = status_error(500, body, key).to_string();
        assert!(!e.contains(key), "{e}");
        assert!(status_error(401, "", key).to_string().contains("turned the key down"));
    }

    #[test]
    fn searches_by_words() {
        let models = vec![
            RemoteModel { id: "openai/gpt-6-luna".into(), name: "OpenAI: GPT-6 Luna".into() },
            RemoteModel { id: "anthropic/claude-sonnet-5-5".into(), name: "Claude Sonnet".into() },
        ];
        assert_eq!(search(&models, "claude son").len(), 1);
        assert_eq!(search(&models, "").len(), 2);
    }
}
