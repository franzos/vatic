use crate::config::types::OutputSection;
use crate::error::{Error, Result};

/// Fire a desktop notification via notify-send.
pub async fn send(
    _output: &OutputSection,
    result: &str,
    rendered_message: Option<&str>,
) -> Result<()> {
    let message = rendered_message.unwrap_or(result);
    let (program, args) = build_command(message);

    let status = tokio::process::Command::new(&program)
        .args(&args)
        .status()
        .await
        .map_err(|e| Error::Output(format!("failed to run notify-send: {e}")))?;

    if !status.success() {
        return Err(Error::Output(format!(
            "notify-send exited with status: {}",
            status
        )));
    }

    Ok(())
}

/// Build the notify-send invocation.
pub fn build_command(message: &str) -> (String, Vec<String>) {
    (
        "notify-send".to_string(),
        vec!["vatic".to_string(), message.to_string()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_notification_command() {
        let (cmd, args) = build_command("Hello world");
        assert_eq!(cmd, "notify-send");
        assert_eq!(args, vec!["vatic", "Hello world"]);
    }

    #[test]
    fn test_build_notification_escaping() {
        let (cmd, args) = build_command("He said \"hello\"\nNew line");
        assert_eq!(cmd, "notify-send");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "vatic");
        assert_eq!(args[1], "He said \"hello\"\nNew line");
    }
}
