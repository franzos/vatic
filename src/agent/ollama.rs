use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};

use crate::config::types::AgentSection;
use crate::env::EnvironmentWrapper;
use crate::error::{Error, Result};

use super::Agent;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// 5 minutes — models can be slow, especially on CPU
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub struct OllamaAgent {
    host: String,
    model: String,
    client: Client,
}

impl OllamaAgent {
    pub fn new(config: &AgentSection) -> Self {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            host: config
                .host
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            model: config.model.clone().unwrap_or_else(|| "gemma3".to_string()),
            client,
        }
    }

    /// Build the request body for Ollama's `/api/generate`.
    pub fn build_request_body(&self, prompt: &str, system_prompt: Option<&str>) -> Value {
        let mut body = json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
        });

        if let Some(sp) = system_prompt {
            body["system"] = json!(sp);
        }

        body
    }
}

#[async_trait]
impl Agent for OllamaAgent {
    async fn run(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        _env_wrapper: &dyn EnvironmentWrapper,
    ) -> Result<String> {
        let body = self.build_request_body(prompt, system_prompt);
        let url = format!("{}/api/generate", self.host);

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("ollama request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(Error::Agent(format!("ollama returned {status}: {text}")));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to parse ollama response: {e}")))?;

        parse_response(&json)
    }
}

/// Pull the `response` field out of Ollama's JSON reply.
pub fn parse_response(json: &serde_json::Value) -> Result<String> {
    json["response"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| Error::Agent("ollama response missing 'response' field".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AgentSection;

    fn make_agent(host: Option<&str>, model: Option<&str>) -> OllamaAgent {
        let config = AgentSection {
            name: "ollama".to_string(),
            prompt: None,
            host: host.map(|s| s.to_string()),
            model: model.map(|s| s.to_string()),
            skip_permissions: None,
            allowed_tools: None,
        };
        OllamaAgent::new(&config)
    }

    #[test]
    fn test_ollama_request_body() {
        let agent = make_agent(None, Some("gemma3"));
        let body = agent.build_request_body("What is Rust?", Some("You are helpful."));
        assert_eq!(body["model"], "gemma3");
        assert_eq!(body["prompt"], "What is Rust?");
        assert_eq!(body["system"], "You are helpful.");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn test_ollama_request_body_with_system() {
        let agent = make_agent(Some("http://myhost:11434"), Some("llama3"));
        let body = agent.build_request_body("Hello", Some("Be concise."));
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["prompt"], "Hello");
        assert_eq!(body["system"], "Be concise.");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn test_ollama_request_body_no_system() {
        let agent = make_agent(None, None);
        let body = agent.build_request_body("Tell me a joke.", None);
        assert_eq!(body["model"], "gemma3");
        assert_eq!(body["prompt"], "Tell me a joke.");
        assert!(body.get("system").is_none());
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn test_parse_response_valid() {
        let json = json!({"response": "Hello!"});
        let result = parse_response(&json).unwrap();
        assert_eq!(result, "Hello!");
    }

    #[test]
    fn test_parse_response_missing_field() {
        let json = json!({"model": "gemma3"});
        let err = parse_response(&json).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("missing 'response'"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_parse_response_null() {
        let json = json!({"response": null});
        assert!(parse_response(&json).is_err());
    }

    #[test]
    fn test_parse_response_numeric() {
        let json = json!({"response": 42});
        assert!(parse_response(&json).is_err());
    }

    #[test]
    fn test_parse_response_empty_string() {
        let json = json!({"response": ""});
        let result = parse_response(&json).unwrap();
        assert_eq!(result, "");
    }
}
