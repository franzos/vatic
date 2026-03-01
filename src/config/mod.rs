pub mod dictionary;
pub mod secrets;
pub mod types;

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

use self::dictionary::Dictionary;
use self::secrets::Secrets;
use self::types::{parse_channel_config, parse_job_config, ChannelConfig, JobConfig};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub dictionary: Dictionary,
    pub secrets: Secrets,
    pub jobs: Vec<(String, JobConfig)>,
    pub channels: Vec<(String, ChannelConfig)>,
}

impl AppConfig {
    /// Resolve XDG paths, load dictionary, secrets, jobs, and channels.
    pub fn load() -> Result<Self> {
        let config_dir = resolve_config_dir()?;
        let data_dir = resolve_data_dir()?;

        let dict_path = config_dir.join("dictionary.toml");
        let dictionary = Dictionary::load(&dict_path)?;

        let secrets_path = config_dir.join("secrets.toml");
        let secrets = Secrets::load(&secrets_path)?;

        let jobs = load_jobs(&config_dir)?;
        let channels = load_channels(&config_dir)?;

        Ok(Self {
            config_dir,
            data_dir,
            dictionary,
            secrets,
            jobs,
            channels,
        })
    }

    /// Extension point for cross-field validation.
    /// Most checks happen at parse time via serde's tagged enum.
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

/// `$XDG_CONFIG_HOME/vatic` or `~/.config/vatic/`.
fn resolve_config_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("vatic"));
    }

    let home = home_dir()?;
    Ok(home.join(".config").join("vatic"))
}

/// `$XDG_DATA_HOME/vatic` or `~/.local/share/vatic/`.
fn resolve_data_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg).join("vatic"));
    }

    let home = home_dir()?;
    Ok(home.join(".local").join("share").join("vatic"))
}

fn home_dir() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| Error::Config("HOME environment variable not set".into()))
}

/// Walk a directory for `.toml` files, parse each with the given closure.
fn load_toml_dir<T, F>(dir: &Path, parse: F) -> Result<Vec<(String, T)>>
where
    F: Fn(&str, &Path) -> Result<(String, T)>,
{
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut items = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::Config(format!("cannot read directory {}: {e}", dir.display())))?;

    for entry in entries {
        let entry =
            entry.map_err(|e| Error::Config(format!("error reading directory entry: {e}")))?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| Error::Config(format!("cannot read {}: {e}", path.display())))?;

        let item = parse(&content, &path)?;
        items.push(item);
    }

    Ok(items)
}

/// Filename without extension — used as the lookup key.
fn filename_key(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Load `config_dir/channels/*.toml`, keyed by filename.
fn load_channels(config_dir: &Path) -> Result<Vec<(String, ChannelConfig)>> {
    load_toml_dir(&config_dir.join("channels"), |content, path| {
        let config = parse_channel_config(content)?;
        let key = filename_key(path);
        Ok((key, config))
    })
}

/// Load `config_dir/jobs/*.toml`, keyed by alias or filename.
fn load_jobs(config_dir: &Path) -> Result<Vec<(String, JobConfig)>> {
    load_toml_dir(&config_dir.join("jobs"), |content, path| {
        let table: toml::Table = toml::from_str(&content).map_err(|e| {
            Error::Config(format!("invalid TOML in {}: {e}", path.display()))
        })?;
        let value = toml::Value::Table(table);
        let config = parse_job_config(&value)?;
        let key = config.alias.clone().unwrap_or_else(|| filename_key(path));
        Ok((key, config))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // -- filename_key --

    #[test]
    fn test_filename_key_normal() {
        assert_eq!(
            filename_key(Path::new("/home/user/weather.toml")),
            "weather"
        );
    }

    #[test]
    fn test_filename_key_no_extension() {
        assert_eq!(filename_key(Path::new("/home/user/weather")), "weather");
    }

    #[test]
    fn test_filename_key_multiple_dots() {
        assert_eq!(filename_key(Path::new("my.job.toml")), "my.job");
    }

    #[test]
    fn test_filename_key_hidden_file() {
        assert_eq!(filename_key(Path::new(".hidden.toml")), ".hidden");
    }

    // -- load_toml_dir --

    #[test]
    fn test_load_toml_dir_nonexistent() {
        let result = load_toml_dir(Path::new("/nonexistent/path"), |_, _| {
            Ok(("key".to_string(), "value".to_string()))
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_load_toml_dir_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result: Result<Vec<(String, String)>> = load_toml_dir(dir.path(), |_, _| {
            Ok(("key".to_string(), "value".to_string()))
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_load_toml_dir_skips_non_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.md"), "# Hello").unwrap();
        std::fs::write(dir.path().join("data.json"), "{}").unwrap();
        let result: Result<Vec<(String, String)>> = load_toml_dir(dir.path(), |_, _| {
            Ok(("key".to_string(), "value".to_string()))
        });
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_load_toml_dir_loads_toml_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.toml"), "content = true").unwrap();
        std::fs::write(dir.path().join("two.toml"), "content = true").unwrap();
        std::fs::write(dir.path().join("skip.md"), "not toml").unwrap();
        let result: Result<Vec<(String, String)>> = load_toml_dir(dir.path(), |content, path| {
            let key = filename_key(path);
            Ok((key, content.to_string()))
        });
        let items = result.unwrap();
        assert_eq!(items.len(), 2);
        let keys: Vec<&str> = items.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"one"));
        assert!(keys.contains(&"two"));
    }

    #[test]
    fn test_load_toml_dir_parse_error_propagates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.toml"), "content").unwrap();
        let result: Result<Vec<(String, String)>> =
            load_toml_dir(dir.path(), |_, _| Err(Error::Config("parse failed".into())));
        assert!(result.is_err());
    }
}
