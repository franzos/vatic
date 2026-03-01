pub mod channel;
pub mod command;
pub mod msmtp;
pub mod notification;

use crate::config::types::OutputSection;
use crate::error::Result;

use crate::config::types::OutputName;

/// Route output to the right handler based on config.
pub async fn dispatch(
    output: &OutputSection,
    result: &str,
    rendered_message: Option<&str>,
) -> Result<()> {
    let name = output.name.unwrap_or_default();

    match name {
        OutputName::Notification => notification::send(output, result, rendered_message).await,
        OutputName::Msmtp => msmtp::send(output, result, rendered_message).await,
        OutputName::Command => command::execute(output, result, rendered_message).await,
        OutputName::Channel => {
            // Daemon handles channel delivery, not us
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::OutputSection;

    fn output_config(name: Option<OutputName>) -> OutputSection {
        OutputSection {
            name,
            channel: None,
            to: None,
            subject: None,
            message: None,
            command: None,
        }
    }

    #[tokio::test]
    async fn test_dispatch_channel_noop() {
        let result = dispatch(&output_config(Some(OutputName::Channel)), "test result", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_defaults_to_notification() {
        // notify-send won't exist in CI, but we just want to confirm
        // the default path works without panicking
        let _ = dispatch(&output_config(None), "test result", None).await;
    }
}
