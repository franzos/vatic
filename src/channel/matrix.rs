use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use super::{Channel, IncomingMessage};

pub struct MatrixChannel {
    homeserver: String,
    user: String,
    password: String,
    data_dir: PathBuf,
    client: Arc<Mutex<Option<matrix_sdk::Client>>>,
}

impl MatrixChannel {
    pub fn new(homeserver: String, user: String, password: String, data_dir: PathBuf) -> Self {
        Self {
            homeserver,
            user,
            password,
            data_dir,
            client: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Channel for MatrixChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> crate::error::Result<()> {
        use matrix_sdk::config::SyncSettings;
        use matrix_sdk::ruma::events::room::message::{MessageType, OriginalSyncRoomMessageEvent};
        use matrix_sdk::Client;

        std::fs::create_dir_all(&self.data_dir).map_err(|e| {
            crate::error::Error::Channel(format!(
                "cannot create matrix data dir {}: {e}",
                self.data_dir.display()
            ))
        })?;

        let db_path = self.data_dir.join("matrix-store");

        let client = Client::builder()
            .homeserver_url(&self.homeserver)
            .sqlite_store(&db_path, None)
            .build()
            .await
            .map_err(|e| {
                crate::error::Error::Channel(format!("matrix client build failed: {e}"))
            })?;

        // The sqlite store handles session persistence across restarts
        client
            .matrix_auth()
            .login_username(&self.user, &self.password)
            .initial_device_display_name("vatic")
            .send()
            .await
            .map_err(|e| crate::error::Error::Channel(format!("matrix login failed: {e}")))?;

        tracing::info!("matrix connected as {}", self.user);

        // Stash client so send() can use it later
        {
            let mut slot = self.client.lock().await;
            *slot = Some(client.clone());
        }

        client.add_event_handler(
            move |event: OriginalSyncRoomMessageEvent, room: matrix_sdk::Room| {
                let tx = tx.clone();
                async move {
                    // Don't respond to our own messages
                    if room
                        .client()
                        .user_id()
                        .is_some_and(|uid| uid == event.sender)
                    {
                        return;
                    }

                    let text = match event.content.msgtype {
                        MessageType::Text(text_content) => text_content.body,
                        _ => return,
                    };

                    if text.is_empty() {
                        return;
                    }

                    // room_id as sender so replies go back to the right room
                    let sender = room.room_id().to_string();

                    let msg = IncomingMessage {
                        channel: "matrix".to_string(),
                        sender,
                        text,
                    };

                    let _ = tx.send(msg).await;
                }
            },
        );

        // This blocks forever — initial sync, then incremental from there
        tracing::info!("matrix syncing, listening for messages");
        client
            .sync(SyncSettings::default())
            .await
            .map_err(|e| crate::error::Error::Channel(format!("matrix sync failed: {e}")))?;

        Ok(())
    }

    async fn send(&self, to: &str, message: &str) -> crate::error::Result<()> {
        use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
        use matrix_sdk::ruma::RoomId;

        // Clone out of the lock — can't hold a mutex across await
        let client = {
            let guard = self.client.lock().await;
            guard
                .as_ref()
                .ok_or_else(|| {
                    crate::error::Error::Channel("matrix client not connected".to_string())
                })?
                .clone()
        };

        let room_id = <&RoomId>::try_from(to).map_err(|e| {
            crate::error::Error::Channel(format!("invalid matrix room_id '{}': {}", to, e))
        })?;

        let room = client.get_room(room_id).ok_or_else(|| {
            crate::error::Error::Channel(format!("matrix room '{}' not found", to))
        })?;

        let content = RoomMessageEventContent::text_plain(message);
        room.send(content)
            .await
            .map_err(|e| crate::error::Error::Channel(format!("matrix send failed: {e}")))?;

        Ok(())
    }

    fn name(&self) -> &str {
        "matrix"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_matrix_channel_name() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "@bot:matrix.org".to_string(),
            "password".to_string(),
            PathBuf::from("/tmp/test-matrix"),
        );
        assert_eq!(ch.name(), "matrix");
    }

    #[test]
    fn test_matrix_channel_retains_homeserver() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "@bot:matrix.org".to_string(),
            "secret".to_string(),
            PathBuf::from("/tmp/test-matrix"),
        );
        assert_eq!(ch.homeserver, "https://matrix.org");
    }

    #[test]
    fn test_matrix_channel_retains_user() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "@bot:matrix.org".to_string(),
            "secret".to_string(),
            PathBuf::from("/tmp/test-matrix"),
        );
        assert_eq!(ch.user, "@bot:matrix.org");
    }

    #[test]
    fn test_matrix_channel_retains_password() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "@bot:matrix.org".to_string(),
            "secret".to_string(),
            PathBuf::from("/tmp/test-matrix"),
        );
        assert_eq!(ch.password, "secret");
    }

    #[test]
    fn test_matrix_channel_retains_data_dir() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "@bot:matrix.org".to_string(),
            "secret".to_string(),
            PathBuf::from("/data/matrix"),
        );
        assert_eq!(ch.data_dir, PathBuf::from("/data/matrix"));
    }

    #[tokio::test]
    async fn test_matrix_channel_client_starts_as_none() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "@bot:matrix.org".to_string(),
            "secret".to_string(),
            PathBuf::from("/tmp/test-matrix"),
        );
        let guard = ch.client.lock().await;
        assert!(guard.is_none());
    }
}
