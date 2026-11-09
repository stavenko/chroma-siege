//! Chroma Siege game server: one WebSocket per player, run state kept in memory only.
//!
//! Env:
//!   CHROMA_BIND              listen address (default 127.0.0.1:8090)
//!   CHROMA_ALLOWED_ORIGINS   comma-separated Origin whitelist; unset = any origin
//!   RUST_LOG                 log filter (default info)

mod session;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header::ORIGIN, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chroma_protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION};
use session::{now_ms, Session};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

const STATS_INTERVAL: Duration = Duration::from_millis(500);

struct Config {
    allowed_origins: Option<Vec<String>>,
    connections: AtomicU64,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let bind = std::env::var("CHROMA_BIND").unwrap_or_else(|_| "127.0.0.1:8090".to_string());
    let addr: SocketAddr = bind.parse().unwrap_or_else(|e| panic!("CHROMA_BIND={bind} is invalid: {e}"));
    let allowed_origins = std::env::var("CHROMA_ALLOWED_ORIGINS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
    info!(?addr, ?allowed_origins, "starting");
    let cfg = Arc::new(Config { allowed_origins, connections: AtomicU64::new(0) });
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(|| async { "ok" }))
        .with_state(cfg);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    axum::serve(listener, app).await.expect("server failed");
}

async fn ws_handler(ws: WebSocketUpgrade, headers: HeaderMap, State(cfg): State<Arc<Config>>) -> Response {
    let origin = headers.get(ORIGIN).and_then(|v| v.to_str().ok()).map(str::to_string);
    if let Some(allowed) = &cfg.allowed_origins {
        if !origin.as_ref().is_some_and(|o| allowed.contains(o)) {
            warn!(?origin, "rejected origin");
            return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
        }
    }
    let conn = cfg.connections.fetch_add(1, Ordering::Relaxed);
    ws.on_upgrade(move |socket| handle_socket(socket, conn, origin))
}

async fn send(socket: &mut WebSocket, msg: &ServerMsg) -> Result<(), axum::Error> {
    let text = serde_json::to_string(msg).expect("ServerMsg serialization failed");
    socket.send(Message::Text(text.into())).await
}

enum Outcome {
    Reply(Vec<ServerMsg>),
    /// Protocol violation: tell the client why and drop the connection.
    Fatal(String),
}

fn process(session: &mut Option<Session>, msg: ClientMsg, conn: u64) -> Outcome {
    match msg {
        ClientMsg::StartRun { version } => {
            if version != PROTOCOL_VERSION {
                return Outcome::Fatal(format!("protocol version {version} != server {PROTOCOL_VERSION}"));
            }
            let s = Session::new(now_ms() ^ (conn << 32));
            let started_at = s.started_at;
            *session = Some(s);
            info!(conn, "run started");
            Outcome::Reply(vec![ServerMsg::RunStarted { started_at }])
        }
        ClientMsg::Events { events } => match session {
            Some(s) => Outcome::Reply(s.apply(&events)),
            None => Outcome::Fatal("events sent before StartRun".to_string()),
        },
        ClientMsg::Claim { quest_id } => match session {
            Some(s) => match s.claim(quest_id) {
                Ok(reward) => {
                    info!(conn, quest_id, ?reward, "reward claimed");
                    Outcome::Reply(vec![ServerMsg::RewardGranted { quest_id, reward }, s.snapshot()])
                }
                Err(message) => {
                    warn!(conn, quest_id, %message, "bad claim");
                    Outcome::Reply(vec![ServerMsg::Error { message }])
                }
            },
            None => Outcome::Fatal("claim sent before StartRun".to_string()),
        },
        ClientMsg::EndRun => {
            if let Some(s) = session.take() {
                info!(conn, waves = s.stats.waves, quests = s.stats.quests_completed, "run ended");
            }
            Outcome::Reply(Vec::new())
        }
    }
}

async fn handle_socket(mut socket: WebSocket, conn: u64, origin: Option<String>) {
    info!(conn, ?origin, "connected");
    let mut session: Option<Session> = None;
    let mut tick = tokio::time::interval(STATS_INTERVAL);
    let reason = loop {
        tokio::select! {
            incoming = socket.recv() => {
                let msg = match incoming {
                    None => break "client went away".to_string(),
                    Some(Err(e)) => break format!("socket error: {e}"),
                    Some(Ok(m)) => m,
                };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Close(_) => break "client closed".to_string(),
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Binary(_) => {
                        let message = "binary frames are not supported".to_string();
                        // Best effort: the connection is dropped right after either way.
                        let _ = send(&mut socket, &ServerMsg::Error { message: message.clone() }).await;
                        break message;
                    }
                };
                let parsed: ClientMsg = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        let message = format!("malformed message: {e}");
                        let _ = send(&mut socket, &ServerMsg::Error { message: message.clone() }).await;
                        break message;
                    }
                };
                match process(&mut session, parsed, conn) {
                    Outcome::Reply(replies) => {
                        let mut failed = None;
                        for r in &replies {
                            if let Err(e) = send(&mut socket, r).await {
                                failed = Some(format!("send failed: {e}"));
                                break;
                            }
                        }
                        if let Some(f) = failed { break f; }
                    }
                    Outcome::Fatal(message) => {
                        let _ = send(&mut socket, &ServerMsg::Error { message: message.clone() }).await;
                        break message;
                    }
                }
            }
            _ = tick.tick() => {
                let snapshot = match session.as_mut() {
                    Some(s) if s.dirty => { s.dirty = false; Some(s.snapshot()) }
                    _ => None,
                };
                if let Some(m) = snapshot {
                    if let Err(e) = send(&mut socket, &m).await {
                        break format!("send failed: {e}");
                    }
                }
            }
        }
    };
    info!(conn, %reason, "disconnected");
}
