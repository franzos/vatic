use std::collections::{HashSet, VecDeque};

use tokio::sync::mpsc;

const MAX_SEEN: usize = 10_000;

use super::{Channel, IncomingMessage};

/// Strip CR/LF to prevent header injection.
fn sanitize_header(value: &str) -> String {
    value.chars().filter(|c| *c != '\r' && *c != '\n').collect()
}

/// Prepend subject to body when present, otherwise just the body.
fn format_email_text(subject: &str, body: &str) -> String {
    if subject.is_empty() {
        body.to_string()
    } else {
        format!("{}\n\n{}", subject, body)
    }
}

pub struct EmailChannel {
    poll_interval: u64,
    account: Option<String>,
}

impl EmailChannel {
    pub fn new(poll_interval: u64, account: Option<String>) -> Self {
        Self {
            poll_interval,
            account,
        }
    }
}

#[async_trait::async_trait]
impl Channel for EmailChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> crate::error::Result<()> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut seen_order: VecDeque<String> = VecDeque::new();
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(self.poll_interval));

        loop {
            interval.tick().await;

            let envelopes = match list_envelopes(self.account.as_deref()).await {
                Ok(lines) => lines,
                Err(e) => {
                    tracing::error!("himalaya envelope list failed: {e}");
                    continue;
                }
            };

            for envelope in &envelopes {
                if seen.contains(&envelope.id) {
                    continue;
                }
                seen.insert(envelope.id.clone());
                seen_order.push_back(envelope.id.clone());
                while seen.len() > MAX_SEEN {
                    if let Some(old) = seen_order.pop_front() {
                        seen.remove(&old);
                    }
                }

                let body = match read_message(&envelope.id, self.account.as_deref()).await {
                    Ok(body) => body,
                    Err(e) => {
                        tracing::error!("himalaya message read {} failed: {e}", envelope.id);
                        continue;
                    }
                };

                let text = format_email_text(&envelope.subject, &body);

                let msg = IncomingMessage {
                    channel: "himalaya".to_string(),
                    sender: envelope.from.clone(),
                    text,
                };

                if tx.send(msg).await.is_err() {
                    return Ok(()); // receiver dropped
                }
            }
        }
    }

    async fn send(&self, to: &str, message: &str) -> crate::error::Result<()> {
        let mut args = vec!["message", "send"];
        if let Some(ref acct) = self.account {
            args.extend(["--account", acct]);
        }

        // Build minimal RFC 2822 message (strip CR/LF from header values)
        let safe_to = sanitize_header(to);
        let email = format!("To: {}\r\nSubject: Re: vatic\r\n\r\n{}", safe_to, message);

        let mut cmd = tokio::process::Command::new("himalaya");
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| crate::error::Error::Channel(format!("cannot spawn himalaya: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(email.as_bytes()).await.map_err(|e| {
                crate::error::Error::Channel(format!("himalaya stdin write failed: {e}"))
            })?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| crate::error::Error::Channel(format!("himalaya send failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::error::Error::Channel(format!(
                "himalaya send failed: {}",
                stderr.trim()
            )));
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "himalaya"
    }
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub id: String,
    pub flags: String,
    pub from: String,
    pub subject: String,
}

/// Parse one tab-separated line from `himalaya envelope list`.
pub fn parse_envelope_line(line: &str) -> Option<Envelope> {
    let parts: Vec<&str> = line.splitn(4, '\t').collect();
    if parts.len() < 4 {
        return None;
    }
    let id = parts[0].trim().to_string();
    // Skip header lines
    if id == "ID" || id.is_empty() {
        return None;
    }
    Some(Envelope {
        id,
        flags: parts[1].trim().to_string(),
        from: parts[2].trim().to_string(),
        subject: parts[3].trim().to_string(),
    })
}

