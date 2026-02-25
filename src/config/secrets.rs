use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::error::{Error, Result};

#[derive(Clone)]
pub struct Secret {
    pub key: String,
    pub header: String,
    pub match_url: String,
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("key", &"***")
            .field("header", &self.header)
            .field("match_url", &self.match_url)
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct Secrets {
    pub entries: HashMap<String, Secret>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets")
            .field("entries", &format!("[{} entries]", self.entries.len()))
            .finish()
    }
}

impl Secrets {
    /// Secrets shouldn't be world-readable — warn if they are.
    fn check_permissions(path: &Path) {
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                tracing::warn!(
                    "{} has mode {:04o} — should be 0600. Fix with: chmod 600 {}",
                    path.display(),
                    mode,
                    path.display()
                );
            }
        }
    }

    /// Returns empty if the file doesn't exist.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        Self::check_permissions(path);

        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("failed to read secrets: {e}")))?;
        let table: toml::Table = toml::from_str(&content)
            .map_err(|e| Error::Config(format!("failed to parse secrets: {e}")))?;

        let mut entries = HashMap::new();
        for (name, value) in table {
            if let toml::Value::Table(t) = value {
                let key = t
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let header = t
                    .get("header")
                    .and_then(|v| v.as_str())
                    .unwrap_or("bearer")
                    .to_string();
                let match_url = t
                    .get("match")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                entries.insert(
                    name,
                    Secret {
                        key,
                        header,
                        match_url,
                    },
                );
            }
        }

        Ok(Self { entries })
    }

    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.entries.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_secrets() {
        let dir = std::env::temp_dir().join("vatic_test_secrets");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[formshive]
key = "abc123"
header = "bearer"
match = "https://api.formshive.com"

[github]
key = "ghp_token"
header = "basic"
match = "https://api.github.com"
"#
        )
        .unwrap();

        let secrets = Secrets::load(&path).unwrap();
        assert_eq!(secrets.entries.len(), 2);

        let fh = secrets.get("formshive").unwrap();
        assert_eq!(fh.key, "abc123");
        assert_eq!(fh.header, "bearer");
        assert_eq!(fh.match_url, "https://api.formshive.com");

        let gh = secrets.get("github").unwrap();
        assert_eq!(gh.key, "ghp_token");
        assert_eq!(gh.header, "basic");
        assert_eq!(gh.match_url, "https://api.github.com");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_missing_file() {
        let path = std::path::PathBuf::from("/tmp/vatic_no_such_secrets.toml");
        let secrets = Secrets::load(&path).unwrap();
        assert!(secrets.entries.is_empty());
    }

    #[test]
    fn test_get_secret() {
        let mut secrets = Secrets::default();
        secrets.entries.insert(
            "test".into(),
            Secret {
                key: "k".into(),
                header: "bearer".into(),
                match_url: "https://example.com".into(),
            },
        );
        assert!(secrets.get("test").is_some());
        assert!(secrets.get("missing").is_none());
    }

    #[test]
    fn test_secret_with_defaults() {
        let dir = std::env::temp_dir().join("vatic_test_secrets_defaults");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[test]\nkey = \"abc\"").unwrap();

        let secrets = Secrets::load(&path).unwrap();
        let s = secrets.get("test").unwrap();
        assert_eq!(s.key, "abc");
        assert_eq!(s.header, "bearer");
        assert_eq!(s.match_url, "");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_non_table_entry_skipped() {
        let dir = std::env::temp_dir().join("vatic_test_secrets_nontable");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "plainvalue = \"string\"").unwrap();

        let secrets = Secrets::load(&path).unwrap();
        assert!(secrets.entries.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_empty_secrets_file() {
        let dir = std::env::temp_dir().join("vatic_test_secrets_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.toml");

        std::fs::File::create(&path).unwrap();

        let secrets = Secrets::load(&path).unwrap();
        assert_eq!(secrets.entries.len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_malformed_toml() {
        let dir = std::env::temp_dir().join("vatic_test_secrets_malformed");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secrets.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[broken\nkey = nope").unwrap();

        let err = Secrets::load(&path).unwrap_err();
        assert!(err.to_string().contains("failed to parse secrets"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_secret_debug_hides_key() {
        let secret = Secret {
            key: "super_secret_value_12345".into(),
            header: "bearer".into(),
            match_url: "https://example.com".into(),
        };
        let debug_output = format!("{:?}", secret);
        assert!(debug_output.contains("***"));
        assert!(!debug_output.contains("super_secret_value_12345"));
    }
}
