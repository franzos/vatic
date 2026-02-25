use crate::error::{Error, Result};

/// Apply a pipe transformation. Currently just `summary` as a placeholder —
/// TODO: wire this up to an actual agent call.
pub async fn apply_pipe(pipe: &str, input: &str) -> Result<String> {
    match pipe {
        "summary" => Ok(format!("Summary of: {}", input)),
        _ => Err(Error::Template(format!("unknown pipe: '{pipe}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pipe_summary() {
        let result = apply_pipe("summary", "some long text").await.unwrap();
        assert_eq!(result, "Summary of: some long text");
    }

    #[tokio::test]
    async fn test_pipe_unknown() {
        let result = apply_pipe("nonexistent", "input").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown pipe"));
    }

    #[tokio::test]
    async fn test_pipe_summary_empty_input() {
        let result = apply_pipe("summary", "").await.unwrap();
        assert_eq!(result, "Summary of: ");
    }

    #[tokio::test]
    async fn test_pipe_name_is_case_sensitive() {
        let err = apply_pipe("Summary", "text").await.unwrap_err();
        assert!(err.to_string().contains("unknown pipe"));
    }

    #[tokio::test]
    async fn test_pipe_with_whitespace_in_name() {
        let err = apply_pipe(" summary", "text").await.unwrap_err();
        assert!(err.to_string().contains("unknown pipe"));
    }
}
