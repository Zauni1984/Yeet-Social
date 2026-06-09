//! WebSocket fan-out hub.
//!
//! The hub is the in-memory registry that connects "an event just
//! happened" (a message was sent / edited / unsent / read /
//! delivered, someone started typing) to "which currently-connected
//! sockets should hear about it".
//!
//! Scope:
//!   * One process, many sockets per user (multi-device + multi-tab).
//!   * No persistence — every reconnect re-syncs via the REST list.
//!   * Server-blind: events carry ciphertext just like REST does, so
//!     the hub never sees plaintext.
//!   * Fan-out is per-user. Callers compute the recipient set (e.g.
//!     "all members of conversation X except the sender") and call
//!     `publish_to_user` for each. That keeps the hub free of any
//!     conversation-membership knowledge.
//!
//! Sender-side privacy hooks
//!   * Read-receipt events are published only when the recipient's
//!     `read_receipts_enabled` is true. The handler in `messages.rs`
//!     gates the broadcast (the row write was already gated).
//!   * Typing events are published only when the typer's
//!     `typing_indicators_enabled` is true. The `ws::handle_typing`
//!     consumer checks this before fan-out.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::{mpsc::UnboundedSender, RwLock};
use uuid::Uuid;

/// Wire event envelope. The `event` field is the type tag the client
/// switches on; `data` is event-specific.
#[derive(Debug, Clone, Serialize)]
pub struct WsEnvelope {
    pub event: String,
    pub data: Value,
}

/// Per-connection identifier. Each websocket upgrade gets one.
/// Used so we can remove the right sender when a connection closes
/// without affecting sibling tabs/devices.
pub type ConnId = u64;

#[derive(Clone)]
pub struct Hub {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    /// user_id → list of (connection_id, sender). We use Vec because
    /// per-user multi-device count is small; a HashMap would be
    /// over-engineered.
    by_user: HashMap<Uuid, Vec<(ConnId, UnboundedSender<WsEnvelope>)>>,
    /// Monotonic counter for `ConnId` minting. Wraps at u64::MAX,
    /// which is effectively never for a single-process lifetime.
    next_id: u64,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                by_user: HashMap::new(),
                next_id: 1,
            })),
        }
    }

    /// Register a connection. Caller keeps the returned `ConnId` to
    /// pass back to `unregister` on close.
    pub async fn register(&self, user_id: Uuid, tx: UnboundedSender<WsEnvelope>) -> ConnId {
        let mut g = self.inner.write().await;
        let id = g.next_id;
        g.next_id = g.next_id.wrapping_add(1);
        g.by_user.entry(user_id).or_default().push((id, tx));
        id
    }

    pub async fn unregister(&self, user_id: Uuid, conn_id: ConnId) {
        let mut g = self.inner.write().await;
        if let Some(v) = g.by_user.get_mut(&user_id) {
            v.retain(|(id, _)| *id != conn_id);
            if v.is_empty() {
                g.by_user.remove(&user_id);
            }
        }
    }

    /// True if any device for this user has an open socket.
    pub async fn is_online(&self, user_id: Uuid) -> bool {
        let g = self.inner.read().await;
        g.by_user.get(&user_id).map(|v| !v.is_empty()).unwrap_or(false)
    }

    /// Fan a single envelope out to every socket of one user. Closed
    /// senders are dropped opportunistically — they can't be detected
    /// until we try to push (the future-side will fail with a closed
    /// channel error).
    pub async fn publish_to_user(&self, user_id: Uuid, env: &WsEnvelope) {
        // We snapshot the senders under a read lock so the actual
        // send isn't done while holding the lock (the channel is
        // unbounded but we still want to keep the critical section
        // tight).
        let snapshot: Vec<UnboundedSender<WsEnvelope>> = {
            let g = self.inner.read().await;
            g.by_user.get(&user_id)
                .map(|v| v.iter().map(|(_, tx)| tx.clone()).collect())
                .unwrap_or_default()
        };
        if snapshot.is_empty() { return; }
        for tx in snapshot {
            let _ = tx.send(env.clone());
        }
    }

    /// Convenience for the typical "fan to many users" case.
    pub async fn publish_to_users(&self, users: &[Uuid], env: &WsEnvelope) {
        for u in users {
            self.publish_to_user(*u, env).await;
        }
    }
}

impl Default for Hub {
    fn default() -> Self { Self::new() }
}
