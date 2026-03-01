pub mod claude;
pub mod ollama;

use async_trait::async_trait;

use crate::config::types::AgentSection;
use crate::env::EnvironmentWrapper;
use crate::error::Result;

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
    match config.name {
        crate::config::types::AgentName::Claude => Ok(Box::new(claude::ClaudeAgent::new(config))),
        crate::config::types::AgentName::Ollama => Ok(Box::new(ollama::OllamaAgent::new(config))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AgentSection;

    use crate::config::types::AgentName;

    fn agent_config(name: AgentName) -> AgentSection {
        AgentSection {
            name,
            prompt: None,
            host: None,
            model: None,
            skip_permissions: None,
            allowed_tools: None,
        }
    }

    #[test]
    fn test_create_claude_agent() {
        let result = create_agent(&agent_config(AgentName::Claude));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_ollama_agent() {
        let result = create_agent(&agent_config(AgentName::Ollama));
        assert!(result.is_ok());
    }
}
