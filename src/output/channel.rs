use crate::config::types::OutputSection;
use crate::error::Result;

/// Channel output stub — the daemon handles actual delivery.
/// No-op when called outside daemon context.
pub async fn send(
    _output: &OutputSection,
    _result: &str,
    _rendered_message: Option<&str>,
) -> Result<()> {
    Ok(())
}
