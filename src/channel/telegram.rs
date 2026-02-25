use std::sync::Arc;

use frankenstein::client_reqwest::Bot;
use frankenstein::methods::{GetUpdatesParams, SendMessageParams};
use frankenstein::types::AllowedUpdate;
use frankenstein::updates::UpdateContent;
use frankenstein::AsyncTelegramApi;
use tokio::sync::{mpsc, Mutex};

use super::{Channel, IncomingMessage};

/// Remove the first @botname mention so the prompt isn't polluted with it.
fn strip_bot_mention(text: &str, bot_username: Option<&str>) -> String {
    if let Some(mention) = bot_username {
        let lower = text.to_lowercase();
        if let Some(pos) = lower.find(mention) {
            let mut s = text.to_string();
            s.replace_range(pos..pos + mention.len(), "");
            s.trim().to_string()
        } else {
            text.to_string()
        }
    } else {
        text.to_string()
    }
}

pub struct TelegramChannel {
    token: String,
    bot: Arc<Mutex<Option<Bot>>>,
}

impl TelegramChannel {
    pub fn new(token: String) -> Self {
        Self {
            token,
            bot: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Channel for TelegramChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> crate::error::Result<()> {
        let bot = Bot::new(&self.token);

        // Separate Bot instance for send() — frankenstein's Bot isn't Clone-friendly
        {
            let mut slot = self.bot.lock().await;
            *slot = Some(Bot::new(&self.token));
        }

        // We need the bot's username to strip @mentions from incoming text
        let bot_username = match bot.get_me().await {
            Ok(me) => me.result.username.map(|u| format!("@{}", u.to_lowercase())),
            Err(e) => {
                tracing::warn!("telegram get_me failed, cannot strip mentions: {e}");
                None
            }
        };

        let mut offset: Option<i64> = None;

        tracing::info!("telegram channel polling for updates");

        loop {
            let mut params = GetUpdatesParams::builder()
                .allowed_updates(vec![AllowedUpdate::Message])
                .timeout(30)
                .build();

            if let Some(off) = offset {
                params.offset = Some(off);
            }

            match bot.get_updates(&params).await {
                Ok(response) => {
                    for update in response.result {
                        offset = Some(update.update_id as i64 + 1);

                        let message = match update.content {
                            UpdateContent::Message(msg) => msg,
                            _ => continue,
                        };

                        let raw_text = match message.text {
                            Some(t) => t,
                            None => continue,
                        };

                        // Clean up the @mention before it reaches the agent
                        let text = strip_bot_mention(&raw_text, bot_username.as_deref());

                        if text.is_empty() {
                            continue;
                        }

                        let sender = message.chat.id.to_string();

                        let msg = IncomingMessage {
                            channel: "telegram".to_string(),
                            sender,
                            text,
                        };

                        if tx.send(msg).await.is_err() {
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("telegram get_updates failed: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn send(&self, to: &str, message: &str) -> crate::error::Result<()> {
        // Clone out of the lock — can't hold a mutex across await
        let bot = {
            let guard = self.bot.lock().await;
            guard
                .as_ref()
                .ok_or_else(|| {
                    crate::error::Error::Channel("telegram bot not initialized".to_string())
                })?
                .clone()
        };

        let chat_id: i64 = to.parse().map_err(|e| {
            crate::error::Error::Channel(format!("invalid telegram chat_id '{}': {}", to, e))
        })?;

        let params = SendMessageParams::builder()
            .chat_id(chat_id)
            .text(message)
            .build();

        bot.send_message(&params)
            .await
            .map_err(|e| crate::error::Error::Channel(format!("telegram send failed: {e}")))?;

        Ok(())
    }

    fn name(&self) -> &str {
        "telegram"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_channel_name() {
        let ch = TelegramChannel::new("fake-token".to_string());
        assert_eq!(ch.name(), "telegram");
    }

    #[test]
    fn test_strip_mention_at_start() {
        assert_eq!(strip_bot_mention("@mybot hello", Some("@mybot")), "hello");
    }

    #[test]
    fn test_strip_mention_case_insensitive() {
        assert_eq!(strip_bot_mention("@MyBot hello", Some("@mybot")), "hello");
    }

    #[test]
    fn test_strip_mention_in_middle() {
        assert_eq!(
            strip_bot_mention("hey @mybot do this", Some("@mybot")),
            "hey  do this"
        );
    }

    #[test]
    fn test_strip_mention_not_present() {
        assert_eq!(
            strip_bot_mention("hello world", Some("@mybot")),
            "hello world"
        );
    }

    #[test]
    fn test_strip_mention_no_username() {
        assert_eq!(strip_bot_mention("hello world", None), "hello world");
    }

    #[test]
    fn test_strip_mention_only_mention() {
        assert_eq!(strip_bot_mention("@mybot", Some("@mybot")), "");
    }

    #[test]
    fn test_strip_mention_with_trailing_whitespace() {
        assert_eq!(strip_bot_mention("@mybot   ", Some("@mybot")), "");
    }

    #[test]
    fn test_strip_mention_multiple_occurrences() {
        // Only the first occurrence is stripped; second remains
        assert_eq!(
            strip_bot_mention("@mybot hello @mybot", Some("@mybot")),
            "hello @mybot"
        );
    }

    #[test]
    fn test_strip_mention_empty_text() {
        assert_eq!(strip_bot_mention("", Some("@mybot")), "");
    }

    #[test]
    fn test_strip_mention_empty_mention_string() {
        // Empty string is found at position 0, replaces 0 chars, then trims
        assert_eq!(strip_bot_mention("hello", Some("")), "hello");
    }

    #[test]
    fn test_strip_mention_different_case_in_middle() {
        assert_eq!(
            strip_bot_mention("hey @MYBOT help", Some("@mybot")),
            "hey  help"
        );
    }
}
