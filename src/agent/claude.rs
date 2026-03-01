use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::types::AgentSection;
use crate::env::EnvironmentWrapper;
use crate::error::{Error, Result};

use super::Agent;

pub struct ClaudeAgent {
    model: Option<String>,
    skip_permissions: bool,
    allowed_tools: Option<Vec<String>>,
}

impl ClaudeAgent {
    pub fn new(config: &AgentSection) -> Self {
        Self {
            model: config.model.clone(),
            skip_permissions: config.skip_permissions.unwrap_or(true),
            allowed_tools: config.allowed_tools.clone(),
        }
    }

    /// Returns (command_name, args) before environment wrapping.
    pub fn build_args(&self, system_prompt: Option<&str>) -> (String, Vec<String>) {
        let mut args = vec!["--print".to_string()];

        if self.skip_permissions {
            args.push("--dangerously-skip-permissions".to_string());
        } else if let Some(tools) = &self.allowed_tools {
            for tool in tools {
                args.push("--allowedTools".to_string());
                args.push(tool.clone());
            }
        }

        if let Some(model) = &self.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        if let Some(sp) = system_prompt {
            args.push("--system-prompt".to_string());
            args.push(sp.to_string());
        }

        ("claude".to_string(), args)
    }
}

#[async_trait]
impl Agent for ClaudeAgent {
    async fn run(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        env_wrapper: &dyn EnvironmentWrapper,
    ) -> Result<String> {
        let (base_cmd, base_args) = self.build_args(system_prompt);
        let arg_refs: Vec<&str> = base_args.iter().map(|s| s.as_str()).collect();
        let (cmd, args) = env_wrapper.wrap_command(&base_cmd, &arg_refs);

        let mut child = Command::new(&cmd)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| Error::Agent(format!("failed to spawn '{cmd}': {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| Error::Agent(format!("failed to write to stdin: {e}")))?;
            // stdin drops here, signaling EOF to the child process
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| Error::Agent("claude process timed out after 5 minutes".to_string()))?
        .map_err(|e| Error::Agent(format!("failed to wait for process: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Agent(format!(
                "claude exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AgentSection;

    fn make_agent(model: Option<&str>) -> ClaudeAgent {
        let config = AgentSection {
            name: crate::config::types::AgentName::Claude,
            prompt: None,
            host: None,
            model: model.map(|s| s.to_string()),
            skip_permissions: None,
            allowed_tools: None,
        };
        ClaudeAgent::new(&config)
    }

    fn make_agent_with_permissions(skip: Option<bool>, tools: Option<Vec<String>>) -> ClaudeAgent {
        let config = AgentSection {
            name: crate::config::types::AgentName::Claude,
            prompt: None,
            host: None,
            model: None,
            skip_permissions: skip,
            allowed_tools: tools,
        };
        ClaudeAgent::new(&config)
    }

    #[test]
    fn test_claude_build_args_basic() {
        let agent = make_agent(None);
        let (cmd, args) = agent.build_args(None);
        assert_eq!(cmd, "claude");
        assert_eq!(args, vec!["--print", "--dangerously-skip-permissions"]);
    }

    #[test]
    fn test_claude_build_args_with_system_prompt() {
        let agent = make_agent(None);
        let (cmd, args) = agent.build_args(Some("You are a weather reporter."));
        assert_eq!(cmd, "claude");
        assert_eq!(
            args,
            vec![
                "--print",
                "--dangerously-skip-permissions",
                "--system-prompt",
                "You are a weather reporter."
            ]
        );
    }

    #[test]
    fn test_claude_build_args_with_model() {
        let agent = make_agent(Some("claude-sonnet-4-20250514"));
        let (cmd, args) = agent.build_args(None);
        assert_eq!(cmd, "claude");
        assert_eq!(
            args,
            vec![
                "--print",
                "--dangerously-skip-permissions",
                "--model",
                "claude-sonnet-4-20250514"
            ]
        );
    }

    #[test]
    fn test_claude_build_args_with_model_and_system_prompt() {
        let agent = make_agent(Some("claude-sonnet-4-20250514"));
        let (cmd, args) = agent.build_args(Some("Be concise."));
        assert_eq!(cmd, "claude");
        assert_eq!(
            args,
            vec![
                "--print",
                "--dangerously-skip-permissions",
                "--model",
                "claude-sonnet-4-20250514",
                "--system-prompt",
                "Be concise."
            ]
        );
    }

    #[test]
    fn test_claude_skip_permissions_default_true() {
        let agent = make_agent_with_permissions(None, None);
        let (_, args) = agent.build_args(None);
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn test_claude_skip_permissions_explicit_false() {
        let agent = make_agent_with_permissions(Some(false), None);
        let (_, args) = agent.build_args(None);
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
        assert_eq!(args, vec!["--print"]);
    }

    #[test]
    fn test_claude_allowed_tools() {
        let tools = vec![
            "Read".to_string(),
            "Grep".to_string(),
            "WebSearch".to_string(),
        ];
        let agent = make_agent_with_permissions(Some(false), Some(tools));
        let (_, args) = agent.build_args(None);
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
        assert_eq!(
            args,
            vec![
                "--print",
                "--allowedTools",
                "Read",
                "--allowedTools",
                "Grep",
                "--allowedTools",
                "WebSearch"
            ]
        );
    }

    #[test]
    fn test_claude_skip_permissions_true_ignores_allowed_tools() {
        let tools = vec!["Read".to_string()];
        let agent = make_agent_with_permissions(Some(true), Some(tools));
        let (_, args) = agent.build_args(None);
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(!args.contains(&"--allowedTools".to_string()));
    }
}
