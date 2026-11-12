//! 能力契约的存储语义：收点、收样本、查询、按时间段删除、按人导出、按期限清理。
//!
//! 数据按 `X-Wonton-User` 的人分开存；授权（哪个 Bot 能读）不在这里判定。回查原样回存下的 JSON，
//! 比本提供方新的字段不丢；重传同一个点（同一个人、同一测量时间、同一设备）或同一个健康来源只留一份。

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::problem::Problem;
use crate::wire::{self, CleanupRequest, HealthSamples, LocationPoint, RetentionPolicy};

pub type Reply = Result<Value, Problem>;

const DAY_MS: i64 = 86_400_000;

/// 收一个位置点。
pub fn write_location(conn: &Connection, user: &str, raw: Value) -> Reply {
    let point: LocationPoint = serde_json::from_value(raw.clone())
        .map_err(|e| Problem::invalid(format!("位置点不对：{e}")))?;
    // 后面只用到测量时间和设备，其余字段靠上面这行校验写法。
    let _ = (point.kind, point.lat, point.lon, point.acc);
    if matches!(
        retention(conn, user, wire::LOCATION)?,
        Some(RetentionPolicy::Never)
    ) {
        return Ok(json!({"accepted": 0}));
    }
    conn.execute(
        "INSERT INTO locations (user, at_ms, tst, tid, wire) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(user, tst, tid) DO UPDATE SET wire = excluded.wire",
        params![
            user,
            (point.tst * 1000) as i64,
            point.tst as i64,
            point.tid,
            raw.to_string()
        ],
    )
    .map_err(db)?;
    Ok(json!({"accepted": 1}))
}

/// 收一批健康样本。同步到的时刻只往前推。
pub fn write_health(conn: &Connection, user: &str, raw: Value) -> Reply {
    let batch: HealthSamples = serde_json::from_value(raw.clone())
        .map_err(|e| Problem::invalid(format!("样本批次不对：{e}")))?;
    let raws = raw["samples"].as_array().cloned().unwrap_or_default();
    let mut accepted = 0;
    for (sample, wire) in batch.samples.into_iter().zip(raws) {
        if !sample.value.valid() {
            return Err(Problem::invalid("健康样本的值不对"));
        }
        let _ = sample.end_ms;
        if matches!(
            retention(conn, user, &sample.type_name)?,
            Some(RetentionPolicy::Never)
        ) {
            continue;
        }
        conn.execute(
            "INSERT INTO health (user, type_name, start_ms, source_id, wire) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(user, type_name, source_id) DO UPDATE SET start_ms = excluded.start_ms, wire = excluded.wire",
            params![user, sample.type_name, sample.start_ms as i64, sample.source_id, wire.to_string()],
        )
        .map_err(db)?;
        accepted += 1;
    }
    if let Some(through) = batch.synced_through_ms {
        for type_name in raw_types(&raw) {
            conn.execute(
                "INSERT INTO synced (user, type_name, through_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT(user, type_name) DO UPDATE SET through_ms = max(synced.through_ms, excluded.through_ms)",
                params![user, type_name, through as i64],
            )
            .map_err(db)?;
        }
    }
    Ok(json!({"accepted": accepted}))
}

/// 一批里出现的类型名，用来把同步时刻记到每个类型上。
fn raw_types(raw: &Value) -> Vec<String> {
    let mut types: Vec<String> = raw["samples"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sample| sample["type_name"].as_str().map(str::to_owned))
        .collect();
    types.sort();
    types.dedup();
    types
}

