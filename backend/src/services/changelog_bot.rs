//! Changelog bot: a system user ("YEET Updates") that publishes release
//! notes as permanent posts.
//!
//! Source of truth is `backend/changelog.json`, embedded at build time —
//! so a deploy of a new image carries its own release notes, and the
//! backend posts every entry it has not posted yet (tracked in
//! `changelog_posts`) right after startup and then hourly. Posting is
//! drip-fed (`CHANGELOG_BOT_MAX_PER_RUN`, default 3) so a backlog does not
//! flood the feed. Posts are English, ≤ 420 chars including hashtags
//! (checked by `cargo test changelog`), `is_permanent`, `lang = 'en'`.
//!
//! Env: CHANGELOG_BOT_ENABLED (default true), CHANGELOG_BOT_USERNAME
//! (default yeet_updates), CHANGELOG_BOT_MAX_PER_RUN (default 3).
use std::time::Duration;
use chrono::{Duration as ChronoDuration, Utc};
use serde::Deserialize;
use tokio::time::interval;
use tracing::{info, warn};
use uuid::Uuid;
use crate::{db::Database, AppState};

const CHANGELOG_JSON: &str = include_str!("../../changelog.json");
pub const MAX_POST_CHARS: usize = 420;
const DEFAULT_TAG: &str = "#YeetUpdate";
const BOT_DISPLAY_NAME: &str = "YEET Updates";
const BOT_BIO: &str = "Official changelog: every fix and new feature that ships to justyeet.it, posted permanently. Not a human, no DMs.";

#[derive(Debug, Deserialize)]
struct Changelog { entries: Vec<Entry> }

#[derive(Debug, Deserialize, Clone)]
pub struct Entry {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Entry {
    /// Post body: text + default tag + entry tags, deduplicated.
    pub fn render(&self) -> String {
        let mut tags: Vec<String> = vec![DEFAULT_TAG.to_string()];
        for t in &self.tags {
            let t = t.trim();
            if t.is_empty() { continue; }
            let t = if t.starts_with('#') { t.to_string() } else { format!("#{t}") };
            if !tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) { tags.push(t); }
        }
        format!("{}\n\n{}", self.text.trim(), tags.join(" "))
    }
}

pub fn entries() -> Vec<Entry> {
    match serde_json::from_str::<Changelog>(CHANGELOG_JSON) {
        Ok(c) => c.entries,
        Err(e) => { warn!("changelog-bot: changelog.json unparsable: {e}"); Vec::new() }
    }
}

/// Validation used by the unit test and at startup: unique ids, sortable
/// order, every rendered post within the 420-char post limit.
pub fn validate(entries: &[Entry]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    let mut prev: Option<&str> = None;
    for e in entries {
        if e.id.trim().is_empty() { return Err("entry with empty id".into()); }
        if !seen.insert(e.id.as_str()) { return Err(format!("duplicate id {}", e.id)); }
        if let Some(p) = prev { if p >= e.id.as_str() { return Err(format!("ids must be ascending: {p} >= {}", e.id)); } }
        prev = Some(e.id.as_str());
        if e.text.trim().is_empty() { return Err(format!("{}: empty text", e.id)); }
        let n = e.render().chars().count();
        if n > MAX_POST_CHARS { return Err(format!("{}: {n} chars > {MAX_POST_CHARS}", e.id)); }
    }
    Ok(())
}

fn enabled() -> bool {
    !matches!(std::env::var("CHANGELOG_BOT_ENABLED").as_deref(), Ok("0") | Ok("false") | Ok("FALSE") | Ok("off"))
}
fn username() -> String {
    std::env::var("CHANGELOG_BOT_USERNAME").ok().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "yeet_updates".into())
}
fn max_per_run() -> i64 {
    std::env::var("CHANGELOG_BOT_MAX_PER_RUN").ok().and_then(|v| v.parse().ok()).filter(|n: &i64| *n > 0).unwrap_or(3)
}

