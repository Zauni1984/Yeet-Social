//! WebSocket upgrade endpoint for real-time messaging events.
//!
//! Connection lifecycle
//! ────────────────────
//! 1. Client opens `GET /api/v1/ws` (no Bearer header — browsers can't
//!    set custom headers on the upgrade handshake).
//! 2. Server upgrades the socket immediately and waits up to 5 s for
//!    a JSON auth message `{"type":"auth","token":"<jwt>"}`. Anything
//!    else, including silence, closes the socket.
//! 3. On success the server publishes a `hello` envelope with the
//!    resolved user id and adds the connection to the hub. From then
//!    on it forwards every envelope the hub sends to this user.
//! 4. The client may send `typing` events; nothing else.
//! 5. Idle pings every 30 s; if a pong doesn't come back within 90 s,
//!    the server tears down the socket and the hub entry.
//!
//! The full ciphertext-only invariant is preserved on the wire — the
//! `message.new` envelope is exactly the `MessageDto` the REST
//! endpoint returns. Plaintext lives only in the browser.
//!
//! Privacy gates
//! ─────────────
//! * `typing` fan-out only happens if the typer has
//!   `typing_indicators_enabled = true`.
//! * `receipt.read` fan-out only happens if the reader has
//!   `read_receipts_enabled = true` (mirrors the REST get_receipts
//!   filter so the WS path can't bypass the user's opt-out).
//! * Both gates query the DB; we don't trust the connecting client to
//!   advertise its own preference truthfully.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::services::auth::verify_access_token;
use crate::services::ws_hub::WsEnvelope;
use crate::AppState;

const AUTH_TIMEOUT_SECS: u64 = 5;
const PING_INTERVAL_SECS: u64 = 30;
const PONG_DEADLINE_SECS: u64 = 90;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Auth { token: String },
    Typing {
        conversation_id: Uuid,
        // true = started, false = stopped
        active: bool,
    },
    Ping,
}

pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // ─── Auth handshake ──────────────────────────────────────────────
    let user_id = match auth_handshake(&mut socket, &state).await {
        Some(u) => u,
        None => {
            let _ = socket.close().await;
            return;
        }
    };

    // ack hello so the client knows it's connected
    let _ = socket.send(Message::Text(
        serde_json::to_string(&WsEnvelope {
            event: "hello".into(),
            data: json!({ "user_id": user_id }),
        }).unwrap_or_default()
    )).await;

    // ─── Register with the hub ───────────────────────────────────────
    let (tx, mut rx) = mpsc::unbounded_channel::<WsEnvelope>();
    // Keep a clone so the heartbeat path can ping only THIS connection
    // instead of fanning out to every device the user has open.
    let ping_tx = tx.clone();
    let conn_id = state.ws_hub.register(user_id, tx).await;

    // Split the socket so we can read + write in parallel tasks.
    let (mut ws_tx, mut ws_rx) = socket.split();

    // ─── Outbound forward task: hub → socket ─────────────────────────
    let mut outbound = tokio::spawn(async move {
        while let Some(env) = rx.recv().await {
            let payload = match serde_json::to_string(&env) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if ws_tx.send(Message::Text(payload)).await.is_err() {
                break;
            }
        }
    });

    // ─── Inbound + heartbeat loop ────────────────────────────────────
    let mut last_pong = std::time::Instant::now();
    let mut ping_ticker = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
    ping_ticker.tick().await; // skip the immediate first tick

    'main_loop: loop {
        tokio::select! {
            _ = ping_ticker.tick() => {
                if last_pong.elapsed() > Duration::from_secs(PONG_DEADLINE_SECS) {
                    break 'main_loop;
                }
                // Push the ping straight into this connection's own
                // channel, not via the hub — otherwise every device
                // the same user has open would receive a copy.
                let _ = ping_tx.send(WsEnvelope {
                    event: "ping".into(),
                    data: json!({}),
                });
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if !handle_text(&state, user_id, &t, &mut last_pong).await {
                            break 'main_loop;
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {
                        // We don't accept binary frames; the wire is JSON
                        // only. Silently drop rather than tear the socket
                        // down so a misbehaving extension can't DoS the
                        // connection.
                    }
                    Some(Ok(Message::Ping(p))) => {
                        // Reply with a raw WS pong through the outbound
                        // channel is awkward; the client doesn't send
                        // raw pings, so just keep the loop alive.
                        last_pong = std::time::Instant::now();
                        let _ = p; // unused
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_pong = std::time::Instant::now();
                    }
                    Some(Ok(Message::Close(_))) | None => break 'main_loop,
                    Some(Err(_)) => break 'main_loop,
                }
            }
            _ = &mut outbound => break 'main_loop,
        }
    }

    // ─── Cleanup ─────────────────────────────────────────────────────
    outbound.abort();
    state.ws_hub.unregister(user_id, conn_id).await;
}

