//! An [`AiProvider`] for any OpenAI-compatible `/chat/completions` endpoint:
//! NVIDIA NIM, Groq, OpenRouter, a local vLLM, and OpenAI itself.
//!
//! Behind the `openai` feature so the crate stays `reqwest`-free otherwise.

use super::{AiError, AiProvider};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Talks to a `POST {base_url}/chat/completions` endpoint with a bearer token.
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    temperature: f32,
}

impl OpenAiCompatProvider {
    /// `base_url` is the API root without a trailing slash, e.g.
    /// `https://integrate.api.nvidia.com/v1`. `request_timeout` bounds the whole
    /// round trip; on expiry the call surfaces as [`AiError::Timeout`].
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        temperature: f32,
        request_timeout: Duration,
    ) -> Result<Self, AiError> {
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .user_agent(concat!("sherwood-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AiError::Transport(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            api_key: api_key.into(),
            temperature,
        })
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [Message<'a>; 2],
    max_tokens: u32,
    temperature: f32,
    stream: bool,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    message: ChoiceMessage,
}

#[derive(Deserialize, Default)]
struct ChoiceMessage {
    #[serde(default)]
    content: String,
}

#[async_trait]
impl AiProvider for OpenAiCompatProvider {
    async fn complete(&self, system: &str, user: &str, max_tokens: u32) -> Result<String, AiError> {
        let body = ChatRequest {
            model: &self.model,
            messages: [
                Message {
                    role: "system",
                    content: system,
                },
                Message {
                    role: "user",
                    content: user,
                },
            ],
            max_tokens,
            temperature: self.temperature,
            stream: false,
        };

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    AiError::Timeout
                } else {
                    AiError::Transport(e.to_string())
                }
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let mut body: String = body.chars().take(500).collect();
            body.truncate(500);
            return Err(AiError::Status {
                status: status.as_u16(),
                body,
            });
        }

        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AiError::Transport(e.to_string()))?;

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .filter(|s| !s.trim().is_empty())
            .ok_or(AiError::Empty)
    }

    fn describe(&self) -> String {
        format!("openai-compat model={} @ {}", self.model, self.base_url)
    }
}
