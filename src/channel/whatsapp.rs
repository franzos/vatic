use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use super::{Channel, IncomingMessage};

pub struct WhatsAppChannel {
    data_dir: PathBuf,
    client: Arc<Mutex<Option<Arc<whatsapp_rust::Client>>>>,
}

impl WhatsAppChannel {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            client: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Channel for WhatsAppChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> crate::error::Result<()> {
        use wacore::types::events::Event;
        use whatsapp_rust::bot::Bot;
        use whatsapp_rust::store::SqliteStore;
        use whatsapp_rust_tokio_transport::TokioWebSocketTransportFactory;
        use whatsapp_rust_ureq_http_client::UreqHttpClient;

        std::fs::create_dir_all(&self.data_dir).map_err(|e| {
            crate::error::Error::Channel(format!(
                "cannot create whatsapp data dir {}: {e}",
                self.data_dir.display()
            ))
        })?;

        let db_path = self.data_dir.join("whatsapp.db");
        let backend = Arc::new(
            SqliteStore::new(db_path.to_str().unwrap_or("whatsapp.db"))
                .await
                .map_err(|e| crate::error::Error::Channel(format!("sqlite init failed: {e}")))?,
        );

        let client_slot = Arc::clone(&self.client);
        let tx_clone = tx.clone();

        let mut bot = Bot::builder()
            .with_backend(backend)
            .with_transport_factory(TokioWebSocketTransportFactory::new())
            .with_http_client(UreqHttpClient::new())
            .on_event(move |event, client| {
                let tx = tx_clone.clone();
                let client_slot = Arc::clone(&client_slot);
                async move {
                    match event {
                        Event::PairingQrCode { code, .. } => {
                            tracing::info!("scan this QR code with WhatsApp:");
                            println!("\n{}\n", code);
                        }
                        Event::Connected { .. } => {
                            tracing::info!("whatsapp connected");
                            let mut slot = client_slot.lock().await;
                            *slot = Some(client);
                        }
                        Event::LoggedOut { reason, .. } => {
                            tracing::warn!("whatsapp logged out: {:?}", reason);
                            let mut slot = client_slot.lock().await;
                            *slot = None;
                        }
                        Event::Message(message, info) => {
                            if info.source.is_from_me {
                                return;
                            }
                            // Text can live in `conversation` or nested in `extended_text_message`
                            let text = message
                                .conversation
                                .as_deref()
                                .or_else(|| {
                                    message
                                        .extended_text_message
                                        .as_ref()
                                        .and_then(|m| m.text.as_deref())
                                })
                                .unwrap_or("")
                                .to_string();

                            if text.is_empty() {
                                return;
                            }

                            let sender = info.source.sender.to_string();
                            let msg = IncomingMessage {
                                channel: "whatsapp".to_string(),
                                sender,
                                text,
                            };
                            let _ = tx.send(msg).await;
                        }
                        _ => {}
                    }
                }
            })
            .build()
            .await
            .map_err(|e| crate::error::Error::Channel(format!("whatsapp bot build failed: {e}")))?;

        bot.run()
            .await
            .map_err(|e| crate::error::Error::Channel(format!("whatsapp bot run failed: {e}")))?
            .await
            .map_err(|e| crate::error::Error::Channel(format!("whatsapp bot task failed: {e}")))?;

        Ok(())
    }

    async fn send(&self, to: &str, message: &str) -> crate::error::Result<()> {
        // Clone out of the lock — can't hold a mutex across await
        let client = {
            let guard = self.client.lock().await;
            guard
                .as_ref()
                .ok_or_else(|| crate::error::Error::Channel("whatsapp not connected".to_string()))?
                .clone()
        };

        let jid: wacore::Jid = to
            .parse()
            .map_err(|e| crate::error::Error::Channel(format!("invalid JID '{}': {}", to, e)))?;

        let mut msg = waproto::wa::Message::default();
        msg.conversation = Some(message.to_string());

        client
            .send_message(jid, msg)
            .await
            .map_err(|e| crate::error::Error::Channel(format!("whatsapp send failed: {e}")))?;

        Ok(())
    }

    fn name(&self) -> &str {
        "whatsapp"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_whatsapp_channel_name() {
        let ch = WhatsAppChannel::new(PathBuf::from("/tmp/test-whatsapp"));
        assert_eq!(ch.name(), "whatsapp");
    }

    #[test]
    fn test_whatsapp_channel_retains_data_dir() {
        let ch = WhatsAppChannel::new(PathBuf::from("/data/whatsapp"));
        assert_eq!(ch.data_dir, PathBuf::from("/data/whatsapp"));
    }

    #[tokio::test]
    async fn test_whatsapp_channel_client_starts_as_none() {
        let ch = WhatsAppChannel::new(PathBuf::from("/tmp/test-whatsapp"));
        let guard = ch.client.lock().await;
        assert!(guard.is_none());
    }
}