/// Find or create the bot user. It has no wallet, no email and no
/// password, so nobody can log in as it; `is_bot` lets the UI badge it.
pub async fn ensure_bot_user(db: &Database) -> anyhow::Result<Uuid> {
    let name = username();
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE username = $1")
        .bind(&name).fetch_optional(db.pool()).await?
    {
        sqlx::query("UPDATE users SET is_bot = TRUE, is_verified = TRUE WHERE id = $1 AND (is_bot = FALSE OR is_verified = FALSE)")
            .bind(id).execute(db.pool()).await?;
        return Ok(id);
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, display_name, bio, is_bot, is_verified)
         VALUES ($1, $2, $3, TRUE, TRUE) RETURNING id"
    ).bind(&name).bind(BOT_DISPLAY_NAME).bind(BOT_BIO).fetch_one(db.pool()).await?;
    info!("changelog-bot: created bot user @{name} ({id})");
    Ok(id)
}

/// Post up to `max_per_run` unpublished entries, oldest first. Returns
/// how many were posted.
pub async fn publish_pending(db: &Database) -> anyhow::Result<usize> {
    let all = entries();
    if let Err(e) = validate(&all) { warn!("changelog-bot: invalid changelog.json, not posting: {e}"); return Ok(0); }
    let bot_id = ensure_bot_user(db).await?;
    let done: Vec<String> = sqlx::query_scalar("SELECT entry_id FROM changelog_posts").fetch_all(db.pool()).await?;
    let mut posted = 0usize;
    for e in all.iter().filter(|e| !done.contains(&e.id)) {
        if posted as i64 >= max_per_run() { break; }
        let body = e.render();
        let mut tx = db.pool().begin().await?;
        // Claim the entry first; a concurrent replica loses on the PK.
        let claimed = sqlx::query("INSERT INTO changelog_posts (entry_id) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(&e.id).execute(&mut *tx).await?.rows_affected();
        if claimed == 0 { tx.rollback().await?; continue; }
        let post_id: Uuid = sqlx::query_scalar(
            "INSERT INTO posts (author_id, content, media_urls, expires_at, is_adult, is_nft, is_permanent, kind, lang)
             VALUES ($1, $2, '{}', $3, FALSE, FALSE, TRUE, 'text', 'en') RETURNING id"
        ).bind(bot_id).bind(&body).bind(Utc::now() + ChronoDuration::hours(24 * 365 * 100))
        .fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE changelog_posts SET post_id = $2 WHERE entry_id = $1")
            .bind(&e.id).bind(post_id).execute(&mut *tx).await?;
        tx.commit().await?;
        info!("changelog-bot: posted {} as {post_id}", e.id);
        posted += 1;
    }
    Ok(posted)
}

/// Startup + hourly: publish whatever is pending.
pub async fn start_changelog_bot(state: AppState) {
    if !enabled() { info!("changelog-bot: disabled (CHANGELOG_BOT_ENABLED=false)"); return; }
    let mut tick = interval(Duration::from_secs(3600));
    loop {
        tick.tick().await;
        match publish_pending(&state.db).await {
            Ok(0) => {}
            Ok(n) => info!("changelog-bot: published {n} entr{}", if n == 1 { "y" } else { "ies" }),
            Err(e) => warn!("changelog-bot: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_json_is_valid_and_fits() {
        let all = entries();
        assert!(!all.is_empty(), "changelog.json has no entries");
        validate(&all).unwrap_or_else(|e| panic!("changelog.json: {e}"));
        for e in &all {
            let r = e.render();
            assert!(r.contains(DEFAULT_TAG), "{}: default tag missing", e.id);
            assert!(r.chars().count() <= MAX_POST_CHARS);
        }
    }

    #[test]
    fn render_adds_and_dedupes_tags() {
        let e = Entry { id: "x".into(), text: "Hello".into(), tags: vec!["Fix".into(), "#YeetUpdate".into(), "#fix".into(), "".into()] };
        assert_eq!(e.render(), "Hello\n\n#YeetUpdate #Fix");
    }

    #[test]
    fn validate_rejects_overlong_and_unsorted() {
        let long = Entry { id: "2026-01-01-a".into(), text: "x".repeat(430), tags: vec![] };
        assert!(validate(&[long]).is_err());
        let a = Entry { id: "2026-01-02-a".into(), text: "a".into(), tags: vec![] };
        let b = Entry { id: "2026-01-01-b".into(), text: "b".into(), tags: vec![] };
        assert!(validate(&[a, b]).is_err());
    }
}
