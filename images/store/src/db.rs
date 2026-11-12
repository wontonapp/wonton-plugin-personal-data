//! SQLite 的打开、表结构和凭据。
//!
//! 每个请求在内存里直接查一次，数据量按一个人的位置和健康样本算，不套连接池：一个进程一个连接，
//! 请求之间用互斥锁排队。

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

/// 打开数据库并建好表。目录已经由核心按 `WONTON_APP_DATA` 准备好，这里只建文件。
pub fn open(dir: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(dir.join("store.db"))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS keys (
             name TEXT PRIMARY KEY,
             key TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS locations (
             user TEXT NOT NULL,
             at_ms INTEGER NOT NULL,
             tst INTEGER NOT NULL,
             tid TEXT NOT NULL,
             wire TEXT NOT NULL,
             PRIMARY KEY (user, tst, tid)
         );
         CREATE INDEX IF NOT EXISTS locations_range ON locations (user, at_ms);
         CREATE TABLE IF NOT EXISTS health (
             user TEXT NOT NULL,
             type_name TEXT NOT NULL,
             start_ms INTEGER NOT NULL,
             source_id TEXT NOT NULL,
             wire TEXT NOT NULL,
             PRIMARY KEY (user, type_name, source_id)
         );
         CREATE INDEX IF NOT EXISTS health_range ON health (user, type_name, start_ms);
         CREATE TABLE IF NOT EXISTS synced (
             user TEXT NOT NULL,
             type_name TEXT NOT NULL,
             through_ms INTEGER NOT NULL,
             PRIMARY KEY (user, type_name)
         );
         CREATE TABLE IF NOT EXISTS retention (
             user TEXT NOT NULL,
             type_name TEXT NOT NULL,
             policy TEXT NOT NULL,
             PRIMARY KEY (user, type_name)
         );",
    )?;
    Ok(conn)
}

/// 凭据命令：`issue` 收下核心给的这一行的值，`revoke` 删掉；本来没有也算成功。
pub fn credentials(
    conn: &Connection,
    action: &str,
    name: &str,
    key: Option<&str>,
) -> rusqlite::Result<()> {
    match action {
        "issue" => {
            conn.execute(
                "INSERT INTO keys (name, key) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET key = excluded.key",
                params![name, key.unwrap_or_default()],
            )?;
            Ok(())
        }
        "revoke" => {
            conn.execute("DELETE FROM keys WHERE name = ?1", params![name])?;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// 请求带的凭据认得出来才算数。按使用方签发，一个使用方一把，认不出就 403。
pub fn authorized(conn: &Connection, header: Option<&str>) -> rusqlite::Result<bool> {
    let Some(key) = header.and_then(|value| value.strip_prefix("Bearer ")) else {
        return Ok(false);
    };
    Ok(conn
        .query_row("SELECT 1 FROM keys WHERE key = ?1", params![key], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?
        .is_some())
}
