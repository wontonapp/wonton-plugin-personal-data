//! HTTP 入口：认凭据、取数据主体、把查询串解析好，再把活交给 [`crate::store`]。
//!
//! 核心是所有请求唯一的调用方（经网关转发或直接连），每个请求都必须带核心签发的凭据和 `X-Wonton-User`。

use std::sync::{Arc, Mutex};

use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::db;
use crate::problem::Problem;
use crate::store;
use crate::wire::{CLEANUP_PATH, DATA_PATH, EXPORT_PATH, HEALTH_PATH, LOCATION_PATH, QUERY_PATH};

pub struct App {
    pub conn: Mutex<Connection>,
}

pub type Shared = Arc<App>;

pub fn router(app: Shared) -> Router {
    Router::new()
        .route(LOCATION_PATH, post(location))
        .route(HEALTH_PATH, post(health))
        .route(QUERY_PATH, get(query))
        .route(DATA_PATH, delete(remove))
        .route(EXPORT_PATH, get(export))
        .route(CLEANUP_PATH, post(cleanup))
        .route("/health", get(health_of_process))
        .with_state(app)
}

/// 通过凭据和 `X-Wonton-User` 之后的调用方。
struct Caller {
    user: String,
}

impl FromRequestParts<Shared> for Caller {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Shared,
    ) -> Result<Self, Self::Rejection> {
        let authorized = {
            let conn = state.conn.lock().expect("数据库锁中毒");
            db::authorized(
                &conn,
                parts
                    .headers
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok()),
            )
            .map_err(|e| Problem::unavailable(format!("数据库出错：{e}")).into_response())?
        };
        if !authorized {
            return Err(Problem::denied().into_response());
        }
        let user = parts
            .headers
            .get(USER_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|user| !user.is_empty())
            .ok_or_else(|| Problem::invalid("缺 X-Wonton-User").into_response())?;
        Ok(Self {
            user: user.to_owned(),
        })
    }
}

/// 契约里数据主体的请求头名。
const USER_HEADER: &str = "X-Wonton-User";

#[derive(Deserialize)]
pub struct WireAsk {
    #[serde(rename = "type")]
    type_name: String,
    from: u64,
    to: u64,
}

async fn location(
    State(state): State<Shared>,
    caller: Caller,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(e) => return Problem::invalid(format!("请求体不是 JSON：{e}")).into_response(),
    };
    let conn = state.conn.lock().expect("数据库锁中毒");
    reply(store::write_location(&conn, &caller.user, body))
}

async fn health(
    State(state): State<Shared>,
    caller: Caller,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(e) => return Problem::invalid(format!("请求体不是 JSON：{e}")).into_response(),
    };
    let conn = state.conn.lock().expect("数据库锁中毒");
    reply(store::write_health(&conn, &caller.user, body))
}

async fn query(
    State(state): State<Shared>,
    caller: Caller,
    ask: Result<Query<WireAsk>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(ask) = match ask {
        Ok(ask) => ask,
        Err(e) => return Problem::invalid(format!("查询串不对：{e}")).into_response(),
    };
    let conn = state.conn.lock().expect("数据库锁中毒");
    reply(store::query(
        &conn,
        &caller.user,
        &ask.type_name,
        ask.from,
        ask.to,
    ))
}

async fn remove(
    State(state): State<Shared>,
    caller: Caller,
    ask: Result<Query<WireAsk>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(ask) = match ask {
        Ok(ask) => ask,
        Err(e) => return Problem::invalid(format!("查询串不对：{e}")).into_response(),
    };
    let conn = state.conn.lock().expect("数据库锁中毒");
    reply(store::delete(
        &conn,
        &caller.user,
        &ask.type_name,
        ask.from,
        ask.to,
    ))
}

async fn export(State(state): State<Shared>, caller: Caller) -> Response {
    let conn = state.conn.lock().expect("数据库锁中毒");
    reply(store::export(&conn, &caller.user))
}

async fn cleanup(
    State(state): State<Shared>,
    caller: Caller,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(e) => return Problem::invalid(format!("请求体不是 JSON：{e}")).into_response(),
    };
    let conn = state.conn.lock().expect("数据库锁中毒");
    reply(store::cleanup(&conn, &caller.user, body))
}

/// 进程自己的健康检查：数据库查得动就算可用。
async fn health_of_process(State(state): State<Shared>) -> Response {
    let ok = {
        let conn = state.conn.lock().expect("数据库锁中毒");
        conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .is_ok()
    };
    if ok {
        (StatusCode::OK, "ok").into_response()
    } else {
        Problem::unavailable("数据库查不动").into_response()
    }
}

fn reply(result: store::Reply) -> Response {
    match result {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(problem) => problem.into_response(),
    }
}
