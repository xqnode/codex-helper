use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use super::RequestLogEntry;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS request_logs (
    id TEXT PRIMARY KEY,
    time_ms INTEGER NOT NULL,
    time_label TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    provider_name TEXT NOT NULL,
    model TEXT NOT NULL,
    path TEXT NOT NULL,
    client_request_id TEXT NOT NULL DEFAULT '',
    stream INTEGER NOT NULL,
    status INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL,
    cost_yuan REAL,
    cost_label TEXT NOT NULL,
    ok INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_request_logs_time ON request_logs(time_ms);
";

pub struct RequestLogDb {
    conn: Mutex<Connection>,
}

impl RequestLogDb {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let _ = conn.execute(
            "ALTER TABLE request_logs ADD COLUMN client_request_id TEXT NOT NULL DEFAULT ''",
            [],
        );
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn load_recent(&self, limit: usize) -> anyhow::Result<Vec<RequestLogEntry>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, time_ms, time_label, provider_id, provider_name, model, path,
                    client_request_id, stream, status, duration_ms, input_tokens, output_tokens,
                    total_tokens, cost_yuan, cost_label, ok
             FROM request_logs
             ORDER BY time_ms DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_entry)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    pub fn insert(&self, entry: &RequestLogEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO request_logs (
                id, time_ms, time_label, provider_id, provider_name, model, path,
                client_request_id, stream, status, duration_ms, input_tokens, output_tokens,
                total_tokens, cost_yuan, cost_label, ok
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                entry.id,
                entry.time_ms,
                entry.time_label,
                entry.provider_id,
                entry.provider_name,
                entry.model,
                entry.path,
                entry.client_request_id,
                entry.stream as i64,
                entry.status,
                entry.duration_ms,
                entry.input_tokens,
                entry.output_tokens,
                entry.total_tokens,
                entry.cost_yuan,
                entry.cost_label,
                entry.ok as i64,
            ],
        )?;
        Ok(())
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        conn.execute("DELETE FROM request_logs", [])?;
        Ok(())
    }

    pub fn trim(&self, max_entries: usize) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        conn.execute(
            "DELETE FROM request_logs WHERE id IN (
                SELECT id FROM request_logs
                ORDER BY time_ms ASC
                LIMIT MAX(0, (SELECT COUNT(*) FROM request_logs) - ?1)
            )",
            params![max_entries as i64],
        )?;
        Ok(())
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RequestLogEntry> {
    let time_ms: i64 = row.get(1)?;
    Ok(RequestLogEntry {
        id: row.get(0)?,
        time_ms,
        time_label: super::format_time_label(time_ms),
        provider_id: row.get(3)?,
        provider_name: row.get(4)?,
        model: row.get(5)?,
        path: row.get(6)?,
        client_request_id: row.get(7)?,
        stream: row.get::<_, i64>(8)? != 0,
        status: row.get(9)?,
        duration_ms: row.get(10)?,
        input_tokens: row.get(11)?,
        output_tokens: row.get(12)?,
        total_tokens: row.get(13)?,
        cost_yuan: row.get(14)?,
        cost_label: row.get(15)?,
        ok: row.get::<_, i64>(16)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_log::RequestLogEntry;

    fn sample_entry(id: &str, time_ms: i64) -> RequestLogEntry {
        RequestLogEntry {
            id: id.into(),
            time_ms,
            time_label: "12:00:00.000".into(),
            provider_id: "deepseek".into(),
            provider_name: "DeepSeek".into(),
            model: "deepseek-v4-flash".into(),
            path: "/chat/completions".into(),
            client_request_id: String::new(),
            stream: true,
            status: 200,
            duration_ms: 100,
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cost_yuan: Some(0.001),
            cost_label: "约 ¥0.001".into(),
            ok: true,
        }
    }

    #[test]
    fn persists_and_reloads_entries() {
        let dir = std::env::temp_dir().join(format!("codex-helper-log-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("request-log.sqlite");

        {
            let db = RequestLogDb::open(&path).unwrap();
            db.insert(&sample_entry("a", 1)).unwrap();
            db.insert(&sample_entry("b", 2)).unwrap();
            db.trim(300).unwrap();
        }

        let db = RequestLogDb::open(&path).unwrap();
        let loaded = db.load_recent(300).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "a");
        assert_eq!(loaded[1].id, "b");

        db.clear().unwrap();
        assert!(db.load_recent(300).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }
}
