//! API handlers grouped by domain.
//
// Many handlers use ad-hoc tuples for `sqlx::query_as` results to avoid
// declaring a single-use FromRow struct. Clippy's `type_complexity`
// lint flags those tuples; suppressing it here keeps CI clean while
// preserving the lint everywhere else.
#![allow(clippy::type_complexity)]
pub mod auth;
pub mod feed;
pub mod middleware;
pub mod posts;
pub mod tips;
pub mod tokens;
pub mod users;
pub mod boards;
pub mod notifications;
pub mod email_auth;
pub mod link_preview;
pub mod report;
pub mod permanent;
pub mod settings;
pub mod uploads;
pub mod paper_wallets;
pub mod blocks;
pub mod e2ee;
pub mod conversations;
pub mod messages;
pub mod invitations;
pub mod search;
pub mod admin_mod;
pub mod lives;
pub mod scheduled_posts;
pub mod message_reports;
pub mod sessions;
pub mod messaging_prefs;
pub mod ws;
