use super::{Channel, IncomingMessage};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

pub struct StdinChannel;

#[async_trait::async_trait]
impl Channel for StdinChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> crate::error::Result<()> {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            let msg = IncomingMessage {
                channel: "stdin".to_string(),
                sender: "local".to_string(),
                text: line,
            };
            if tx.send(msg).await.is_err() {
                break; // receiver dropped
            }
        }
        Ok(())
    }

    async fn send(&self, _to: &str, message: &str) -> crate::error::Result<()> {
        println!("{}", message);
        Ok(())
    }

    fn name(&self) -> &str {
        "stdin"
    }
}
