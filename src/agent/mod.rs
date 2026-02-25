pub mod claude;
pub mod ollama;

use async_trait::async_trait;

use crate::config::types::AgentSection;
use crate::env::EnvironmentWrapper;
use crate::error::{Error, Result};

#[async_trait]
pub trait Agent: Send + Sync {
    async fn run(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        env_wrapper: &dyn EnvironmentWrapper,
    ) -> Result<String>;
}

/// Factory — maps an agent name from config to its implementation.
pub fn create_agent(config: &AgentSection) -> Result<Box<dyn Agent>> {
    match config.name.as_str() {
        "claude" => Ok(Box::new(claude::ClaudeAgent::new(config))),
        "ollama" => Ok(Box::new(ollama::OllamaAgent::new(config))),
        other => Err(Error::Agent(format!("unknown agent: '{other}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AgentSection;

    fn agent_config(name: &str) -> AgentSection {
        AgentSection {
            name: name.to_string(),
            prompt: None,
            host: None,
            model: None,
            skip_permissions: None,
            allowed_tools: None,
        }
    }

    #[test]
    fn test_create_claude_agent() {
        let result = create_agent(&agent_config("claude"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_ollama_agent() {
        let result = create_agent(&agent_config("ollama"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_unknown_agent() {
        let result = create_agent(&agent_config("unknown"));
        match result {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("unknown agent"),
                    "expected 'unknown agent' in: {msg}"
                );
            }
            Ok(_) => panic!("expected Err for unknown agent"),
        }
    }
}
