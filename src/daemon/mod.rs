pub mod scheduler;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Local;

use crate::agent;
use crate::channel::email::EmailChannel;
use crate::channel::matrix::MatrixChannel;
use crate::channel::stdin::StdinChannel;
use crate::channel::telegram::TelegramChannel;
#[cfg(feature = "whatsapp")]
use crate::channel::whatsapp::WhatsAppChannel;
use crate::channel::{Channel, IncomingMessage};
use crate::config::types::{ChannelSection, JobConfig, TriggerMatch};
use crate::config::AppConfig;
use crate::env;
use crate::error::Result;
use crate::output;
use crate::store::{SessionMessage, Store};
use crate::template;
use crate::template::functions::RenderContext;
use tokio::sync::mpsc;

use self::scheduler::CronSchedule;

/// Main loop — listens on channels, runs cron schedules, dispatches jobs.
pub async fn run_daemon(app: &AppConfig) -> Result<()> {
    app.validate()?;

    let db_path = app.data_dir.join("vatic.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::error::Error::Store(format!("cannot create data directory: {e}"))
        })?;
    }
    let store = Store::open(&db_path)?;
    if let Err(e) = store.prune(1000, 30) {
        tracing::warn!("database pruning failed: {e}");
    }

    tracing::info!("config: {}", app.config_dir.display());
    tracing::info!("data:   {}", app.data_dir.display());
    for (alias, job) in &app.jobs {
        let env_name = job
            .environment
            .as_ref()
            .map(|e| e.name)
            .unwrap_or(crate::config::types::EnvironmentName::Local);
        let via = job
            .input
            .as_ref()
            .map_or("manual".to_string(), |i| match &i.trigger {
                Some(t) => format!("{} (trigger: {})", i.channel, t),
                None => i.channel.clone(),
            });
        tracing::info!("[{}] in {} via {}", alias, env_name, via);
    }

    let (tx, mut rx) = mpsc::channel::<IncomingMessage>(100);

    // Fall back to stdin if no channels are configured
    let mut channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();

    if app.channels.is_empty() {
        tracing::info!("no channel configs found, defaulting to stdin");
        let ch: Arc<dyn Channel> = Arc::new(StdinChannel);
        channels.insert("stdin".to_string(), ch);
    } else {
        for (name, channel_config) in &app.channels {
            let ch: Arc<dyn Channel> = match &channel_config.channel {
                ChannelSection::Stdin => Arc::new(StdinChannel),
                #[cfg(feature = "whatsapp")]
                ChannelSection::Whatsapp => {
                    let data_dir = app.data_dir.join("channels").join("whatsapp");
                    Arc::new(WhatsAppChannel::new(data_dir))
                }
                ChannelSection::Telegram { token } => Arc::new(TelegramChannel::new(token.clone())),
                ChannelSection::Matrix {
                    homeserver,
                    user,
                    password,
                } => {
                    let data_dir = app.data_dir.join("channels").join("matrix");
                    Arc::new(MatrixChannel::new(
                        homeserver.clone(),
                        user.clone(),
                        password.clone(),
                        data_dir,
                    ))
                }
                ChannelSection::Himalaya {
                    poll_interval,
                    account,
                } => Arc::new(EmailChannel::new(
                    poll_interval.unwrap_or(60),
                    account.clone(),
                )),
                #[cfg(not(feature = "whatsapp"))]
                ChannelSection::Whatsapp => {
                    tracing::warn!(
                        "whatsapp channel requires the 'whatsapp' feature flag, skipping '{}'",
                        name
                    );
                    continue;
                }
            };
            channels.insert(ch.name().to_string(), ch);
        }
    }

    let channel_names: Vec<&str> = channels.keys().map(|s| s.as_str()).collect();
    tracing::info!("channels: [{}]", channel_names.join(", "));

    for (name, channel) in &channels {
        let ch = Arc::clone(channel);
        let channel_tx = tx.clone();
        let channel_name = name.clone();
        tokio::spawn(async move {
            if let Err(e) = ch.start(channel_tx).await {
                tracing::error!("{} channel error: {}", channel_name, e);
            }
        });
    }

    let mut schedules: Vec<(String, CronSchedule)> = Vec::new();
    for (alias, job) in &app.jobs {
        if let Some(interval) = job.job.as_ref().and_then(|j| j.interval.as_deref()) {
            match CronSchedule::parse(interval) {
                Ok(schedule) => {
                    tracing::info!("[{}] scheduled: {}", alias, interval);
                    schedules.push((alias.clone(), schedule));
                }
                Err(e) => {
                    tracing::error!("[{}] invalid cron expression '{}': {}", alias, interval, e);
                }
            }
        }
    }

    // 30s granularity is fine — cron's smallest unit is 1 minute
    let mut cron_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    let mut last_cron_check = Local::now().naive_local();

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received shutdown signal, exiting");
                break;
            }
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                let preview: String = msg.text.chars().take(50).collect();
                let truncated = if msg.text.len() > preview.len() { "…" } else { "" };
                tracing::info!("received message on {}: {}{}", msg.channel, preview, truncated);
                tracing::debug!("full message: {}", msg.text);

                for (alias, job_config) in &app.jobs {
                    if !matches_input(job_config, &msg) {
                        continue;
                    }

                    let db_path = db_path.clone();
                    let app = app.clone();
                    let alias = alias.clone();
                    let job_config = job_config.clone();
                    let msg = msg.clone();
                    let channels = channels.clone();
                    tokio::spawn(async move {
                        match run_channel_job(&app, &db_path, &alias, &job_config, &msg).await {
                            Ok(result) => {
                                for out in &job_config.outputs {
                                    if out.channel.is_some() {
                                        if let Some(ch) = channels.get(&msg.channel) {
                                            if let Err(e) = ch.send(&msg.sender, &result).await {
                                                tracing::error!("failed to send response on {}: {}", msg.channel, e);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("job {} failed: {}", alias, e);
                            }
                        }
                    });
                }
            }
            _ = cron_interval.tick() => {
                let now = Local::now().naive_local();
                for (alias, schedule) in &schedules {
                    if let Some(next) = schedule.next_from(last_cron_check) {
                        if next <= now {
                            tracing::info!("[{}] cron triggered", alias);
                            if let Some((_, job_config)) = app.jobs.iter().find(|(a, _)| a == alias) {
                                let db_path = db_path.clone();
                                let app = app.clone();
                                let alias = alias.clone();
                                let job_config = job_config.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = run_scheduled_job(&app, &db_path, &alias, &job_config).await {
                                        tracing::error!("[{}] scheduled job failed: {}", alias, e);
                                    }
                                });
                            }
                        }
                    }
                }
                last_cron_check = now;
            }
        }
    }

    Ok(())
}

