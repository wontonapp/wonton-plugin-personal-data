//! 手机位置和健康数据的存储提供方（能力 `personal_data`）。
//!
//! 三个入口：默认起 HTTP 服务；`credentials issue|revoke <名字>` 是核心的凭据命令（密钥走标准输入）；
//! `health` 给容器健康检查用，数据库查得动才算好。

mod api;
mod db;
mod problem;
mod store;
mod wire;

use std::path::PathBuf;
use std::process::ExitCode;

use tokio::net::TcpListener;

const DEFAULT_PORT: u16 = 8090;

// 两个工作线程就够：查询都在一把锁后面排队，宿主机核多时按核数开线程会顶到 pids_limit。
#[tokio::main(worker_threads = 2)]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = data_dir();
    let result = match args.first().map(String::as_str) {
        Some("credentials") => credentials(&dir, &args[1..]),
        Some("health") => health(&dir),
        Some("serve") | None => serve(&dir).await,
        Some(other) => Err(format!(
            "不认识的命令 {other}：用 serve、credentials 或 health"
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn data_dir() -> PathBuf {
    std::env::var_os("DATA_DIR").map_or_else(|| PathBuf::from("/data"), PathBuf::from)
}

fn credentials(dir: &std::path::Path, args: &[String]) -> Result<(), String> {
    let (action, name) = match args {
        [action, name] if action == "issue" || action == "revoke" => {
            (action.as_str(), name.as_str())
        }
        _ => return Err("用法：credentials issue|revoke <名字>".into()),
    };
    let key = if action == "issue" {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| format!("标准输入读不出来：{e}"))?;
        let key = line.trim();
        if key.is_empty() {
            return Err("没有从标准输入拿到凭据".into());
        }
        Some(key.to_owned())
    } else {
        None
    };
    let conn = db::open(dir).map_err(|e| format!("数据库打不开：{e}"))?;
    db::credentials(&conn, action, name, key.as_deref()).map_err(|e| format!("数据库出错：{e}"))
}

fn health(dir: &std::path::Path) -> Result<(), String> {
    let conn = db::open(dir).map_err(|e| format!("数据库打不开：{e}"))?;
    conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
        .map(|_| ())
        .map_err(|e| format!("数据库查不动：{e}"))
}

async fn serve(dir: &std::path::Path) -> Result<(), String> {
    let conn = db::open(dir).map_err(|e| format!("数据库打不开：{e}"))?;
    let app = std::sync::Arc::new(api::App {
        conn: std::sync::Mutex::new(conn),
    });
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("监听 {port} 失败：{e}"))?;
    axum::serve(listener, api::router(app))
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|e| format!("服务退出：{e}"))
}

/// 核心停容器时给 SIGTERM，收尾后退出，别让迁移中的请求硬断。
async fn shutdown() {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}
