pub mod channel;
pub mod command;
pub mod msmtp;
pub mod notification;

use crate::config::types::OutputSection;
use crate::error::Result;

/// Route output to the right handler based on config.
pub async fn dispatch(
    output: &OutputSection,
    result: &str,
    rendered_message: Option<&str>,
) -> Result<()> {
    let name = output.name.as_deref().unwrap_or("notification");

    match name {
        "notification" => notification::send(output, result, rendered_message).await,
        "msmtp" => msmtp::send(output, result, rendered_message).await,
        "command" => command::execute(output, result, rendered_message).await,
        "channel" => {
            // Daemon handles channel delivery, not us
            Ok(())
        }
        other => Err(crate::error::Error::Output(format!(
            "unknown output type: {}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::OutputSection;

    fn output_config(name: Option<&str>) -> OutputSection {
        OutputSection {
            name: name.map(|s| s.to_string()),
            channel: None,
            to: None,
            subject: None,
            message: None,
            command: None,
        }
    }

    #[tokio::test]
    async fn test_dispatch_channel_noop() {
        let result = dispatch(&output_config(Some("channel")), "test result", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_unknown_output() {
        let result = dispatch(&output_config(Some("bogus")), "test result", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("unknown output type"),
            "expected 'unknown output type' in: {err}"
        );
    }

    #[tokio::test]
    async fn test_dispatch_defaults_to_notification() {
        // notify-send won't exist in CI, but we just want to confirm it doesn't
        // hit the "unknown output type" branch
        let result = dispatch(&output_config(None), "test result", None).await;
        if let Err(e) = &result {
            let msg = e.to_string();
            assert!(
                !msg.contains("unknown output type"),
                "should not be unknown: {msg}"
            );
        }
    }
}
