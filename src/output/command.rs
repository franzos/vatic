use crate::config::types::OutputSection;
use crate::error::{Error, Result};

/// Run a shell command with the result passed as `$VATIC_RESULT` env var.
/// We swap `{% result %}` for `$VATIC_RESULT` so the shell treats it as data,
/// not code — prevents command injection.
pub async fn execute(
    output: &OutputSection,
    result: &str,
    _rendered_message: Option<&str>,
) -> Result<()> {
    let command_template = output
        .command
        .as_deref()
        .ok_or_else(|| Error::Output("command output requires a 'command' field".to_string()))?;

    let command = prepare_command(command_template);

    let output = tokio::process::Command::new("sh")
        .args(["-c", &command])
        .env("VATIC_RESULT", result)
        .output()
        .await
        .map_err(|e| Error::Output(format!("failed to run command: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Output(format!(
            "command exited with status {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    Ok(())
}

/// Swap template placeholder for shell env var reference.
pub fn prepare_command(command: &str) -> String {
    command.replace("{% result %}", "$VATIC_RESULT")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::OutputSection;

    #[test]
    fn test_prepare_command() {
        let cmd = prepare_command("echo '{% result %}'");
        assert_eq!(cmd, "echo '$VATIC_RESULT'");
    }

    #[test]
    fn test_prepare_command_no_placeholder() {
        let cmd = prepare_command("echo hello");
        assert_eq!(cmd, "echo hello");
    }

    #[tokio::test]
    async fn test_result_passed_as_env_var() {
        let output = OutputSection {
            name: Some("command".to_string()),
            channel: None,
            to: None,
            subject: None,
            message: None,
            command: Some("echo $VATIC_RESULT".to_string()),
        };
        let result = execute(&output, "safe; echo injected", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_missing_command() {
        let output = OutputSection {
            name: Some("command".to_string()),
            channel: None,
            to: None,
            subject: None,
            message: None,
            command: None,
        };
        let result = execute(&output, "test", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("requires a 'command' field"), "got: {err}");
    }
}
