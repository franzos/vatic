pub mod email;
pub mod matrix;
pub mod stdin;
pub mod telegram;
#[cfg(feature = "whatsapp")]
pub mod whatsapp;

use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub channel: String,
    pub sender: String,
    pub text: String,
}

#[async_trait::async_trait]
pub trait Channel: Send + Sync {
    /// Begin listening; incoming messages go through `tx`.
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> crate::error::Result<()>;

    /// Send a response back to a user/room.
    async fn send(&self, to: &str, message: &str) -> crate::error::Result<()>;

    /// Identifier used for routing and logging.
    fn name(&self) -> &str;
}
