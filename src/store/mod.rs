use rusqlite::{Connection, OptionalExtension};
use std::path::PathBuf;

use crate::error::Result;
use crate::template::functions::MemoryEntry;

#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open or create the database at the given path.
    pub fn open(path: &PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// In-memory database for tests.
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS job_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_alias TEXT NOT NULL,
                result TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel TEXT NOT NULL,
                sender TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_job_runs_alias ON job_runs(job_alias);
            CREATE INDEX IF NOT EXISTS idx_sessions_channel_sender ON sessions(channel, sender);
        ",
        )?;
        Ok(())
    }

    /// Persist a job run result.
    pub fn store_run(&self, job_alias: &str, result: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO job_runs (job_alias, result) VALUES (?1, ?2)",
            rusqlite::params![job_alias, result],
        )?;
        Ok(())
    }

    /// Get a single memory for a job. offset 0 = latest, 1 = second latest, etc.
    pub fn get_memory(&self, job_alias: &str, offset: u32) -> Result<Option<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT result, created_at FROM job_runs \
             WHERE job_alias = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2",
        )?;

        let entry = stmt
            .query_row(rusqlite::params![job_alias, offset], |row| {
                let result: String = row.get(0)?;
                let created_at: String = row.get(1)?;
                Ok(MemoryEntry {
                    result,
                    date: created_at.get(..10).unwrap_or(&created_at).to_string(),
                    datetime: created_at.clone(),
                })
            })
            .optional()?;

        Ok(entry)
    }

    /// Recent memories for a job, newest first.
    pub fn get_memories(&self, job_alias: &str, limit: u32) -> Result<Vec<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT result, created_at FROM job_runs \
             WHERE job_alias = ?1 ORDER BY id DESC LIMIT ?2",
        )?;

        let entries = stmt
            .query_map(rusqlite::params![job_alias, limit], |row| {
                let result: String = row.get(0)?;
                let created_at: String = row.get(1)?;
                Ok(MemoryEntry {
                    result,
                    date: created_at.get(..10).unwrap_or(&created_at).to_string(),
                    datetime: created_at.clone(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Append a message to a session (user or assistant).
    pub fn store_message(
        &self,
        channel: &str,
        sender: &str,
        role: &str,
        content: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (channel, sender, role, content) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![channel, sender, role, content],
        )?;
        Ok(())
    }

    /// Session history for a channel+sender, oldest first, capped at `limit`.
    pub fn get_session(
        &self,
        channel: &str,
        sender: &str,
        limit: u32,
    ) -> Result<Vec<SessionMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT role, content, created_at FROM sessions \
             WHERE channel = ?1 AND sender = ?2 \
             ORDER BY id DESC LIMIT ?3",
        )?;

        let mut entries: Vec<SessionMessage> = stmt
            .query_map(rusqlite::params![channel, sender, limit], |row| {
                Ok(SessionMessage {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    timestamp: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // We query newest-first for the LIMIT, then reverse to get chronological order
        entries.reverse();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dictionary::Dictionary;
    use crate::template::functions::RenderContext;
    use crate::template::render;

    #[test]
    fn test_open_memory() {
        let store = Store::open_memory();
        assert!(store.is_ok());
    }

    #[test]
    fn test_migrations_idempotent() {
        let store = Store::open_memory().unwrap();
        // Should be idempotent
        assert!(store.migrate().is_ok());
    }

    #[test]
    fn test_store_and_get_memory() {
        let store = Store::open_memory().unwrap();
        store.store_run("weather", "sunny and warm").unwrap();

        let entry = store.get_memory("weather", 0).unwrap();
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.result, "sunny and warm");
        assert!(!entry.date.is_empty());
        assert!(!entry.datetime.is_empty());
    }

    #[test]
    fn test_get_memory_offset() {
        let store = Store::open_memory().unwrap();
        store.store_run("weather", "first").unwrap();
        store.store_run("weather", "second").unwrap();
        store.store_run("weather", "third").unwrap();

        let latest = store.get_memory("weather", 0).unwrap().unwrap();
        assert_eq!(latest.result, "third");

        let prev = store.get_memory("weather", 1).unwrap().unwrap();
        assert_eq!(prev.result, "second");

        let oldest = store.get_memory("weather", 2).unwrap().unwrap();
        assert_eq!(oldest.result, "first");
    }

    #[test]
    fn test_get_memory_empty() {
        let store = Store::open_memory().unwrap();
        let entry = store.get_memory("weather", 0).unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn test_get_memories() {
        let store = Store::open_memory().unwrap();
        for i in 1..=5 {
            store.store_run("weather", &format!("run {i}")).unwrap();
        }

        let memories = store.get_memories("weather", 3).unwrap();
        assert_eq!(memories.len(), 3);
        assert_eq!(memories[0].result, "run 5");
        assert_eq!(memories[1].result, "run 4");
        assert_eq!(memories[2].result, "run 3");
    }

    #[test]
    fn test_get_memories_fewer_than_limit() {
        let store = Store::open_memory().unwrap();
        store.store_run("weather", "run 1").unwrap();
        store.store_run("weather", "run 2").unwrap();

        let memories = store.get_memories("weather", 5).unwrap();
        assert_eq!(memories.len(), 2);
    }

    #[tokio::test]
    async fn test_memory_roundtrip() {
        let store = Store::open_memory().unwrap();
        store.store_run("weather", "sunny and warm").unwrap();
        store.store_run("weather", "cloudy with rain").unwrap();

        let memories = store.get_memories("weather", 10).unwrap();
        let mut ctx = RenderContext::new(Dictionary::new());
        ctx.memories = memories;

        // Latest memory
        let result = render("{% memory %}", &ctx).await.unwrap();
        assert_eq!(result, "cloudy with rain");

        // Second most recent
        let result = render("{% memory minus=2 %}", &ctx).await.unwrap();
        assert_eq!(result, "sunny and warm");
    }

    #[test]
    fn test_store_and_get_session() {
        let store = Store::open_memory().unwrap();
        store
            .store_message("#general", "alice", "user", "hello")
            .unwrap();
        store
            .store_message("#general", "alice", "assistant", "hi there")
            .unwrap();
        store
            .store_message("#general", "alice", "user", "how are you?")
            .unwrap();

        let messages = store.get_session("#general", "alice", 10).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "hi there");
        assert_eq!(messages[2].role, "user");
        assert_eq!(messages[2].content, "how are you?");
    }

    #[test]
    fn test_session_limit() {
        let store = Store::open_memory().unwrap();
        for i in 1..=5 {
            store
                .store_message("#ch", "bob", "user", &format!("msg {i}"))
                .unwrap();
        }

        let messages = store.get_session("#ch", "bob", 3).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "msg 3");
        assert_eq!(messages[1].content, "msg 4");
        assert_eq!(messages[2].content, "msg 5");
    }

    #[test]
    fn test_session_sender_isolation() {
        let store = Store::open_memory().unwrap();
        store
            .store_message("#ch", "alice", "user", "alice msg")
            .unwrap();
        store
            .store_message("#ch", "bob", "user", "bob msg")
            .unwrap();

        let alice_msgs = store.get_session("#ch", "alice", 10).unwrap();
        assert_eq!(alice_msgs.len(), 1);
        assert_eq!(alice_msgs[0].content, "alice msg");

        let bob_msgs = store.get_session("#ch", "bob", 10).unwrap();
        assert_eq!(bob_msgs.len(), 1);
        assert_eq!(bob_msgs[0].content, "bob msg");
    }

    #[test]
    fn test_session_channel_isolation() {
        let store = Store::open_memory().unwrap();
        store
            .store_message("#general", "alice", "user", "general msg")
            .unwrap();
        store
            .store_message("#random", "alice", "user", "random msg")
            .unwrap();

        let general = store.get_session("#general", "alice", 10).unwrap();
        assert_eq!(general.len(), 1);
        assert_eq!(general[0].content, "general msg");

        let random = store.get_session("#random", "alice", 10).unwrap();
        assert_eq!(random.len(), 1);
        assert_eq!(random[0].content, "random msg");
    }

    #[test]
    fn test_session_empty() {
        let store = Store::open_memory().unwrap();
        let messages = store.get_session("#ch", "nobody", 10).unwrap();
        assert!(messages.is_empty());
    }
}
