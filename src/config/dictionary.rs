use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Dictionary {
    pub entries: HashMap<String, HashMap<String, String>>,
}

impl Dictionary {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Returns an empty dictionary if the file doesn't exist.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let content = std::fs::read_to_string(path)?;
        let value: toml::Value = content
            .parse()
            .map_err(|e: toml::de::Error| Error::Config(format!("invalid dictionary TOML: {e}")))?;

        let table = value
            .as_table()
            .ok_or_else(|| Error::Config("dictionary TOML must be a table".into()))?;

        let mut entries = HashMap::new();
        for (section, val) in table {
            let section_table = val.as_table().ok_or_else(|| {
                Error::Config(format!("dictionary section '{section}' must be a table"))
            })?;

            let mut section_map = HashMap::new();
            for (key, v) in section_table {
                let s = v.as_str().ok_or_else(|| {
                    Error::Config(format!(
                        "dictionary value '{section}.{key}' must be a string"
                    ))
                })?;
                section_map.insert(key.clone(), s.to_string());
            }
            entries.insert(section.clone(), section_map);
        }

        Ok(Self { entries })
    }

    /// e.g. `dictionary.get("general", "name")`
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.entries
            .get(section)
            .and_then(|s| s.get(key))
            .map(|s| s.as_str())
    }
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_dictionary_lookup() {
        let dir = std::env::temp_dir().join("vatic_test_dict");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[general]
name = "Franz"
location = "Lisbon"
"#
        )
        .unwrap();

        let dict = Dictionary::load(&path).unwrap();
        assert_eq!(dict.get("general", "name"), Some("Franz"));
        assert_eq!(dict.get("general", "location"), Some("Lisbon"));
        assert_eq!(dict.get("general", "missing"), None);
        assert_eq!(dict.get("unknown", "name"), None);

        // cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dictionary_missing_file() {
        let path = std::path::PathBuf::from("/tmp/vatic_no_such_file_dict.toml");
        let dict = Dictionary::load(&path).unwrap();
        assert!(dict.entries.is_empty());
    }

    #[test]
    fn test_non_string_value() {
        let dir = std::env::temp_dir().join("vatic_test_dict_non_string");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[general]\nname = 123").unwrap();

        let err = Dictionary::load(&path).unwrap_err();
        assert!(err.to_string().contains("must be a string"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_non_table_section() {
        let dir = std::env::temp_dir().join("vatic_test_dict_non_table");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "general = \"not a table\"").unwrap();

        let err = Dictionary::load(&path).unwrap_err();
        assert!(err.to_string().contains("must be a table"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_empty_section() {
        let dir = std::env::temp_dir().join("vatic_test_dict_empty_section");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[general]").unwrap();

        let dict = Dictionary::load(&path).unwrap();
        assert!(dict.entries.contains_key("general"));
        assert!(dict.entries["general"].is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_multiple_sections() {
        let dir = std::env::temp_dir().join("vatic_test_dict_multi");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "[general]\nname = \"Franz\"\n\n[preferences]\ntheme = \"dark\""
        )
        .unwrap();

        let dict = Dictionary::load(&path).unwrap();
        assert_eq!(dict.get("general", "name"), Some("Franz"));
        assert_eq!(dict.get("preferences", "theme"), Some("dark"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_malformed_toml() {
        let dir = std::env::temp_dir().join("vatic_test_dict_malformed");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.toml");

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[general\nname = broken").unwrap();

        let err = Dictionary::load(&path).unwrap_err();
        assert!(err.to_string().contains("invalid dictionary TOML"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