/// Read up to one auth frame within the timeout; resolve subject to a
/// user id by reusing the same logic the REST `caller_user_id` uses.
async fn auth_handshake(socket: &mut WebSocket, state: &AppState) -> Option<Uuid> {
    let auth_msg = tokio::time::timeout(
        Duration::from_secs(AUTH_TIMEOUT_SECS),
        socket.recv(),
    ).await.ok()??;

    let text = match auth_msg.ok()? {
        Message::Text(t) => t,
        _ => return None,
    };
    let parsed: ClientMsg = serde_json::from_str(&text).ok()?;
    let token = match parsed {
        ClientMsg::Auth { token } => token,
        _ => return None,
    };

    let claims = verify_access_token(&token, &state.jwt).ok()?;
    if state.cache.is_blacklisted(&claims.jti).await.unwrap_or(false) {
        return None;
    }
    resolve_subject_to_user_id(state, &claims.sub).await
}

async fn resolve_subject_to_user_id(state: &AppState, sub: &str) -> Option<Uuid> {
    if let Some(rest) = sub.strip_prefix("email:") {
        return Uuid::parse_str(rest).ok();
    }
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE wallet_address = $1")
        .bind(sub)
        .fetch_optional(state.db.pool()).await.ok().flatten()
}

/// Returns false when the loop should terminate.
async fn handle_text(
    state: &AppState,
    user_id: Uuid,
    text: &str,
    last_pong: &mut std::time::Instant,
) -> bool {
    let Ok(parsed) = serde_json::from_str::<ClientMsg>(text) else {
        return true; // ignore garbage
    };
    match parsed {
        ClientMsg::Auth { .. } => {
            // re-auth is not supported; close to force a fresh connect
            false
        }
        ClientMsg::Ping => {
            *last_pong = std::time::Instant::now();
            true
        }
        ClientMsg::Typing { conversation_id, active } => {
            handle_typing(state, user_id, conversation_id, active).await;
            true
        }
    }
}

/// Publish a `typing.start` / `typing.stop` event to every other
/// member of the conversation, gated on the typer's privacy toggle.
async fn handle_typing(
    state: &AppState,
    user_id: Uuid,
    conversation_id: Uuid,
    active: bool,
) {
    // Honour the typer's opt-out — silently drop, don't error.
    let typing_on: bool = sqlx::query_scalar(
        "SELECT typing_indicators_enabled FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(state.db.pool()).await.ok().flatten().unwrap_or(true);
    if !typing_on { return; }

    // Membership check — we trust the user only insofar as they're
    // actually a member. A non-member sending typing frames is a noop.
    let is_member: Option<bool> = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_members
                        WHERE conversation_id = $1 AND user_id = $2)"
    )
    .bind(conversation_id).bind(user_id)
    .fetch_optional(state.db.pool()).await.ok().flatten();
    if !is_member.unwrap_or(false) { return; }

    let others: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM conversation_members
          WHERE conversation_id = $1 AND user_id <> $2"
    )
    .bind(conversation_id).bind(user_id)
    .fetch_all(state.db.pool()).await.unwrap_or_default();
    if others.is_empty() { return; }

    let env = WsEnvelope {
        event: if active { "typing.start".into() } else { "typing.stop".into() },
        data: json!({
            "conversation_id": conversation_id,
            "user_id": user_id,
        }),
    };
    state.ws_hub.publish_to_users(&others, &env).await;
}
