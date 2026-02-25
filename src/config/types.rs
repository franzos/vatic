use serde::Deserialize;

use crate::error::{Error, Result};

/// Loaded from `~/.config/vatic/channels/*.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelConfig {
    pub channel: ChannelSection,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ChannelSection {
    #[serde(rename = "stdin")]
    Stdin,
    #[serde(rename = "telegram")]
    Telegram { token: String },
    #[serde(rename = "matrix")]
    Matrix {
        homeserver: String,
        user: String,
        password: String,
    },
    #[serde(rename = "himalaya")]
    Himalaya {
        poll_interval: Option<u64>,
        account: Option<String>,
    },
    #[serde(rename = "whatsapp")]
    Whatsapp,
}

impl std::fmt::Debug for ChannelSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelSection::Stdin => f.debug_struct("Stdin").finish(),
            ChannelSection::Telegram { .. } => {
                f.debug_struct("Telegram").field("token", &"***").finish()
            }
            ChannelSection::Matrix {
                homeserver, user, ..
            } => f
                .debug_struct("Matrix")
                .field("homeserver", homeserver)
                .field("user", user)
                .field("password", &"***")
                .finish(),
            ChannelSection::Himalaya {
                poll_interval,
                account,
            } => f
                .debug_struct("Himalaya")
                .field("poll_interval", poll_interval)
                .field("account", account)
                .finish(),
            ChannelSection::Whatsapp => f.debug_struct("Whatsapp").finish(),
        }
    }
}