async fn run_scheduled_job(
    app: &AppConfig,
    db_path: &PathBuf,
    alias: &str,
    job_config: &JobConfig,
) -> Result<String> {
    let store = Store::open(db_path)?;

    let prompt_template = job_config
        .job
        .as_ref()
        .and_then(|j| j.prompt.as_deref())
        .ok_or_else(|| {
            crate::error::Error::Config(format!("scheduled job '{alias}' has no prompt"))
        })?;

    let env_wrapper = env::create_environment(job_config.environment.as_ref())?;
    env_wrapper.ensure_ready()?;
    let agent = agent::create_agent(&job_config.agent)?;

    let mut ctx = RenderContext::new(app.dictionary.clone());
    ctx.memories = store.get_memories(alias, 100)?;
    ctx.secrets = app.secrets.clone();

    let rendered_prompt = template::render(prompt_template, &ctx).await?;
    let system_prompt = job_config.agent.prompt.as_deref();

    let result = agent
        .run(&rendered_prompt, system_prompt, env_wrapper.as_ref())
        .await?;

    // If there's a history prompt, ask the agent to summarize before storing
    let result_to_store = if let Some(history) = &job_config.history {
        let summary_prompt = format!("{}\n\n{}", history.prompt, result);
        match agent.run(&summary_prompt, None, env_wrapper.as_ref()).await {
            Ok(summary) => summary,
            Err(e) => {
                tracing::warn!("[{}] history summarization failed: {}", alias, e);
                result.clone()
            }
        }
    } else {
        result.clone()
    };

    store.store_run(alias, &result_to_store)?;

    for output_section in &job_config.outputs {
        let rendered_message = if let Some(msg_template) = &output_section.message {
            let mut output_ctx = ctx.clone();
            output_ctx.result = Some(result.clone());
            Some(template::render(msg_template, &output_ctx).await?)
        } else {
            None
        };
        if let Err(e) = output::dispatch(output_section, &result, rendered_message.as_deref()).await
        {
            tracing::error!("[{}] output dispatch failed: {}", alias, e);
        }
    }

    Ok(result)
}

