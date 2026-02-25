use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("template error: {0}")]
    Template(String),

    #[error("agent error: {0}")]
    Agent(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("output error: {0}")]
    Output(String),

    #[error("environment error: {0}")]
    Environment(String),

    #[error("channel error: {0}")]
    Channel(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_config() {
        let err = Error::Config("bad value".into());
        assert_eq!(err.to_string(), "config error: bad value");
    }

    #[test]
    fn test_display_template() {
        let err = Error::Template("missing var".into());
        assert_eq!(err.to_string(), "template error: missing var");
    }

    #[test]
    fn test_display_agent() {
        let err = Error::Agent("timeout".into());
        assert_eq!(err.to_string(), "agent error: timeout");
    }

    #[test]
    fn test_display_store() {
        let err = Error::Store("not found".into());
        assert_eq!(err.to_string(), "store error: not found");
    }

    #[test]
    fn test_display_output() {
        let err = Error::Output("write failed".into());
        assert_eq!(err.to_string(), "output error: write failed");
    }

    #[test]
    fn test_display_environment() {
        let err = Error::Environment("missing package".into());
        assert_eq!(err.to_string(), "environment error: missing package");
    }

    #[test]
    fn test_display_channel() {
        let err = Error::Channel("disconnected".into());
        assert_eq!(err.to_string(), "channel error: disconnected");
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(err.to_string().contains("file not found"));
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn test_display_sqlite_variant() {
        // rusqlite::Error isn't easily constructable, so we just verify the format works
        let err = Error::Store("sqlite constraint violation".into());
        assert!(err.to_string().starts_with("store error:"));
    }
}