pub fn parse_channel_config(toml_str: &str) -> Result<ChannelConfig> {
    toml::from_str(toml_str)
        .map_err(|e| Error::Config(format!("failed to parse channel config: {e}")))
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentSection {
    pub name: String,
    pub prompt: Option<String>,
    pub host: Option<String>,
    pub model: Option<String>,
    /// Defaults to true. Set false + `allowed_tools` for granular control.
    pub skip_permissions: Option<bool>,
    /// Only used when `skip_permissions` is false.
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JobSection {
    pub interval: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvironmentSection {
    pub name: String,
    pub pwd: Option<String>,
    pub packages: Option<Vec<String>>,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputSection {
    pub name: Option<String>,
    pub channel: Option<String>,
    pub to: Option<String>,
    pub subject: Option<String>,
    pub message: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputSection {
    pub channel: String,
    pub trigger: Option<String>,
    /// "anywhere" (default), "start", or "end".
    pub trigger_match: Option<String>,
    /// If unset, all senders are allowed.
    pub allowed_senders: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionSection {
    pub context: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HistorySection {
    pub prompt: String,
}

/// Intermediate serde target — `name` and `alias` are bare TOML keys, not sections.
#[derive(Debug, Clone, Deserialize)]
struct RawJobConfig {
    pub name: Option<String>,
    pub alias: Option<String>,
    pub agent: AgentSection,
    pub job: Option<JobSection>,
    pub environment: Option<EnvironmentSection>,
    pub output: Option<OutputSection>,
    pub input: Option<InputSection>,
    pub session: Option<SessionSection>,
    pub history: Option<HistorySection>,
}

#[derive(Debug, Clone)]
pub struct JobConfig {
    pub name: Option<String>,
    pub alias: Option<String>,
    pub agent: AgentSection,
    pub job: Option<JobSection>,
    pub environment: Option<EnvironmentSection>,
    pub outputs: Vec<OutputSection>,
    pub input: Option<InputSection>,
    pub session: Option<SessionSection>,
    pub history: Option<HistorySection>,
}

/// Handles `[output]` plus `[output:1]`, `[output:2]`, etc. — all
/// collected into `JobConfig.outputs`, sorted by number.
pub fn parse_job_config(value: &toml::Value) -> Result<JobConfig> {
    let table = value
        .as_table()
        .ok_or_else(|| Error::Config("expected TOML table at top level".to_string()))?;

    // Pull out numbered outputs before serde gets confused by them
    let mut numbered_outputs: Vec<(u32, OutputSection)> = Vec::new();
    for key in table.keys() {
        if let Some(suffix) = key.strip_prefix("output:") {
            let num: u32 = suffix
                .parse()
                .map_err(|_| Error::Config(format!("invalid output number in key '{key}'")))?;
            let section: OutputSection =
                table[key]
                    .clone()
                    .try_into()
                    .map_err(|e: toml::de::Error| {
                        Error::Config(format!("failed to parse {key}: {e}"))
                    })?;
            numbered_outputs.push((num, section));
        }
    }
    numbered_outputs.sort_by_key(|(n, _)| *n);

    // Strip the numbered keys so serde doesn't choke on them
    let mut cleaned = table.clone();
    cleaned.retain(|key, _| !key.starts_with("output:"));

    let raw: RawJobConfig = toml::Value::Table(cleaned)
        .try_into()
        .map_err(|e: toml::de::Error| Error::Config(format!("failed to parse job config: {e}")))?;

    // Base [output] goes first, then numbered ones in order
    let mut outputs: Vec<OutputSection> = Vec::new();
    if let Some(base) = raw.output {
        outputs.push(base);
    }
    for (_, section) in numbered_outputs {
        outputs.push(section);
    }

    Ok(JobConfig {
        name: raw.name,
        alias: raw.alias,
        agent: raw.agent,
        job: raw.job,
        environment: raw.environment,
        outputs,
        input: raw.input,
        session: raw.session,
        history: raw.history,
    })
}

/// Convenience wrapper for tests and one-off parsing.
pub fn parse_job_config_str(toml_str: &str) -> Result<JobConfig> {
    let value: toml::Value = toml_str
        .parse()
        .map_err(|e: toml::de::Error| Error::Config(format!("invalid TOML: {e}")))?;
    parse_job_config(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_job_config() {
        let toml_str = r#"
name = "Today's weather"
alias = "weather"

[agent]
name = "claude"
prompt = "You are a weather reporter."

[job]
interval = "0 8 * * *"
prompt = "What's the weather for {% date %} in Lisbon?"

[environment]
name = "guix-shell"

[output]
name = "notification"
message = "Good morning {% custom:name %}; {% result %}"
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert_eq!(config.name.as_deref(), Some("Today's weather"));
        assert_eq!(config.alias.as_deref(), Some("weather"));
        assert_eq!(config.agent.name, "claude");
        assert_eq!(
            config.agent.prompt.as_deref(),
            Some("You are a weather reporter.")
        );
        assert!(config.job.is_some());
        let job = config.job.as_ref().unwrap();
        assert_eq!(job.interval.as_deref(), Some("0 8 * * *"));
        assert!(config.environment.is_some());
        assert_eq!(config.outputs.len(), 1);
        assert_eq!(config.outputs[0].name.as_deref(), Some("notification"));
    }

    #[test]
    fn test_parse_minimal_job() {
        let toml_str = r#"
[agent]
name = "claude"
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert!(config.name.is_none());
        assert!(config.alias.is_none());
        assert_eq!(config.agent.name, "claude");
        assert!(config.job.is_none());
        assert!(config.environment.is_none());
        assert!(config.outputs.is_empty());
        assert!(config.input.is_none());
        assert!(config.session.is_none());
        assert!(config.history.is_none());
    }

    #[test]
    fn test_parse_single_output() {
        let toml_str = r#"
[agent]
name = "claude"

[output]
name = "notification"
message = "Hello {% result %}"
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert_eq!(config.outputs.len(), 1);
        assert_eq!(config.outputs[0].name.as_deref(), Some("notification"));
        assert_eq!(
            config.outputs[0].message.as_deref(),
            Some("Hello {% result %}")
        );
    }

    #[test]
    fn test_parse_multiple_outputs() {
        let toml_str = r#"
[agent]
name = "claude"

[output]
name = "notification"
message = "Desktop: {% result %}"

["output:1"]
name = "msmtp"
to = "user@example.com"
subject = "Alert"
message = "Email: {% result %}"
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert_eq!(config.outputs.len(), 2);
        assert_eq!(config.outputs[0].name.as_deref(), Some("notification"));
        assert_eq!(config.outputs[1].name.as_deref(), Some("msmtp"));
        assert_eq!(config.outputs[1].to.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn test_parse_no_output() {
        let toml_str = r#"
[agent]
name = "claude"

[job]
prompt = "Hello"
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert!(config.outputs.is_empty());
    }

    #[test]
    fn test_parse_history_section() {
        let toml_str = r#"
[agent]
name = "claude"

[job]
prompt = "What happened today?"

[history]
prompt = "Summarize the following into a single paragraph."
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert!(config.history.is_some());
        let history = config.history.unwrap();
        assert_eq!(
            history.prompt,
            "Summarize the following into a single paragraph."
        );
    }

    #[test]
    fn test_parse_no_history() {
        let toml_str = r#"
[agent]
name = "claude"

[job]
prompt = "Hello"
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert!(config.history.is_none());
    }

    #[test]
    fn test_parse_channel_config_stdin() {
        let toml_str = r#"
[channel]
type = "stdin"
"#;
        let config = parse_channel_config(toml_str).unwrap();
        assert!(matches!(config.channel, ChannelSection::Stdin));
    }

    #[test]
    fn test_parse_channel_config_whatsapp() {
        let toml_str = r#"
[channel]
type = "whatsapp"
"#;
        let config = parse_channel_config(toml_str).unwrap();
        assert!(matches!(config.channel, ChannelSection::Whatsapp));
    }

    #[test]
    fn test_parse_channel_config_himalaya() {
        let toml_str = r#"
[channel]
type = "himalaya"
poll_interval = 60
account = "personal"
"#;
        let config = parse_channel_config(toml_str).unwrap();
        match &config.channel {
            ChannelSection::Himalaya {
                poll_interval,
                account,
            } => {
                assert_eq!(*poll_interval, Some(60));
                assert_eq!(account.as_deref(), Some("personal"));
            }
            other => panic!("expected Himalaya, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_channel_config_telegram() {
        let toml_str = r#"
[channel]
type = "telegram"
token = "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"
"#;
        let config = parse_channel_config(toml_str).unwrap();
        match &config.channel {
            ChannelSection::Telegram { token } => {
                assert_eq!(token, "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11");
            }
            other => panic!("expected Telegram, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_channel_config_matrix() {
        let toml_str = r#"
[channel]
type = "matrix"
homeserver = "https://matrix.org"
user = "@vatic:matrix.org"
password = "secret"
"#;
        let config = parse_channel_config(toml_str).unwrap();
        match &config.channel {
            ChannelSection::Matrix {
                homeserver,
                user,
                password,
            } => {
                assert_eq!(homeserver, "https://matrix.org");
                assert_eq!(user, "@vatic:matrix.org");
                assert_eq!(password, "secret");
            }
            other => panic!("expected Matrix, got {:?}", other),
        }
    }

    #[test]
    fn test_missing_agent_section() {
        let toml_str = r#"
[job]
prompt = "hello"
"#;
        let err = parse_job_config_str(toml_str).unwrap_err();
        assert!(err.to_string().contains("agent"));
    }

    #[test]
    fn test_unknown_channel_type() {
        let toml_str = r#"
[channel]
type = "irc"
"#;
        let err = parse_channel_config(toml_str).unwrap_err();
        assert!(err.to_string().contains("failed to parse channel config"));
    }

    #[test]
    fn test_missing_telegram_token() {
        let toml_str = r#"
[channel]
type = "telegram"
"#;
        let err = parse_channel_config(toml_str).unwrap_err();
        assert!(err.to_string().contains("failed to parse channel config"));
    }

    #[test]
    fn test_missing_matrix_fields() {
        let toml_str = r#"
[channel]
type = "matrix"
"#;
        let err = parse_channel_config(toml_str).unwrap_err();
        assert!(err.to_string().contains("failed to parse channel config"));
    }

    #[test]
    fn test_himalaya_no_optional_fields() {
        let toml_str = r#"
[channel]
type = "himalaya"
"#;
        let config = parse_channel_config(toml_str).unwrap();
        match &config.channel {
            ChannelSection::Himalaya {
                poll_interval,
                account,
            } => {
                assert!(poll_interval.is_none());
                assert!(account.is_none());
            }
            other => panic!("expected Himalaya, got {:?}", other),
        }
    }

    #[test]
    fn test_job_with_input_section() {
        let toml_str = r#"
[agent]
name = "claude"

[input]
channel = "telegram"
trigger = "weather"
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert!(config.input.is_some());
        let input = config.input.unwrap();
        assert_eq!(input.channel, "telegram");
        assert_eq!(input.trigger.as_deref(), Some("weather"));
    }

    #[test]
    fn test_job_with_session_section() {
        let toml_str = r#"
[agent]
name = "claude"

[session]
context = 5
"#;
        let config = parse_job_config_str(toml_str).unwrap();
        assert!(config.session.is_some());
        let session = config.session.unwrap();
        assert_eq!(session.context, 5);
    }
}