/// Does the incoming message match this job's input config?
pub fn matches_input(job: &JobConfig, msg: &IncomingMessage) -> bool {
    let input = match &job.input {
        Some(input) => input,
        None => return false,
    };

    if input.channel != msg.channel {
        return false;
    }

    if let Some(allowed) = &input.allowed_senders {
        if !allowed.iter().any(|s| s == &msg.sender) {
            return false;
        }
    }

    if let Some(trigger) = &input.trigger {
        if trigger != "*" {
            let text_lower = msg.text.to_lowercase();
            let trigger_lower = trigger.to_lowercase();
            let mode = input.trigger_match.unwrap_or_default();
            let matched = match mode {
                TriggerMatch::Start => text_lower.starts_with(&trigger_lower),
                TriggerMatch::End => text_lower.ends_with(&trigger_lower),
                TriggerMatch::Anywhere => text_lower.contains(&trigger_lower),
            };
            if !matched {
                return false;
            }
        }
    }

    true
}

async fn run_channel_job(
    app: &AppConfig,
    db_path: &PathBuf,
    alias: &str,
    job_config: &JobConfig,
    msg: &IncomingMessage,
) -> Result<String> {
    let store = Store::open(db_path)?;

    let env_wrapper = env::create_environment(job_config.environment.as_ref())?;
    env_wrapper.ensure_ready()?;
    let agent = agent::create_agent(&job_config.agent)?;

    // Use job's prompt template if available, otherwise the raw message
    let prompt_template = job_config
        .job
        .as_ref()
        .and_then(|j| j.prompt.as_deref())
        .unwrap_or(&msg.text);

    let memories = store.get_memories(alias, 100)?;
    let mut ctx = RenderContext::new(app.dictionary.clone());
    ctx.result = None;
    ctx.message = Some(msg.text.clone());
    ctx.sender = Some(msg.sender.clone());
    ctx.memories = memories;
    ctx.secrets = app.secrets.clone();

    let rendered_prompt = template::render(prompt_template, &ctx).await?;

    // Prepend conversation history if session tracking is on
    let full_prompt = if let Some(session) = &job_config.session {
        let history = store.get_session(&msg.channel, &msg.sender, session.context)?;
        build_session_context(&history, &rendered_prompt)
    } else {
        rendered_prompt.clone()
    };

    let system_prompt = job_config.agent.prompt.as_deref();
    let result = agent
        .run(&full_prompt, system_prompt, env_wrapper.as_ref())
        .await?;

    if job_config.session.is_some() {
        store.store_message(&msg.channel, &msg.sender, crate::store::MessageRole::User, &msg.text)?;
        store.store_message(&msg.channel, &msg.sender, crate::store::MessageRole::Assistant, &result)?;
    }

    store.store_run(alias, &result)?;

    Ok(result)
}

