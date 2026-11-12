//! 失败响应：4xx/5xx 加 `{"reason": …, "message": …}`。`reason` 的取值是核心仓库 `wonton-protocol` 里
//! `PersonalDataFailure` 的写法，核心据此分类，文字给人和 Agent 看。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub struct Problem {
    status: StatusCode,
    reason: &'static str,
    message: String,
}

impl Problem {
    /// 请求写法不对：不是 JSON、缺字段、查询串缺项。
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            reason: "provider_unavailable",
            message: message.into(),
        }
    }

    /// 提供方自己出错（数据库坏了这类）。
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            reason: "provider_unavailable",
            message: message.into(),
        }
    }

    /// 不存这个类型。
    pub fn not_supported(type_name: &str) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            reason: "not_supported",
            message: format!("这个提供方不存类型 {type_name}"),
        }
    }

    /// 凭据不对。
    pub fn denied() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            reason: "permission_denied",
            message: "凭据不对：请求没带核心签发的凭据".into(),
        }
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({"reason": self.reason, "message": self.message})),
        )
            .into_response()
    }
}