/// 查一个类型在两端的闭区间里的数据。
pub fn query(conn: &Connection, user: &str, type_name: &str, from_ms: u64, to_ms: u64) -> Reply {
    if type_name == wire::LOCATION {
        let points = strings(
            conn,
            "SELECT wire FROM locations WHERE user = ?1 AND at_ms BETWEEN ?2 AND ?3 ORDER BY at_ms, tid",
            user,
            from_ms,
            to_ms,
        )?;
        return Ok(json!({"kind": "location", "points": points}));
    }
    let samples = strings_typed(
        conn,
        "SELECT wire FROM health WHERE user = ?1 AND type_name = ?2 AND start_ms BETWEEN ?3 AND ?4 ORDER BY start_ms, source_id",
        user,
        type_name,
        from_ms,
        to_ms,
    )?;
    let synced = conn
        .query_row(
            "SELECT through_ms FROM synced WHERE user = ?1 AND type_name = ?2",
            params![user, type_name],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(db)?;
    if samples.is_empty() && synced.is_none() && !known_health(type_name) {
        return Err(Problem::not_supported(type_name));
    }
    Ok(json!({
        "kind": "health",
        "samples": samples,
        "synced_through_ms": synced,
    }))
}

/// 删一个类型在闭区间里的数据（撤回用）。
pub fn delete(conn: &Connection, user: &str, type_name: &str, from_ms: u64, to_ms: u64) -> Reply {
    let removed = if type_name == wire::LOCATION {
        conn.execute(
            "DELETE FROM locations WHERE user = ?1 AND at_ms BETWEEN ?2 AND ?3",
            params![user, from_ms, to_ms],
        )
    } else {
        conn.execute(
            "DELETE FROM health WHERE user = ?1 AND type_name = ?2 AND start_ms BETWEEN ?3 AND ?4",
            params![user, type_name, from_ms, to_ms],
        )
    }
    .map_err(db)?;
    Ok(json!({"deleted": removed}))
}

/// 这个人的全部数据，按时间排。
pub fn export(conn: &Connection, user: &str) -> Reply {
    let all = 0..i64::MAX as u64;
    let location = strings(
        conn,
        "SELECT wire FROM locations WHERE user = ?1 AND at_ms BETWEEN ?2 AND ?3 ORDER BY at_ms, tid",
        user,
        all.start,
        all.end,
    )?;
    let health = strings(
        conn,
        "SELECT wire FROM health WHERE user = ?1 AND start_ms BETWEEN ?2 AND ?3 ORDER BY start_ms, source_id",
        user,
        all.start,
        all.end,
    )?;
    Ok(json!({
        "location": location,
        "health": health,
        "exported_at_ms": now_ms(),
    }))
}

/// 按当前保存期限删到期数据，并把期限记下：`Never` 的类型以后收下就丢。
pub fn cleanup(conn: &Connection, user: &str, raw: Value) -> Reply {
    let request: CleanupRequest =
        serde_json::from_value(raw).map_err(|e| Problem::invalid(format!("清理请求不对：{e}")))?;
    let mut deleted = 0;
    for setting in request.policies {
        let policy = serde_json::to_string(&setting.retention)
            .map_err(|e| Problem::unavailable(e.to_string()))?;
        conn.execute(
            "INSERT INTO retention (user, type_name, policy) VALUES (?1, ?2, ?3)
             ON CONFLICT(user, type_name) DO UPDATE SET policy = excluded.policy",
            params![user, setting.type_name, policy],
        )
        .map_err(db)?;
        let cutoff = match setting.retention {
            RetentionPolicy::Never => i64::MAX,
            RetentionPolicy::Days { days } => now_ms() as i64 - days as i64 * DAY_MS,
            RetentionPolicy::Other => continue,
        };
        deleted += if setting.type_name == wire::LOCATION {
            conn.execute(
                "DELETE FROM locations WHERE user = ?1 AND at_ms < ?2",
                params![user, cutoff],
            )
        } else {
            conn.execute(
                "DELETE FROM health WHERE user = ?1 AND type_name = ?2 AND start_ms < ?3",
                params![user, setting.type_name, cutoff],
            )
        }
        .map_err(db)?;
    }
    Ok(json!({"deleted": deleted}))
}

fn retention(
    conn: &Connection,
    user: &str,
    type_name: &str,
) -> Result<Option<RetentionPolicy>, Problem> {
    let policy = conn
        .query_row(
            "SELECT policy FROM retention WHERE user = ?1 AND type_name = ?2",
            params![user, type_name],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(db)?;
    policy
        .map(|text| {
            serde_json::from_str(&text)
                .map_err(|e| Problem::unavailable(format!("保存期限读不出来：{e}")))
        })
        .transpose()
}

/// 按 SQL（带类型名）取一列原始 JSON。
fn strings_typed(
    conn: &Connection,
    sql: &str,
    user: &str,
    type_name: &str,
    from: u64,
    to: u64,
) -> Result<Vec<Value>, Problem> {
    let mut statement = conn.prepare(sql).map_err(db)?;
    let rows = statement
        .query_map(params![user, type_name, from as i64, to as i64], |row| {
            row.get::<_, String>(0)
        })
        .map_err(db)?;
    let mut values = Vec::new();
    for row in rows {
        let text = row.map_err(db)?;
        values.push(serde_json::from_str(&text).map_err(|e| Problem::unavailable(e.to_string()))?);
    }
    Ok(values)
}

/// 按 SQL 取一列原始 JSON。
fn strings(
    conn: &Connection,
    sql: &str,
    user: &str,
    from: u64,
    to: u64,
) -> Result<Vec<Value>, Problem> {
    let mut statement = conn.prepare(sql).map_err(db)?;
    let rows = statement
        .query_map(params![user, from as i64, to as i64], |row| {
            row.get::<_, String>(0)
        })
        .map_err(db)?;
    let mut values = Vec::new();
    for row in rows {
        let text = row.map_err(db)?;
        values.push(serde_json::from_str(&text).map_err(|e| Problem::unavailable(e.to_string()))?);
    }
    Ok(values)
}

fn known_health(type_name: &str) -> bool {
    wire::HEALTH_TYPES.contains(&type_name)
}

fn db(error: rusqlite::Error) -> Problem {
    Problem::unavailable(format!("数据库出错：{error}"))
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}