/// Flatten session history into a `User: ... / Assistant: ...` conversation string.
pub fn build_session_context(history: &[SessionMessage], current_message: &str) -> String {
    let mut parts = Vec::new();
    for m in history {
        let role = match m.role {
            crate::store::MessageRole::User => "User",
            crate::store::MessageRole::Assistant => "Assistant",
        };
        parts.push(format!("{}: {}", role, m.content));
    }
    parts.push(format!("User: {}", current_message));
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{AgentName, AgentSection, InputSection, JobConfig, TriggerMatch};
    use crate::store::{MessageRole, SessionMessage};

    fn make_agent() -> AgentSection {
        AgentSection {
            name: AgentName::Claude,
            prompt: None,
            host: None,
            model: None,
            skip_permissions: None,
            allowed_tools: None,
        }
    }

    fn make_msg(channel: &str, text: &str) -> IncomingMessage {
        IncomingMessage {
            channel: channel.into(),
            sender: "local".into(),
            text: text.into(),
        }
    }

    fn make_job(input: Option<InputSection>) -> JobConfig {
        JobConfig {
            name: None,
            alias: None,
            agent: make_agent(),
            job: None,
            environment: None,
            outputs: vec![],
            input,
            session: None,
            history: None,
        }
    }

    #[test]
    fn test_matches_input_channel_match() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: None,
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "hello");
        assert!(matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_channel_mismatch() {
        let job = make_job(Some(InputSection {
            channel: "irc".into(),
            trigger: None,
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "hello");
        assert!(!matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_no_input() {
        let job = make_job(None);
        let msg = make_msg("stdin", "hello");
        assert!(!matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_trigger_match() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: Some("weather".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "weather in Lisbon");
        assert!(matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_trigger_mismatch() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: Some("weather".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "hello world");
        assert!(!matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_trigger_wildcard() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: Some("*".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "anything at all");
        assert!(matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_no_trigger() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: None,
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "anything at all");
        assert!(matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_trigger_anywhere_default() {
        let job = make_job(Some(InputSection {
            channel: "telegram".into(),
            trigger: Some("vatic".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        assert!(matches_input(
            &job,
            &make_msg("telegram", "hey vatic help me")
        ));
        assert!(matches_input(
            &job,
            &make_msg("telegram", "vatic do something")
        ));
        assert!(matches_input(&job, &make_msg("telegram", "ask vatic")));
        assert!(!matches_input(&job, &make_msg("telegram", "hello world")));
    }

    #[test]
    fn test_matches_input_trigger_start() {
        let job = make_job(Some(InputSection {
            channel: "telegram".into(),
            trigger: Some("vatic".into()),
            trigger_match: Some(TriggerMatch::Start),
            allowed_senders: None,
        }));
        assert!(matches_input(&job, &make_msg("telegram", "vatic help me")));
        assert!(!matches_input(
            &job,
            &make_msg("telegram", "hey vatic help me")
        ));
    }

    #[test]
    fn test_matches_input_trigger_end() {
        let job = make_job(Some(InputSection {
            channel: "telegram".into(),
            trigger: Some("vatic".into()),
            trigger_match: Some(TriggerMatch::End),
            allowed_senders: None,
        }));
        assert!(matches_input(&job, &make_msg("telegram", "ask vatic")));
        assert!(!matches_input(&job, &make_msg("telegram", "vatic help me")));
    }

    #[test]
    fn test_build_context_empty_history() {
        let history: Vec<SessionMessage> = vec![];
        let result = build_session_context(&history, "hello bot");
        assert_eq!(result, "User: hello bot");
    }

    #[test]
    fn test_build_context_with_history() {
        let history = vec![
            SessionMessage {
                role: MessageRole::User,
                content: "m1".into(),
                timestamp: "2026-01-01 00:00:00".into(),
            },
            SessionMessage {
                role: MessageRole::Assistant,
                content: "r1".into(),
                timestamp: "2026-01-01 00:00:01".into(),
            },
            SessionMessage {
                role: MessageRole::User,
                content: "m2".into(),
                timestamp: "2026-01-01 00:00:02".into(),
            },
            SessionMessage {
                role: MessageRole::Assistant,
                content: "r2".into(),
                timestamp: "2026-01-01 00:00:03".into(),
            },
        ];
        let result = build_session_context(&history, "current");
        assert_eq!(
            result,
            "User: m1\nAssistant: r1\nUser: m2\nAssistant: r2\nUser: current"
        );
    }

    #[test]
    fn test_matches_input_allowed_senders_match() {
        let job = make_job(Some(InputSection {
            channel: "telegram".into(),
            trigger: Some("*".into()),
            trigger_match: None,
            allowed_senders: Some(vec!["franz".into(), "alice".into()]),
        }));
        let msg = IncomingMessage {
            channel: "telegram".into(),
            sender: "franz".into(),
            text: "hello".into(),
        };
        assert!(matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_allowed_senders_reject() {
        let job = make_job(Some(InputSection {
            channel: "telegram".into(),
            trigger: Some("*".into()),
            trigger_match: None,
            allowed_senders: Some(vec!["franz".into(), "alice".into()]),
        }));
        let msg = IncomingMessage {
            channel: "telegram".into(),
            sender: "attacker".into(),
            text: "hello".into(),
        };
        assert!(!matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_no_allowed_senders_allows_all() {
        let job = make_job(Some(InputSection {
            channel: "telegram".into(),
            trigger: Some("*".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = IncomingMessage {
            channel: "telegram".into(),
            sender: "anyone".into(),
            text: "hello".into(),
        };
        assert!(matches_input(&job, &msg));
    }

    #[test]
    fn test_build_context_content_with_newlines() {
        let history = vec![SessionMessage {
            role: MessageRole::User,
            content: "line1\nline2".into(),
            timestamp: "2026-01-01 00:00:00".into(),
        }];
        let result = build_session_context(&history, "next");
        assert_eq!(result, "User: line1\nline2\nUser: next");
    }

    #[test]
    fn test_matches_input_case_insensitive_trigger() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: Some("Weather".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "weather in Lisbon");
        assert!(matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_trigger_start_with_case() {
        let job = make_job(Some(InputSection {
            channel: "telegram".into(),
            trigger: Some("Vatic".into()),
            trigger_match: Some(TriggerMatch::Start),
            allowed_senders: None,
        }));
        assert!(matches_input(&job, &make_msg("telegram", "vatic help")));
    }

    #[test]
    fn test_matches_input_empty_message_text() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: Some("weather".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "");
        assert!(!matches_input(&job, &msg));
    }

    #[test]
    fn test_matches_input_trigger_substring_of_word() {
        let job = make_job(Some(InputSection {
            channel: "stdin".into(),
            trigger: Some("cat".into()),
            trigger_match: None,
            allowed_senders: None,
        }));
        let msg = make_msg("stdin", "concatenate");
        assert!(matches_input(&job, &msg));
    }
}
