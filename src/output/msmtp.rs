use crate::config::types::OutputSection;
use crate::error::{Error, Result};

/// Send an email via msmtp — pipes RFC 2822 formatted message to stdin.
pub async fn send(
    output: &OutputSection,
    result: &str,
    rendered_message: Option<&str>,
) -> Result<()> {
    let to = output
        .to
        .as_deref()
        .ok_or_else(|| Error::Output("msmtp output requires a 'to' field".to_string()))?;

    let subject = output.subject.as_deref().unwrap_or("vatic notification");

    let body = rendered_message.unwrap_or(result);
    let email = build_email(to, subject, body);
    let (program, args) = build_command(to);

    let mut child = tokio::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Output(format!("failed to run msmtp: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(email.as_bytes())
            .await
            .map_err(|e| Error::Output(format!("failed to write to msmtp stdin: {e}")))?;
    }

    let status = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        child.wait(),
    )
    .await
    .map_err(|_| Error::Output("msmtp timed out after 60 seconds".to_string()))?
    .map_err(|e| Error::Output(format!("failed to wait for msmtp: {e}")))?;

    if !status.success() {
        return Err(Error::Output(format!(
            "msmtp exited with status: {}",
            status
        )));
    }

    Ok(())
}

/// Strip CR/LF — prevents header injection in email headers.
fn sanitize_header(value: &str) -> String {
    value.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

/// Assemble a minimal RFC 2822 email string.
pub fn build_email(to: &str, subject: &str, body: &str) -> String {
    let to = sanitize_header(to);
    let subject = sanitize_header(subject);
    format!("To: {}\nSubject: {}\n\n{}", to, subject, body)
}

/// Build the msmtp invocation.
pub fn build_command(to: &str) -> (String, Vec<String>) {
    ("msmtp".to_string(), vec![to.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::OutputSection;

    #[test]
    fn test_build_email() {
        let email = build_email("user@example.com", "Test Subject", "Hello there");
        assert_eq!(
            email,
            "To: user@example.com\nSubject: Test Subject\n\nHello there"
        );
    }

    #[test]
    fn test_build_email_strips_header_injection() {
        let email = build_email(
            "user@example.com\r\nBCC: evil@attacker.com",
            "Hi\r\nBCC: evil@attacker.com",
            "body",
        );
        assert_eq!(
            email,
            "To: user@example.comBCC: evil@attacker.com\nSubject: HiBCC: evil@attacker.com\n\nbody"
        );
        // Injected newlines get stripped, so the BCC ends up mashed into the value
        assert!(!email.contains("\r\nBCC:"));
    }

    #[test]
    fn test_build_command() {
        let (cmd, args) = build_command("user@example.com");
        assert_eq!(cmd, "msmtp");
        assert_eq!(args, vec!["user@example.com"]);
    }

    #[tokio::test]
    async fn test_missing_to() {
        let output = OutputSection {
            name: Some(crate::config::types::OutputName::Msmtp),
            channel: None,
            to: None,
            subject: None,
            message: None,
            command: None,
        };
        let result = send(&output, "test", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("requires a 'to' field"), "got: {err}");
    }
}