async fn list_envelopes(
    account: Option<&str>,
) -> std::result::Result<Vec<Envelope>, crate::error::Error> {
    let mut args = vec!["envelope", "list", "--max-width", "0"];
    if let Some(acct) = account {
        args.extend(["--account", acct]);
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("himalaya")
            .args(&args)
            .output(),
    )
    .await
    .map_err(|_| crate::error::Error::Channel("himalaya envelope list timed out".to_string()))?
    .map_err(|e| crate::error::Error::Channel(format!("cannot run himalaya: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::Error::Channel(format!(
            "himalaya envelope list failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelopes = stdout.lines().filter_map(parse_envelope_line).collect();
    Ok(envelopes)
}

async fn read_message(
    id: &str,
    account: Option<&str>,
) -> std::result::Result<String, crate::error::Error> {
    let mut args = vec!["message", "read", id, "--mime-type", "plain"];
    if let Some(acct) = account {
        args.extend(["--account", acct]);
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("himalaya")
            .args(&args)
            .output(),
    )
    .await
    .map_err(|_| crate::error::Error::Channel("himalaya message read timed out".to_string()))?
    .map_err(|e| crate::error::Error::Channel(format!("cannot run himalaya: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::Error::Channel(format!(
            "himalaya message read failed: {}",
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_channel_name() {
        let ch = EmailChannel::new(60, None);
        assert_eq!(ch.name(), "himalaya");
    }

    #[test]
    fn test_parse_envelope_line_valid() {
        let line = "42\t\tuser@example.com\tHello World";
        let env = parse_envelope_line(line).unwrap();
        assert_eq!(env.id, "42");
        assert_eq!(env.from, "user@example.com");
        assert_eq!(env.subject, "Hello World");
    }

    #[test]
    fn test_parse_envelope_line_with_flags() {
        let line = "7\tSeen\tjane@test.org\tMeeting tomorrow";
        let env = parse_envelope_line(line).unwrap();
        assert_eq!(env.id, "7");
        assert_eq!(env.flags, "Seen");
        assert_eq!(env.from, "jane@test.org");
        assert_eq!(env.subject, "Meeting tomorrow");
    }

    #[test]
    fn test_parse_envelope_line_header() {
        let line = "ID\tFLAGS\tFROM\tSUBJECT";
        assert!(parse_envelope_line(line).is_none());
    }

    #[test]
    fn test_parse_envelope_line_short() {
        let line = "42\tSeen";
        assert!(parse_envelope_line(line).is_none());
    }

    #[test]
    fn test_sanitize_header_clean() {
        assert_eq!(sanitize_header("user@example.com"), "user@example.com");
    }

    #[test]
    fn test_sanitize_header_strips_cr() {
        assert_eq!(sanitize_header("user@example.com\r"), "user@example.com");
    }

    #[test]
    fn test_sanitize_header_strips_lf() {
        assert_eq!(
            sanitize_header("user@example.com\nBcc: evil@hack.com"),
            "user@example.comBcc: evil@hack.com"
        );
    }

    #[test]
    fn test_sanitize_header_strips_crlf() {
        assert_eq!(
            sanitize_header("user@example.com\r\nBcc: evil@hack.com"),
            "user@example.comBcc: evil@hack.com"
        );
    }

    #[test]
    fn test_format_email_text_with_subject() {
        assert_eq!(
            format_email_text("Hello", "Body text"),
            "Hello\n\nBody text"
        );
    }

    #[test]
    fn test_format_email_text_empty_subject() {
        assert_eq!(format_email_text("", "Body text"), "Body text");
    }

    #[test]
    fn test_parse_envelope_line_empty_string() {
        assert!(parse_envelope_line("").is_none());
    }

    #[test]
    fn test_parse_envelope_line_empty_id() {
        let line = "\tSeen\tuser@example.com\tHello";
        assert!(parse_envelope_line(line).is_none());
    }

    #[test]
    fn test_parse_envelope_line_only_tabs() {
        let line = "\t\t\t";
        assert!(parse_envelope_line(line).is_none());
    }

    #[test]
    fn test_parse_envelope_line_subject_with_tabs() {
        // splitn(4, '\t') means the subject field keeps any extra tabs
        let line = "10\tSeen\tuser@example.com\tHello\tWorld";
        let env = parse_envelope_line(line).unwrap();
        assert_eq!(env.subject, "Hello\tWorld");
    }

    #[test]
    fn test_sanitize_header_only_crlf() {
        assert_eq!(sanitize_header("\r\n\r\n"), "");
    }

    #[test]
    fn test_sanitize_header_only_cr() {
        assert_eq!(sanitize_header("\r\r\r"), "");
    }

    #[test]
    fn test_sanitize_header_only_lf() {
        assert_eq!(sanitize_header("\n\n"), "");
    }

    #[test]
    fn test_sanitize_header_empty() {
        assert_eq!(sanitize_header(""), "");
    }

    #[test]
    fn test_format_email_text_both_empty() {
        assert_eq!(format_email_text("", ""), "");
    }

    #[test]
    fn test_format_email_text_empty_body() {
        assert_eq!(format_email_text("Subject", ""), "Subject\n\n");
    }
}
